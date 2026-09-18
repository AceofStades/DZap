pub mod engine;
pub mod parsers;
pub mod report;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CarveStatus {
    Idle,
    Scanning,
    Paused,
    Completed,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarveTarget {
    pub device_path: String,
    pub output_dir: String,
    pub file_types: Vec<String>,
    pub cluster_size: Option<u64>,
    pub scan_mode: Option<String>, // "fast_signature" | "deep_bifragment"
    pub max_file_size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarvedArtifact {
    pub id: String,
    pub filename: String,
    pub file_type: String,
    pub extension: String,
    pub mime_type: String,
    pub start_sector: u64,
    pub end_sector: u64,
    pub byte_offset: u64,
    pub size_bytes: u64,
    pub sha256: String,
    pub confidence_score: f64,
    pub is_fragmented: bool,
    pub fragment_count: usize,
    pub gap_offset: Option<u64>,
    pub metadata: HashMap<String, String>,
    pub extracted_path: String,
    pub preview_data_url: Option<String>,
    pub carved_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarveProgress {
    pub task_id: String,
    pub device_path: String,
    pub status: CarveStatus,
    pub scanned_bytes: u64,
    pub total_bytes: u64,
    pub current_sector: u64,
    pub total_sectors: u64,
    pub speed_mbps: f64,
    pub elapsed_secs: u64,
    pub artifacts_found: usize,
    pub valid_count: usize,
    pub fragmented_count: usize,
    pub bad_sectors: usize,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct CarverStore {
    pub active_task_id: Arc<Mutex<Option<String>>>,
    pub stop_signal: Arc<AtomicBool>,
    pub artifacts: Arc<Mutex<Vec<CarvedArtifact>>>,
    pub latest_progress: Arc<Mutex<Option<CarveProgress>>>,
}

impl CarverStore {
    pub fn new() -> Self {
        Self {
            active_task_id: Arc::new(Mutex::new(None)),
            stop_signal: Arc::new(AtomicBool::new(false)),
            artifacts: Arc::new(Mutex::new(Vec::new())),
            latest_progress: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start_task(&self, task_id: String, device_path: String, total_bytes: u64, sector_size: u64) {
        self.stop_signal.store(false, Ordering::SeqCst);
        let mut active = self.active_task_id.lock().unwrap();
        *active = Some(task_id.clone());

        let total_sectors = if sector_size > 0 { total_bytes / sector_size } else { 0 };
        let mut prog = self.latest_progress.lock().unwrap();
        *prog = Some(CarveProgress {
            task_id,
            device_path,
            status: CarveStatus::Scanning,
            scanned_bytes: 0,
            total_bytes,
            current_sector: 0,
            total_sectors,
            speed_mbps: 0.0,
            elapsed_secs: 0,
            artifacts_found: 0,
            valid_count: 0,
            fragmented_count: 0,
            bad_sectors: 0,
            error: None,
        });
    }

    pub fn stop_task(&self) {
        self.stop_signal.store(true, Ordering::SeqCst);
        let mut prog = self.latest_progress.lock().unwrap();
        if let Some(p) = prog.as_mut() {
            if p.status == CarveStatus::Scanning {
                p.status = CarveStatus::Stopped;
            }
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.stop_signal.load(Ordering::SeqCst)
    }

    pub fn add_artifact(&self, artifact: CarvedArtifact) {
        let mut artifacts = self.artifacts.lock().unwrap();
        artifacts.push(artifact);
    }

    pub fn get_artifacts(&self) -> Vec<CarvedArtifact> {
        let artifacts = self.artifacts.lock().unwrap();
        artifacts.clone()
    }

    pub fn get_artifact_by_id(&self, id: &str) -> Option<CarvedArtifact> {
        let artifacts = self.artifacts.lock().unwrap();
        artifacts.iter().find(|a| a.id == id).cloned()
    }

    pub fn update_progress(&self, update_fn: impl FnOnce(&mut CarveProgress)) {
        let mut prog = self.latest_progress.lock().unwrap();
        if let Some(p) = prog.as_mut() {
            update_fn(p);
        }
    }

    pub fn get_progress(&self) -> Option<CarveProgress> {
        let prog = self.latest_progress.lock().unwrap();
        prog.clone()
    }

    pub fn clear_artifacts(&self) {
        let mut artifacts = self.artifacts.lock().unwrap();
        artifacts.clear();
    }
}

impl Default for CarverStore {
    fn default() -> Self {
        Self::new()
    }
}
