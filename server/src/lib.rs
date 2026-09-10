// Library root: exposes the app so integration tests can drive the router.
pub mod api;
pub mod core;
pub mod realtime;

#[cfg(test)]
mod api_test;
#[cfg(test)]
mod realtime_test;

use axum::Router;
use axum::http::{HeaderValue, Method, header::CONTENT_TYPE};
use axum::routing::{get, post};
use core::certificate::CertificateStore;
use core::jobs::JobStore;
use std::path::PathBuf;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

const UI_ORIGINS: [&str; 6] = [
    "http://localhost:3000",
    "http://127.0.0.1:3000",
    "http://[::1]:3000",
    "http://localhost:8080",
    "http://127.0.0.1:8080",
    "http://[::1]:8080",
];

pub(crate) fn is_allowed_ui_origin(origin: &str) -> bool {
    UI_ORIGINS.contains(&origin)
}

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(UI_ORIGINS.map(HeaderValue::from_static))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE])
}

#[derive(Clone)]
pub struct AppState {
    pub hub: realtime::Hub,
    pub jobs: JobStore,
    pub certificates: CertificateStore,
}

impl AppState {
    pub fn in_memory(hub: realtime::Hub) -> Self {
        Self {
            hub,
            jobs: JobStore::in_memory(),
            certificates: CertificateStore::in_memory(),
        }
    }

    pub fn persistent(hub: realtime::Hub) -> Result<Self, String> {
        let config = dirs::config_dir()
            .ok_or_else(|| "could not get user config directory".to_string())?
            .join("DZap");
        let state = Self {
            hub,
            jobs: JobStore::persistent(config.join("jobs"))?,
            certificates: CertificateStore::persistent(config.join("certificates"))?,
        };
        for certificate in state.certificates.list()? {
            let job = state.jobs.get(&certificate.data.job_id)?.ok_or_else(|| {
                format!(
                    "certificate {} has no corresponding wipe job",
                    certificate.data.job_id
                )
            })?;
            if !certificate.matches_job(&job) {
                return Err(format!(
                    "certificate {} does not match its wipe evidence",
                    certificate.data.job_id
                ));
            }
        }
        Ok(state)
    }
}

pub fn build_router(hub: realtime::Hub) -> Router {
    build_router_with_state(AppState::in_memory(hub))
}

pub fn build_router_with_state(state: AppState) -> Router {
    let frontend_directory = std::env::var_os("DZAP_FRONTEND_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/opt/dzap/frontend"));
    build_router_with_state_and_frontend(state, frontend_directory)
}

pub fn build_router_with_state_and_frontend(
    state: AppState,
    frontend_directory: impl Into<PathBuf>,
) -> Router {
    Router::new()
        .route("/api/drives", get(api::get_drives_handler))
        .route("/api/wipe/preflight", post(api::preflight_wipe_handler))
        .route("/api/wipe", post(api::wipe_drive_handler))
        .route("/api/wipe/jobs", get(api::list_wipe_jobs_handler))
        .route("/api/wipe/jobs/{id}", get(api::get_wipe_job_handler))
        .route("/api/wipe/pause", post(api::pause_wipe_handler))
        .route("/api/wipe/abort", post(api::abort_wipe_handler))
        .route("/api/certificates", get(api::list_certificates_handler))
        .route("/api/certificate/generate", post(api::certificate_handler))
        .route("/api/certificate", post(api::certificate_handler))
        .route("/api/unmount", post(api::unmount_drive_handler))
        .route(
            "/api/drive/{name}/health",
            get(api::get_drive_health_handler),
        )
        .route(
            "/api/drive/{name}/wipe-methods",
            get(api::get_wipe_methods_handler),
        )
        .route("/ws", get(api::ws_handler))
        .fallback_service(ServeDir::new(frontend_directory.into()))
        .layer(cors_layer())
        .with_state(state)
}
