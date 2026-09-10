// Port of server-go/core/predict.go
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;

use super::drives::log_line;

/// Raw and normalized values for a S.M.A.R.T. attribute
#[derive(Debug, Clone, Serialize)]
pub struct SmartAttribute {
    pub name: String,
    pub value: i64,
}

/// What we send to the frontend
#[derive(Debug, Clone, Serialize)]
pub struct PredictionResult {
    #[serde(rename = "predictedStatus")]
    pub predicted_status: String,
    #[serde(rename = "failureProbability")]
    pub failure_probability: f32,
    #[serde(rename = "smartStatus")]
    pub smart_status: String,
    #[serde(rename = "smartAttributes")]
    pub smart_attributes: HashMap<String, SmartAttribute>,
}

// Wrapper to parse smartctl's ata_smart_attributes table with raw.value.
#[derive(Debug, Deserialize)]
pub(crate) struct AtaAttributeRaw {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    value: i64,
    #[serde(default)]
    raw: AtaRawValue,
}

#[derive(Debug, Default, Deserialize)]
struct AtaRawValue {
    #[serde(default)]
    value: i64,
}

pub fn predict_drive_health(device_path: &str) -> Result<PredictionResult, String> {
    let out = Command::new("smartctl")
        .args(["-a", "-j", device_path])
        .output()
        .map_err(|e| e.to_string());

    let out = match out {
        Ok(o) if o.status.success() => o,
        _ => {
            // If smartctl fails, it could be a USB drive or a device that
            // doesn't support S.M.A.R.T.
            return Ok(PredictionResult {
                predicted_status: "N/A".to_string(),
                failure_probability: 0.0,
                smart_status: "Not available".to_string(),
                smart_attributes: HashMap::new(),
            });
        }
    };

    prediction_from_smartctl_output(&out.stdout, true)
}

pub(crate) fn prediction_from_smartctl_output(
    stdout: &[u8],
    run_model: bool,
) -> Result<PredictionResult, String> {
    let json: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|e| format!("failed to parse S.M.A.R.T. data: {e}"))?;

    let mut result = PredictionResult {
        predicted_status: "Healthy".to_string(), // Default to Healthy
        failure_probability: 0.0,
        smart_status: String::new(),
        smart_attributes: HashMap::new(),
    };

    let passed = json
        .pointer("/smart_status/passed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if passed {
        result.smart_status = "Passed".to_string();
    } else {
        result.smart_status = "Failing".to_string();
        result.predicted_status = "At Risk".to_string();
    }

    let device_type = json
        .pointer("/device/type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match device_type {
        "nvme" => {
            let log = &json["nvme_smart_health_information_log"];
            let get = |key: &str| log.get(key).and_then(|v| v.as_i64()).unwrap_or(0);
            for (name, key) in [
                ("Temperature", "temperature"),
                ("Percentage Used", "percentage_used"),
                ("Data Units Written", "data_units_written"),
                ("Power On Hours", "power_on_hours"),
            ] {
                result.smart_attributes.insert(
                    name.to_string(),
                    SmartAttribute {
                        name: name.to_string(),
                        value: get(key),
                    },
                );
            }
        }
        "sat" => {
            // SATA drive: we can use the ONNX model
            let table: Vec<AtaAttributeRaw> = serde_json::from_value(
                json.pointer("/ata_smart_attributes/table")
                    .cloned()
                    .unwrap_or(serde_json::json!([])),
            )
            .unwrap_or_default();
            for attr in &table {
                if matches!(attr.id, 5 | 9 | 177 | 194 | 241) {
                    result.smart_attributes.insert(
                        attr.name.clone(),
                        SmartAttribute {
                            name: attr.name.clone(),
                            value: attr.raw.value,
                        },
                    );
                }
            }
            if run_model {
                run_sata_prediction(&mut result, &table);
            }
        }
        _ => {
            result.smart_status = "Not supported".to_string();
        }
    }

    Ok(result)
}

/// Runs the ONNX model for SATA drives.
/// Attempts the prediction but never fails hard: on any error it logs a
/// warning and returns the result with just the S.M.A.R.T. data.
fn run_sata_prediction(result: &mut PredictionResult, table: &[AtaAttributeRaw]) {
    if let Err(e) = try_run_sata_prediction(result, table) {
        log_line(&format!("Warning: SATA prediction skipped: {e}"));
    }
}

fn try_run_sata_prediction(
    result: &mut PredictionResult,
    table: &[AtaAttributeRaw],
) -> Result<(), String> {
    let feature_map_file = std::fs::read_to_string("../model/feature_map.json")
        .map_err(|e| format!("could not load feature_map.json: {e}"))?;
    let feature_map: HashMap<String, String> = serde_json::from_str(&feature_map_file)
        .map_err(|e| format!("could not parse feature_map.json: {e}"))?;

    let input_tensor = build_sata_input(&feature_map, table);

    let n = input_tensor.len();
    let input = ort::value::Tensor::from_array(([1usize, n], input_tensor))
        .map_err(|e| format!("failed to create ONNX input tensor: {e}"))?;

    let mut session = ort::session::Session::builder()
        .and_then(|mut b| b.commit_from_file("../model/drive_failure_model.onnx"))
        .map_err(|e| format!("failed to create ONNX session: {e}"))?;

    let outputs = session
        .run(ort::inputs![input])
        .map_err(|e| format!("failed to run model prediction: {e}"))?;

    let (_, probabilities) = outputs["output_probability"]
        .try_extract_tensor::<f32>()
        .map_err(|e| format!("failed to read ONNX output tensor: {e}"))?;

    let failure_probability = probabilities.get(1).copied().unwrap_or(0.0);

    apply_failure_probability(result, failure_probability);
    Ok(())
}

pub(crate) fn apply_failure_probability(result: &mut PredictionResult, failure_probability: f32) {
    result.failure_probability = failure_probability;
    if failure_probability > 0.5 {
        result.predicted_status = "At Risk".to_string();
    } else {
        result.predicted_status = "Healthy".to_string();
    }
}

pub(crate) fn build_sata_input(
    feature_map: &HashMap<String, String>,
    table: &[AtaAttributeRaw],
) -> Vec<f32> {
    let mut feature_names: Vec<String> = vec![String::new(); feature_map.len()];
    for (key, value) in feature_map {
        let index: usize = match value.trim_start_matches('f').parse() {
            Ok(i) => i,
            Err(_) => {
                log_line(&format!(
                    "Warning: could not parse feature index from {value}"
                ));
                continue;
            }
        };
        if index < feature_names.len() {
            feature_names[index] = key.clone();
        }
    }

    let mut smart_values: HashMap<String, i64> = HashMap::new();
    for attr in table {
        smart_values.insert(format!("smart_{}_raw", attr.id), attr.raw.value);
        smart_values.insert(format!("smart_{}_normalized", attr.id), attr.value);
    }

    feature_names
        .iter()
        .map(|feature| smart_values.get(feature).copied().unwrap_or(0) as f32)
        .collect()
}
