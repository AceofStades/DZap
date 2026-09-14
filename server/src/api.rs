// Port of server-go/api/handlers.go
use axum::Json;
use axum::extract::{Path, State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header::ORIGIN};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;
use crate::core::{
    certificate::{self, SignedCertificate},
    drives, evidence_export,
    jobs::{WipeJob, WipeJobStatus},
    predict, preflight, recovery, recovery_plan, verification, wiper,
};

/// Helper to ensure all error responses are in a consistent JSON format.
fn error_response(code: StatusCode, message: &str) -> Response {
    (code, Json(json!({ "error": message }))).into_response()
}

pub async fn get_drives_handler() -> Response {
    match tokio::task::spawn_blocking(drives::detect_devices).await {
        Ok(devices) => Json(devices).into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to detect drives: {e}"),
        ),
    }
}

pub async fn get_drive_health_handler(Path(drive_name): Path<String>) -> Response {
    let device_path = format!("/dev/{drive_name}");

    match tokio::task::spawn_blocking(move || predict::predict_drive_health(&device_path)).await {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err(e)) => {
            drives::log_line(&format!("ERROR in get_drive_health_handler: {e}"));
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to predict health: {e}"),
            )
        }
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to predict health: {e}"),
        ),
    }
}

pub async fn wipe_drive_handler(
    State(state): State<AppState>,
    body: Result<Json<wiper::WipeConfig>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(config) = match body {
        Ok(c) => c,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {e}"),
            );
        }
    };

    let preflight_config = config.clone();
    let plan =
        match tokio::task::spawn_blocking(move || preflight::authorize_wipe(&preflight_config))
            .await
        {
            Ok(Ok(plan)) => plan,
            Ok(Err(e)) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Failed to run wipe preflight: {e}"),
                );
            }
            Err(e) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Failed to run wipe preflight: {e}"),
                );
            }
        };

    if !plan.is_ready() {
        return (StatusCode::PRECONDITION_FAILED, Json(plan)).into_response();
    }

    let reservation = match wiper::reserve_device(&plan.device_path) {
        Ok(reservation) => reservation,
        Err(error) => return error_response(StatusCode::CONFLICT, &error),
    };
    let job = match state.jobs.create(&plan) {
        Ok(job) => job,
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to record wipe authorization: {error}"),
            );
        }
    };
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let device_path = config.device_path.clone();
    let verification_config = config.clone();
    let job_id = job.id.clone();
    let task_job_id = job_id.clone();
    tokio::spawn(async move {
        let _reservation = reservation;
        let wipe_task = tokio::task::spawn_blocking(move || {
            let result = wiper::sanitize_device(config, &progress_tx);
            drop(progress_tx);
            result
        });

        while let Some(message) = progress_rx.recv().await {
            state
                .hub
                .broadcast(progress_message(&task_job_id, &message));
        }

        let result = match wipe_task.await {
            Ok(result) => result,
            Err(error) => Err(format!("wipe worker failed: {error}")),
        };
        match result {
            Err(error) => {
                drives::log_line(&format!(
                    "ERROR in wipe_drive_handler (sanitization): {error}"
                ));
                let evidence_error = state.jobs.fail(&task_job_id, &error).err();
                state.hub.broadcast(
                    json!({
                        "status": "failed",
                        "jobId": task_job_id,
                        "deviceId": device_path,
                        "error": evidence_error
                            .map(|record_error| format!("{error}; evidence error: {record_error}"))
                            .unwrap_or(error),
                    })
                    .to_string(),
                );
            }
            Ok(()) => {
                if let Err(error) = state.jobs.begin_verification(&task_job_id) {
                    state.hub.broadcast(
                        json!({
                            "status": "failed",
                            "jobId": task_job_id,
                            "deviceId": device_path,
                            "error": format!(
                                "Wipe command completed, but verification evidence could not be started: {error}"
                            ),
                        })
                        .to_string(),
                    );
                    return;
                }
                state.hub.broadcast(
                    json!({
                        "status": "verifying",
                        "jobId": task_job_id,
                        "deviceId": device_path,
                    })
                    .to_string(),
                );

                let verification = tokio::task::spawn_blocking(move || {
                    verification::verify_wipe(&verification_config)
                })
                .await
                .unwrap_or_else(|error| Err(format!("verification worker failed: {error}")));
                match verification {
                    Ok(result) => match state
                        .jobs
                        .complete_verification(&task_job_id, result.clone())
                    {
                        Ok(completed) => state.hub.broadcast(
                            json!({
                                "status": "verified",
                                "jobId": task_job_id,
                                "deviceId": device_path,
                                "evidenceHash": completed.evidence_hash,
                                "verification": result,
                            })
                            .to_string(),
                        ),
                        Err(error) => state.hub.broadcast(
                            json!({
                                "status": "failed",
                                "jobId": task_job_id,
                                "deviceId": device_path,
                                "error": format!(
                                    "Wipe was verified, but final evidence could not be recorded: {error}"
                                ),
                            })
                            .to_string(),
                        ),
                    },
                    Err(error) => {
                        let evidence_error = state.jobs.fail(&task_job_id, &error).err();
                        state.hub.broadcast(
                            json!({
                                "status": "failed",
                                "jobId": task_job_id,
                                "deviceId": device_path,
                                "error": evidence_error
                                    .map(|record_error| {
                                        format!("{error}; evidence error: {record_error}")
                                    })
                                    .unwrap_or(error),
                            })
                            .to_string(),
                        );
                    }
                }
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(json!({
            "status": "Wipe process started",
            "jobId": job_id,
            "deviceId": job.device_path,
        })),
    )
        .into_response()
}

fn progress_message(job_id: &str, message: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(message) {
        Ok(serde_json::Value::Object(mut object)) => {
            object.insert("jobId".to_string(), json!(job_id));
            serde_json::Value::Object(object).to_string()
        }
        _ => json!({ "jobId": job_id, "message": message }).to_string(),
    }
}

pub async fn preflight_wipe_handler(
    body: Result<Json<wiper::WipeConfig>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(config) = match body {
        Ok(c) => c,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {e}"),
            );
        }
    };

    match tokio::task::spawn_blocking(move || preflight::preflight_wipe(&config)).await {
        Ok(Ok(plan)) => Json(plan).into_response(),
        Ok(Err(e)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to run wipe preflight: {e}"),
        ),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to run wipe preflight: {e}"),
        ),
    }
}

pub async fn assess_recovery_handler(
    body: Result<
        Json<recovery::RecoveryAssessmentRequest>,
        axum::extract::rejection::JsonRejection,
    >,
) -> Response {
    let Json(request) = match body {
        Ok(request) => request,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {error}"),
            );
        }
    };

    match tokio::task::spawn_blocking(move || recovery::assess_recovery(&request)).await {
        Ok(Ok(assessment)) => Json(assessment).into_response(),
        Ok(Err(error)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to assess recovery source: {error}"),
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to assess recovery source: {error}"),
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryDestinationsQuery {
    pub source_device_path: String,
}

pub async fn list_recovery_destinations_handler(
    query: Result<
        axum::extract::Query<RecoveryDestinationsQuery>,
        axum::extract::rejection::QueryRejection,
    >,
) -> Response {
    let axum::extract::Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid query parameters: {error}"),
            );
        }
    };
    match tokio::task::spawn_blocking(move || {
        recovery_plan::detect_recovery_destinations(&query.source_device_path)
    })
    .await
    {
        Ok(Ok(destinations)) => Json(destinations).into_response(),
        Ok(Err(error)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to detect recovery destinations: {error}"),
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to detect recovery destinations: {error}"),
        ),
    }
}

pub async fn mount_recovery_destination_handler(
    body: Result<
        Json<recovery_plan::RecoveryMountRequest>,
        axum::extract::rejection::JsonRejection,
    >,
) -> Response {
    let Json(request) = match body {
        Ok(request) => request,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {error}"),
            );
        }
    };
    match tokio::task::spawn_blocking(move || recovery_plan::mount_recovery_destination(&request))
        .await
    {
        Ok(Ok(destination)) => Json(destination).into_response(),
        Ok(Err(error)) => error_response(
            StatusCode::CONFLICT,
            &format!("Failed to mount recovery destination: {error}"),
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to mount recovery destination: {error}"),
        ),
    }
}

pub async fn plan_recovery_image_handler(
    body: Result<Json<recovery_plan::RecoveryPlanRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(request) => request,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {error}"),
            );
        }
    };
    match tokio::task::spawn_blocking(move || recovery_plan::plan_recovery_image(&request)).await {
        Ok(Ok(plan)) => Json(plan).into_response(),
        Ok(Err(error)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to plan recovery image: {error}"),
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to plan recovery image: {error}"),
        ),
    }
}

pub async fn list_wipe_jobs_handler(State(state): State<AppState>) -> Response {
    match state.jobs.list() {
        Ok(jobs) => Json(jobs).into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to list wipe jobs: {error}"),
        ),
    }
}

pub async fn get_wipe_job_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.jobs.get(&id) {
        Ok(Some(job)) => Json(job).into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "Wipe job not found"),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to load wipe job: {error}"),
        ),
    }
}

pub async fn get_wipe_methods_handler(Path(identifier): Path<String>) -> Response {
    // Assume it could be a storage device first
    let device_path = format!("/dev/{identifier}");
    let methods = tokio::task::spawn_blocking(move || {
        wiper::get_wipe_methods(&device_path)
            // If that fails, assume it might be a mobile device serial
            .or_else(|_| wiper::get_wipe_methods(&identifier))
    })
    .await;

    match methods {
        Ok(Ok(methods)) => Json(methods).into_response(),
        Ok(Err(e)) => {
            drives::log_line(&format!("ERROR in get_wipe_methods_handler: {e}"));
            error_response(
                StatusCode::NOT_FOUND,
                &format!("Device not found or methods unavailable: {e}"),
            )
        }
        Err(e) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Device not found or methods unavailable: {e}"),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct DeviceIdRequest {
    #[serde(rename = "deviceId", alias = "DeviceId", alias = "deviceID")]
    device_id: String,
}

pub async fn pause_wipe_handler(
    State(state): State<AppState>,
    body: Result<Json<DeviceIdRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(r) => r,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {e}"),
            );
        }
    };

    let device_id = resolve_device_id(&state, &req.device_id);
    match wiper::pause_wipe(&device_id) {
        Ok(()) => Json(json!({ "message": "Wipe pause request received" })).into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to pause wipe: {e}"),
        ),
    }
}

pub async fn abort_wipe_handler(
    State(state): State<AppState>,
    body: Result<Json<DeviceIdRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(r) => r,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {e}"),
            );
        }
    };

    let device_id = resolve_device_id(&state, &req.device_id);
    match wiper::abort_wipe(&device_id) {
        Ok(()) => Json(json!({ "message": "Wipe abort request received" })).into_response(),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to abort wipe: {e}"),
        ),
    }
}

fn resolve_device_id(state: &AppState, job_or_device_id: &str) -> String {
    match state.jobs.get(job_or_device_id) {
        Ok(Some(job)) => job.device_path,
        _ => job_or_device_id.to_string(),
    }
}

pub async fn list_certificates_handler(State(state): State<AppState>) -> Response {
    match state.certificates.list() {
        Ok(certificates) => Json(certificates).into_response(),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to list certificates: {error}"),
        ),
    }
}

pub async fn list_export_destinations_handler() -> Response {
    match tokio::task::spawn_blocking(evidence_export::detect_export_destinations).await {
        Ok(Ok(destinations)) => Json(destinations).into_response(),
        Ok(Err(error)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to detect evidence export destinations: {error}"),
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to detect evidence export destinations: {error}"),
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct UnmountRequest {
    #[serde(alias = "Device")]
    device: String,
}

pub async fn unmount_drive_handler(
    body: Result<Json<UnmountRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(r) => r,
        Err(e) => {
            drives::log_line(&format!("Error decoding unmount request JSON: {e}"));
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {e}"),
            );
        }
    };

    let device = req.device.clone();
    match tokio::task::spawn_blocking(move || drives::unmount_device(&device)).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            drives::log_line(&format!("Error unmounting device {}: {e}", req.device));
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to unmount device: {e}"),
            );
        }
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to unmount device: {e}"),
            );
        }
    }

    drives::log_line(&format!(
        "Successfully processed unmount for device {}",
        req.device
    ));
    Json(json!({ "status": "Device unmounted successfully" })).into_response()
}

#[derive(Debug, Deserialize)]
pub struct CertRequest {
    #[serde(rename = "jobId", alias = "job_id")]
    pub job_id: String,
}

#[derive(Debug, Deserialize)]
pub struct EvidenceExportRequest {
    #[serde(rename = "jobId", alias = "job_id")]
    pub job_id: String,
    pub destination: evidence_export::ExportDestination,
}

#[derive(Debug, Deserialize)]
pub struct EvidenceMountRequest {
    pub destination: evidence_export::ExportDestination,
}

fn load_or_issue_certificate(
    state: &AppState,
    job: &WipeJob,
) -> Result<SignedCertificate, (StatusCode, String)> {
    match state.certificates.get(&job.id) {
        Ok(Some(certificate)) => {
            if !certificate.verify_signature() || !certificate.matches_job(job) {
                return Err((
                    StatusCode::CONFLICT,
                    "Stored certificate does not match the wipe evidence".to_string(),
                ));
            }
            Ok(certificate)
        }
        Ok(None) => {
            let generated = certificate::generate_certificate_for_job(job).map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to generate certificate: {error}"),
                )
            })?;
            state
                .certificates
                .save_if_absent(generated)
                .map_err(|error| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to persist certificate: {error}"),
                    )
                })
        }
        Err(error) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to load certificate: {error}"),
        )),
    }
}

pub async fn export_evidence_handler(
    State(state): State<AppState>,
    body: Result<Json<EvidenceExportRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(request) => request,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {error}"),
            );
        }
    };

    let job = match state.jobs.get(&request.job_id) {
        Ok(Some(job)) => job,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "Wipe job not found"),
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to load wipe job: {error}"),
            );
        }
    };
    if job.status != WipeJobStatus::Verified || !job.verify_evidence() {
        return error_response(
            StatusCode::CONFLICT,
            "Evidence export requires a successfully verified wipe job",
        );
    }

    let certificate = match load_or_issue_certificate(&state, &job) {
        Ok(certificate) => certificate,
        Err((status, message)) => return error_response(status, &message),
    };
    let destination = request.destination;
    match tokio::task::spawn_blocking(move || {
        evidence_export::export_evidence(&job, &certificate, &destination)
    })
    .await
    {
        Ok(Ok(result)) => Json(result).into_response(),
        Ok(Err(error)) => error_response(
            StatusCode::CONFLICT,
            &format!("Failed to export evidence: {error}"),
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to export evidence: {error}"),
        ),
    }
}

pub async fn mount_export_destination_handler(
    body: Result<Json<EvidenceMountRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(request) = match body {
        Ok(request) => request,
        Err(error) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {error}"),
            );
        }
    };

    match tokio::task::spawn_blocking(move || {
        evidence_export::mount_export_destination(&request.destination)
    })
    .await
    {
        Ok(Ok(destination)) => Json(destination).into_response(),
        Ok(Err(error)) => error_response(
            StatusCode::CONFLICT,
            &format!("Failed to mount evidence destination: {error}"),
        ),
        Err(error) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("Failed to mount evidence destination: {error}"),
        ),
    }
}

/// Issues a certificate from server-owned evidence. Supports `?format=pdf`.
pub async fn certificate_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    body: Result<Json<CertRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(req) = match body {
        Ok(r) => r,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid request body: {e}"),
            );
        }
    };

    let job = match state.jobs.get(&req.job_id) {
        Ok(Some(job)) => job,
        Ok(None) => return error_response(StatusCode::NOT_FOUND, "Wipe job not found"),
        Err(error) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to load wipe job: {error}"),
            );
        }
    };
    if job.status != WipeJobStatus::Verified {
        return error_response(
            StatusCode::CONFLICT,
            "Certificate requires a successfully verified wipe job",
        );
    }
    if !job.verify_evidence() {
        return error_response(
            StatusCode::CONFLICT,
            "Wipe job evidence verification failed",
        );
    }

    let signed_cert = match load_or_issue_certificate(&state, &job) {
        Ok(certificate) => certificate,
        Err((status, message)) => return error_response(status, &message),
    };

    // Check if user requested PDF format
    if params.get("format").is_some_and(|f| f == "pdf") {
        match signed_cert.generate_pdf() {
            Ok(pdf_bytes) => (
                StatusCode::OK,
                [
                    ("Content-Type", "application/pdf"),
                    (
                        "Content-Disposition",
                        "attachment; filename=certificate.pdf",
                    ),
                ],
                pdf_bytes,
            )
                .into_response(),
            Err(e) => error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to generate PDF: {e}"),
            ),
        }
    } else {
        // Default to JSON
        Json(signed_cert).into_response()
    }
}

pub async fn ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Response {
    if headers.get(ORIGIN).is_some_and(|origin| {
        origin
            .to_str()
            .map_or(true, |origin| !crate::is_allowed_ui_origin(origin))
    }) {
        return error_response(StatusCode::FORBIDDEN, "WebSocket origin is not allowed");
    }

    ws.on_upgrade(move |mut socket| async move {
        let mut rx = state.hub.sender.subscribe();
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if socket
                        .send(axum::extract::ws::Message::Text(msg.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
    .into_response()
}
