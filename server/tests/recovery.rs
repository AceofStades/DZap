//! Recovery assessment API tests. These requests never read or modify a real disk.

use serde_json::{Value, json};
use tokio::net::TcpListener;

async fn spawn_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = server::build_router(server::realtime::Hub::new());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}")
}

#[tokio::test]
async fn unknown_recovery_source_returns_a_structured_block() {
    let base = spawn_server().await;
    let response = reqwest::Client::new()
        .post(format!("{base}/api/recovery/assess"))
        .json(&json!({"devicePath": "/dev/dzap-nonexistent-recovery"}))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["decision"], "blocked");
    assert_eq!(body["devicePath"], "/dev/dzap-nonexistent-recovery");
    assert_eq!(body["checks"][0]["code"], "device_exists");
    assert_eq!(body["checks"][0]["status"], "blocked");
    assert!(body["identity"].is_null());
    assert_eq!(body["sample"]["requestedSamples"], 0);
}

#[tokio::test]
async fn malformed_recovery_request_returns_json_error() {
    let base = spawn_server().await;
    let response = reqwest::Client::new()
        .post(format!("{base}/api/recovery/assess"))
        .header("Content-Type", "application/json")
        .body("not json")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Invalid request body")
    );
}

#[tokio::test]
async fn recovery_destination_discovery_requires_a_source() {
    let base = spawn_server().await;
    let response = reqwest::Client::new()
        .get(format!("{base}/api/recovery/destinations"))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Invalid query parameters")
    );
}

#[tokio::test]
async fn missing_recovery_source_returns_a_blocked_image_plan() {
    let base = spawn_server().await;
    let response = reqwest::Client::new()
        .post(format!("{base}/api/recovery/plan"))
        .json(&json!({
            "sourceDevicePath": "/dev/dzap-missing-source",
            "expectedSourceIdentity": {
                "model": "Missing",
                "serial": "MISSING",
                "wwn": "",
                "sizeBytes": "1048576",
                "transport": "usb",
                "majorMinor": "8:240"
            },
            "destination": {
                "drivePath": "/dev/dzap-missing-target",
                "devicePath": "/dev/dzap-missing-target1",
                "deviceMajorMinor": "8:241",
                "mountPath": null,
                "filesystem": "ext4",
                "sizeBytes": "2097152",
                "driveIdentity": {
                    "model": "Missing target",
                    "serial": "MISSING-TARGET",
                    "wwn": "",
                    "sizeBytes": "2097152",
                    "transport": "usb",
                    "majorMinor": "8:241"
                },
                "filesystemSizeBytes": null,
                "availableBytes": null,
                "readOnly": null,
                "capacityError": null
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["decision"], "blocked");
    assert_eq!(body["sourceIdentity"], Value::Null);
    assert!(
        body["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| { check["code"] == "source_present" && check["status"] == "blocked" })
    );
}

#[tokio::test]
async fn malformed_recovery_mount_request_returns_json_error() {
    let base = spawn_server().await;
    let response = reqwest::Client::new()
        .post(format!("{base}/api/recovery/destinations/mount"))
        .header("Content-Type", "application/json")
        .body("not json")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Invalid request body")
    );
}

#[tokio::test]
async fn recovery_jobs_list_starts_empty() {
    let base = spawn_server().await;
    let response = reqwest::get(format!("{base}/api/recovery/jobs"))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body, json!([]));
}

#[tokio::test]
async fn unknown_recovery_job_actions_return_not_found() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let id = "recovery-00000000000000000000000000000000";

    let detail = client
        .get(format!("{base}/api/recovery/jobs/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(detail.status(), 404);

    for action in ["pause", "cancel", "resume"] {
        let response = client
            .post(format!("{base}/api/recovery/jobs/{id}/{action}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 404, "action {action}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"], "Recovery job not found");
    }

    let volumes = client
        .get(format!("{base}/api/recovery/jobs/{id}/volumes"))
        .send()
        .await
        .unwrap();
    assert_eq!(volumes.status(), 404);

    let analysis = client
        .post(format!("{base}/api/recovery/jobs/{id}/analyze"))
        .send()
        .await
        .unwrap();
    assert_eq!(analysis.status(), 404);
}

#[tokio::test]
async fn malformed_recovery_start_request_returns_json_error() {
    let base = spawn_server().await;
    let response = reqwest::Client::new()
        .post(format!("{base}/api/recovery/jobs"))
        .header("Content-Type", "application/json")
        .body("not json")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Invalid request body")
    );
}

#[tokio::test]
async fn blocked_recovery_start_does_not_create_a_job() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base}/api/recovery/jobs"))
        .json(&json!({
            "sourceDevicePath": "/dev/dzap-missing-source",
            "expectedSourceIdentity": {
                "model": "Missing",
                "serial": "MISSING",
                "wwn": "",
                "sizeBytes": "1048576",
                "transport": "usb",
                "majorMinor": "8:240"
            },
            "destination": {
                "drivePath": "/dev/dzap-missing-target",
                "devicePath": "/dev/dzap-missing-target1",
                "deviceMajorMinor": "8:241",
                "mountPath": null,
                "filesystem": "ext4",
                "sizeBytes": "2097152",
                "driveIdentity": {
                    "model": "Missing target",
                    "serial": "MISSING-TARGET",
                    "wwn": "",
                    "sizeBytes": "2097152",
                    "transport": "usb",
                    "majorMinor": "8:241"
                },
                "filesystemSizeBytes": null,
                "availableBytes": null,
                "readOnly": null,
                "capacityError": null
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 412);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["decision"], "blocked");

    let jobs: Value = client
        .get(format!("{base}/api/recovery/jobs"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(jobs, json!([]));
}

#[tokio::test]
async fn malformed_recovery_extraction_request_returns_json_error() {
    let base = spawn_server().await;
    let id = "recovery-00000000000000000000000000000000";
    let response = reqwest::Client::new()
        .post(format!("{base}/api/recovery/jobs/{id}/recover"))
        .header("Content-Type", "application/json")
        .body("not json")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Invalid request body")
    );
}
