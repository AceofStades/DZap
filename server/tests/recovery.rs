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
