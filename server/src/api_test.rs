use crate::api::{CertRequest, certificate_handler};
use crate::core::certificate;
use crate::core::drives::DeviceIdentity;
use crate::core::preflight::{PreflightDecision, WipePlan};
use crate::core::verification::{VerificationResult, VerificationStrategy};
use crate::{AppState, realtime::Hub};
use axum::Json;
use axum::body::to_bytes;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use std::collections::HashMap;

fn state_with_job(verified: bool) -> (AppState, String) {
    let state = AppState::in_memory(Hub::new());
    let job = state
        .jobs
        .create(&WipePlan {
            decision: PreflightDecision::Ready,
            device_path: "/dev/server-owned".to_string(),
            device_model: "Detected API Model".to_string(),
            device_type: "HDD".to_string(),
            method: "overwrite_1_pass".to_string(),
            identity: Some(DeviceIdentity {
                model: "Detected API Model".to_string(),
                serial: "DETECTED-API-SERIAL".to_string(),
                wwn: "0x5678".to_string(),
                size_bytes: "8192".to_string(),
                transport: "sata".to_string(),
                major_minor: "8:32".to_string(),
            }),
            checks: Vec::new(),
        })
        .unwrap();
    if verified {
        state.jobs.begin_verification(&job.id).unwrap();
        state
            .jobs
            .complete_verification(
                &job.id,
                VerificationResult {
                    strategy: VerificationStrategy::FullPatternReadback,
                    bytes_checked: 8192,
                    readback_sha256: "b".repeat(64),
                    expected_pattern: Some("0x00".to_string()),
                    firmware_status_sha256: None,
                    identity_revalidated: true,
                },
            )
            .unwrap();
    }
    (state, job.id)
}

#[tokio::test]
async fn certificate_handler_returns_verified_job_evidence() {
    certificate::init_for_tests();
    let (state, job_id) = state_with_job(true);
    let response = certificate_handler(
        State(state.clone()),
        Query(HashMap::new()),
        Ok(Json(CertRequest {
            job_id: job_id.clone(),
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["data"]["jobId"], job_id);
    assert_eq!(body["data"]["devicePath"], "/dev/server-owned");
    assert_eq!(body["data"]["deviceModel"], "Detected API Model");
    assert_eq!(body["data"]["deviceSerial"], "DETECTED-API-SERIAL");
    assert_eq!(body["data"]["wipeMethod"], "overwrite_1_pass");
    assert_eq!(
        body["data"]["verification"]["strategy"],
        "full_pattern_readback"
    );
    assert_eq!(body["data"]["verification"]["bytesChecked"], 8192);
    assert_eq!(body["data"]["evidenceHash"].as_str().unwrap().len(), 64);
    assert_eq!(body["signature"].as_str().unwrap().len(), 512);
    assert!(
        body["publicKey"]
            .as_str()
            .unwrap()
            .starts_with("-----BEGIN PUBLIC KEY-----")
    );

    let repeated = certificate_handler(
        State(state.clone()),
        Query(HashMap::new()),
        Ok(Json(CertRequest { job_id })),
    )
    .await;
    let repeated_bytes = to_bytes(repeated.into_body(), usize::MAX).await.unwrap();
    let repeated_body: serde_json::Value = serde_json::from_slice(&repeated_bytes).unwrap();
    assert_eq!(repeated_body["signature"], body["signature"]);
    assert_eq!(state.certificates.list().unwrap().len(), 1);
}

#[tokio::test]
async fn certificate_handler_rejects_unverified_job() {
    certificate::init_for_tests();
    let (state, job_id) = state_with_job(false);
    let response = certificate_handler(
        State(state),
        Query(HashMap::new()),
        Ok(Json(CertRequest { job_id })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(body["error"].as_str().unwrap().contains("verified"));
}

#[tokio::test]
async fn certificate_handler_returns_downloadable_pdf() {
    certificate::init_for_tests();
    let (state, job_id) = state_with_job(true);
    let response = certificate_handler(
        State(state),
        Query(HashMap::from([("format".to_string(), "pdf".to_string())])),
        Ok(Json(CertRequest { job_id })),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        "attachment; filename=certificate.pdf"
    );
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4\n"));
    assert!(bytes.windows(5).any(|window| window == b"%%EOF"));
}
