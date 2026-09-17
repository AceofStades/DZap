//! Integration tests: drive the real axum router over HTTP + WebSocket.
//!
//! SAFETY MODEL: these tests never touch a real block device.
//! - /api/wipe is only ever called with `/dev/nonexistent-*` paths, which
//!   are blocked by preflight BEFORE a wipe job is created.
//! - The real overwrite logic against a writable target is covered by unit
//!   tests (temp files in /tmp) and by the QEMU end-to-end harness.

use serde_json::{Value, json};
use std::path::PathBuf;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::{Error as WebSocketError, client::IntoClientRequest};

/// Spin up the real router on an ephemeral localhost port.
async fn spawn_server() -> String {
    let hub = server::realtime::Hub::new();
    spawn_app(server::build_router(hub)).await
}

async fn spawn_app(app: axum::Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

fn temp_directory(test_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dzap-api-{test_name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[tokio::test]
async fn drives_endpoint_returns_storage_and_mobile_keys() {
    let base = spawn_server().await;
    let resp = reqwest::get(format!("{base}/api/drives")).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await.unwrap();
    assert!(body.get("storage").is_some(), "missing 'storage': {body}");
    assert!(body.get("mobile").is_some(), "missing 'mobile': {body}");

    // If storage detection worked, drives must match the Go JSON shape.
    if let Some(drives) = body["storage"].as_array() {
        for d in drives {
            for key in [
                "name",
                "model",
                "serial",
                "wwn",
                "size",
                "transport",
                "majorMinor",
                "type",
                "isMounted",
                "isFrozen",
                "isOSDrive",
                "activeDependencies",
                "partitions",
            ] {
                assert!(d.get(key).is_some(), "drive missing {key}: {d}");
            }
            assert!(d["name"].as_str().unwrap().starts_with("/dev/"));
        }
    }
}

#[tokio::test]
async fn wipe_methods_for_unknown_device_is_404_json_error() {
    let base = spawn_server().await;
    let resp = reqwest::get(format!("{base}/api/drive/nonexistent0/wipe-methods"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("not found"),
        "unexpected: {body}"
    );
}

#[tokio::test]
async fn health_for_unknown_device_reports_na_not_500() {
    // smartctl fails on a nonexistent device; the Go server returns 200
    // with predictedStatus "N/A" in that case.
    let base = spawn_server().await;
    let resp = reqwest::get(format!("{base}/api/drive/nonexistent0/health"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["predictedStatus"], json!("N/A"));
    assert_eq!(body["smartStatus"], json!("Not available"));
    assert!(body["smartAttributes"].as_object().unwrap().is_empty());
}

#[tokio::test]
async fn wipe_preflight_returns_structured_block_for_unknown_device() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/wipe/preflight"))
        .json(&json!({
            "DevicePath": "/dev/nonexistent0",
            "Method": "overwrite_1_pass",
            "DeviceSerial": "",
            "DeviceType": "",
            "DeviceModel": "Integration Test",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"], json!("blocked"));
    assert_eq!(body["devicePath"], json!("/dev/nonexistent0"));
    assert_eq!(body["checks"][0]["code"], json!("device_exists"));
    assert_eq!(body["checks"][0]["status"], json!("blocked"));
}

#[tokio::test]
async fn wipe_rejects_unknown_device_before_creating_job() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/wipe"))
        .json(&json!({
            "DevicePath": "/dev/nonexistent0",
            "Method": "overwrite_1_pass",
            "DeviceSerial": "",
            "DeviceType": "",
            "DeviceModel": "Integration Test",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 412);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["decision"], json!("blocked"));
    assert_eq!(body["checks"][0]["code"], json!("device_exists"));
}

#[tokio::test]
async fn wipe_rejects_malformed_body_with_json_error() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/wipe"))
        .header("Content-Type", "application/json")
        .body("this is not json")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("Invalid request body")
    );
}

#[tokio::test]
async fn pause_and_abort_without_active_wipe_error_cleanly() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    for endpoint in ["pause", "abort"] {
        let resp = client
            .post(format!("{base}/api/wipe/{endpoint}"))
            .json(&json!({"deviceId": "/dev/nonexistent0"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 500, "endpoint {endpoint}");
        let body: Value = resp.json().await.unwrap();
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("no active wipe found"),
            "endpoint {endpoint}: {body}"
        );
    }
}

#[tokio::test]
async fn unmount_unknown_device_errors_without_side_effects() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/unmount"))
        .json(&json!({"device": "/dev/nonexistent0"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 500);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("not found in lsblk output"),
        "unexpected: {body}"
    );
}

#[tokio::test]
async fn certificates_list_is_json_array() {
    let base = spawn_server().await;
    let resp = reqwest::get(format!("{base}/api/certificates"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(body.is_array(), "expected array, got: {body}");
}

#[tokio::test]
async fn certificate_rejects_client_supplied_device_claims() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/certificate/generate"))
        .json(&json!({"model": "M", "serial": "S", "method": "overwrite_1_pass"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("missing field `jobId`")
    );
}

#[tokio::test]
async fn certificate_requires_an_existing_server_job() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/certificate"))
        .json(&json!({"jobId": "job-client-invented"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["error"], json!("Wipe job not found"));
}

#[tokio::test]
async fn evidence_export_requires_an_existing_server_job() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/evidence/export"))
        .json(&json!({
            "jobId": "job-00000000000000000000000000000000",
            "destination": {
                "drivePath": "/dev/sdz",
                "driveMajorMinor": "8:240",
                "devicePath": "/dev/sdz1",
                "deviceMajorMinor": "8:241",
                "mountPath": "/mnt/evidence",
                "model": "Evidence USB",
                "serial": "EXPORT-SERIAL",
                "transport": "usb",
                "filesystem": "vfat",
                "sizeBytes": "1048576"
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    assert_eq!(
        resp.json::<Value>().await.unwrap()["error"],
        json!("Wipe job not found")
    );
}

#[tokio::test]
async fn evidence_mount_requires_complete_discovered_identity() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/evidence/mount"))
        .json(&json!({"destination": {"devicePath": "/dev/sdz1"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
    assert!(
        resp.json::<Value>().await.unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("missing field")
    );
}

#[tokio::test]
async fn wipe_jobs_start_empty_and_unknown_job_is_404() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let list = client
        .get(format!("{base}/api/wipe/jobs"))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), 200);
    assert_eq!(list.json::<Value>().await.unwrap(), json!([]));

    let missing = client
        .get(format!("{base}/api/wipe/jobs/job-missing"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    assert_eq!(
        missing.json::<Value>().await.unwrap()["error"],
        json!("Wipe job not found")
    );
}

#[tokio::test]
async fn cors_allows_only_loopback_frontend() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    // The frontend on :3000 preflights POST /api/wipe.
    let resp = client
        .request(reqwest::Method::OPTIONS, format!("{base}/api/wipe"))
        .header("Origin", "http://localhost:3000")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let allow = resp
        .headers()
        .get("access-control-allow-origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(allow, "http://localhost:3000");

    let rejected = client
        .request(reqwest::Method::OPTIONS, format!("{base}/api/wipe"))
        .header("Origin", "https://attacker.example")
        .header("Access-Control-Request-Method", "POST")
        .header("Access-Control-Request-Headers", "content-type")
        .send()
        .await
        .unwrap();
    assert!(
        rejected
            .headers()
            .get("access-control-allow-origin")
            .is_none(),
        "untrusted origin received CORS access: {:?}",
        rejected.headers()
    );
}

#[tokio::test]
async fn websocket_rejects_untrusted_browser_origin() {
    let base = spawn_server().await;
    let ws_url = format!("{}/ws", base.replacen("http://", "ws://", 1));
    let mut request = ws_url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Origin", "https://attacker.example".parse().unwrap());

    let error = tokio_tungstenite::connect_async(request).await.unwrap_err();
    match error {
        WebSocketError::Http(response) => assert_eq!(response.status(), 403),
        other => panic!("unexpected WebSocket error: {other}"),
    }
}

#[tokio::test]
async fn websocket_accepts_live_usb_ui_origin() {
    let base = spawn_server().await;
    let ws_url = format!("{}/ws", base.replacen("http://", "ws://", 1));
    let mut request = ws_url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Origin", "http://127.0.0.1:8080".parse().unwrap());

    let (_socket, response) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(response.status(), 101);
}

#[tokio::test]
async fn backend_serves_the_exported_frontend() {
    let frontend = temp_directory("frontend");
    std::fs::create_dir_all(frontend.join("_next/static")).unwrap();
    std::fs::write(
        frontend.join("index.html"),
        "<!doctype html><title>DZap USB</title>",
    )
    .unwrap();
    std::fs::write(frontend.join("_next/static/app.js"), "window.dzap = true;").unwrap();

    let state = server::AppState::in_memory(server::realtime::Hub::new());
    let app = server::build_router_with_state_and_frontend(state, frontend.clone());
    let base = spawn_app(app).await;

    let index = reqwest::get(format!("{base}/")).await.unwrap();
    assert_eq!(index.status(), 200);
    assert!(index.text().await.unwrap().contains("DZap USB"));

    let asset = reqwest::get(format!("{base}/_next/static/app.js"))
        .await
        .unwrap();
    assert_eq!(asset.status(), 200);
    assert_eq!(asset.text().await.unwrap(), "window.dzap = true;");

    std::fs::remove_dir_all(frontend).ok();
}
