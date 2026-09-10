use crate::core::predict::{
    AtaAttributeRaw, PredictionResult, apply_failure_probability, build_sata_input,
    prediction_from_smartctl_output,
};
use std::collections::HashMap;

#[test]
fn nvme_smart_json_maps_status_attributes_and_frontend_shape() {
    let input = br#"{
        "device":{"type":"nvme"},
        "smart_status":{"passed":true},
        "nvme_smart_health_information_log":{
            "temperature":39,
            "percentage_used":7,
            "data_units_written":123456,
            "power_on_hours":321
        }
    }"#;

    let result = prediction_from_smartctl_output(input, false).unwrap();
    assert_eq!(result.predicted_status, "Healthy");
    assert_eq!(result.failure_probability, 0.0);
    assert_eq!(result.smart_status, "Passed");
    assert_eq!(result.smart_attributes.len(), 4);
    assert_eq!(result.smart_attributes["Temperature"].value, 39);
    assert_eq!(result.smart_attributes["Percentage Used"].value, 7);
    assert_eq!(result.smart_attributes["Data Units Written"].value, 123456);
    assert_eq!(result.smart_attributes["Power On Hours"].value, 321);

    let json = serde_json::to_value(result).unwrap();
    let keys: std::collections::HashSet<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        [
            "predictedStatus",
            "failureProbability",
            "smartStatus",
            "smartAttributes",
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn failing_smart_status_marks_drive_at_risk() {
    let result = prediction_from_smartctl_output(
        br#"{"device":{"type":"nvme"},"smart_status":{"passed":false}}"#,
        false,
    )
    .unwrap();

    assert_eq!(result.smart_status, "Failing");
    assert_eq!(result.predicted_status, "At Risk");
    assert_eq!(result.smart_attributes["Temperature"].value, 0);
}

#[test]
fn sata_smart_json_keeps_only_dashboard_attributes() {
    let input = br#"{
        "device":{"type":"sat"},
        "smart_status":{"passed":true},
        "ata_smart_attributes":{"table":[
            {"id":5,"name":"Reallocated_Sector_Ct","value":90,"raw":{"value":4}},
            {"id":9,"name":"Power_On_Hours","value":99,"raw":{"value":800}},
            {"id":177,"name":"Wear_Leveling_Count","value":88,"raw":{"value":12}},
            {"id":194,"name":"Temperature_Celsius","value":61,"raw":{"value":39}},
            {"id":241,"name":"Total_LBAs_Written","value":100,"raw":{"value":9000}},
            {"id":12,"name":"Power_Cycle_Count","value":99,"raw":{"value":14}}
        ]}
    }"#;

    let result = prediction_from_smartctl_output(input, false).unwrap();
    assert_eq!(result.smart_status, "Passed");
    assert_eq!(result.smart_attributes.len(), 5);
    assert_eq!(result.smart_attributes["Reallocated_Sector_Ct"].value, 4);
    assert_eq!(result.smart_attributes["Power_On_Hours"].value, 800);
    assert!(!result.smart_attributes.contains_key("Power_Cycle_Count"));
}

#[test]
fn sata_feature_tensor_follows_feature_map_indices() {
    let table: Vec<AtaAttributeRaw> = serde_json::from_value(serde_json::json!([
        {"id": 5, "name": "Reallocated", "value": 91, "raw": {"value": 3}},
        {"id": 9, "name": "Hours", "value": 99, "raw": {"value": 700}}
    ]))
    .unwrap();
    let feature_map = HashMap::from([
        ("smart_9_raw".to_string(), "f0".to_string()),
        ("smart_5_normalized".to_string(), "f1".to_string()),
        ("missing_feature".to_string(), "f2".to_string()),
        ("smart_5_raw".to_string(), "bad-index".to_string()),
    ]);

    let input = build_sata_input(&feature_map, &table);
    assert_eq!(input, [700.0, 91.0, 0.0, 0.0]);
}

#[test]
fn unsupported_device_and_invalid_json_are_handled() {
    let unsupported = prediction_from_smartctl_output(
        br#"{"device":{"type":"scsi"},"smart_status":{"passed":true}}"#,
        false,
    )
    .unwrap();
    assert_eq!(unsupported.predicted_status, "Healthy");
    assert_eq!(unsupported.smart_status, "Not supported");
    assert!(unsupported.smart_attributes.is_empty());

    let err = prediction_from_smartctl_output(b"{broken", false).unwrap_err();
    assert!(
        err.starts_with("failed to parse S.M.A.R.T. data:"),
        "got: {err}"
    );
}

#[test]
fn model_probability_above_half_marks_drive_at_risk() {
    let mut result = PredictionResult {
        predicted_status: "unknown".to_string(),
        failure_probability: 0.0,
        smart_status: "Passed".to_string(),
        smart_attributes: HashMap::new(),
    };

    apply_failure_probability(&mut result, 0.5);
    assert_eq!(result.predicted_status, "Healthy");
    assert_eq!(result.failure_probability, 0.5);

    apply_failure_probability(&mut result, 0.500_001);
    assert_eq!(result.predicted_status, "At Risk");
    assert_eq!(result.failure_probability, 0.500_001);
}
