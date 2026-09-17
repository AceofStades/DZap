use chrono::{DateTime, SecondsFormat, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::drives::DeviceIdentity;
use super::recovery_plan::{RecoveryDestination, RecoveryImagePlan, RecoveryPlanDecision};

const JOB_RECORD_FILE: &str = "job.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryJobStatus {
    Imaging,
    Paused,
    ImageComplete,
    Extracting,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryMethod {
    FilesystemCopy,
    Photorec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryResult {
    pub method: RecoveryMethod,
    pub output_directory: String,
    pub manifest_path: String,
    pub manifest_sha256: String,
    pub recovered_file_count: u64,
    pub recovered_bytes: u64,
    pub skipped_entries: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestdiskAnalysis {
    pub completed_at: DateTime<Utc>,
    pub successful: bool,
    pub log_path: String,
    pub log_sha256: String,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RescueMapSummary {
    pub rescued_bytes: u64,
    pub unreadable_bytes: u64,
    pub pending_bytes: u64,
    pub total_bytes: u64,
}

impl RescueMapSummary {
    pub fn progress_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        ((self.rescued_bytes.saturating_add(self.unreadable_bytes)) as f64
            / self.total_bytes as f64
            * 100.0)
            .clamp(0.0, 100.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryEvent {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub message: String,
    pub previous_hash: Option<String>,
    pub event_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryJob {
    pub id: String,
    pub source_device_path: String,
    pub source_identity: DeviceIdentity,
    pub destination: RecoveryDestination,
    pub job_directory: String,
    pub image_path: String,
    pub map_path: String,
    pub log_path: String,
    pub status: RecoveryJobStatus,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub progress_percent: f64,
    pub map_summary: RescueMapSummary,
    pub last_message: String,
    pub failure: Option<String>,
    #[serde(default)]
    pub recovery_method: Option<RecoveryMethod>,
    #[serde(default)]
    pub recovery_output_directory: Option<String>,
    #[serde(default)]
    pub recovery_result: Option<RecoveryResult>,
    #[serde(default)]
    pub testdisk_analysis: Option<TestdiskAnalysis>,
    pub evidence_hash: String,
    pub events: Vec<RecoveryEvent>,
}

impl RecoveryJob {
    pub fn verify_evidence(&self) -> bool {
        if self.events.first().is_none_or(|event| {
            event.event_type != "recovery_authorized" || event.timestamp != self.started_at
        }) {
            return false;
        }
        let mut previous_hash = None;
        for (index, event) in self.events.iter().enumerate() {
            if event.sequence != index as u64 || event.previous_hash != previous_hash {
                return false;
            }
            if event.event_hash != hash_event(self, event) {
                return false;
            }
            previous_hash = Some(event.event_hash.clone());
        }
        if self.evidence_hash != previous_hash.unwrap_or_default() {
            return false;
        }
        let Some(last) = self.events.last() else {
            return false;
        };
        match (
            self.recovery_method,
            self.recovery_output_directory.as_deref(),
        ) {
            (None, None) => {
                if self.recovery_result.is_some() {
                    return false;
                }
            }
            (Some(method), Some(output_directory)) => {
                let expected = recovery_started_message(method, output_directory);
                if !self.events.iter().rev().any(|event| {
                    event.event_type == "extraction_started" && event.message == expected
                }) {
                    return false;
                }
            }
            _ => return false,
        }
        if let Some(analysis) = &self.testdisk_analysis {
            let expected = testdisk_analysis_message(analysis);
            if !self
                .events
                .iter()
                .any(|event| event.event_type == "testdisk_analyzed" && event.message == expected)
            {
                return false;
            }
        }
        match self.status {
            RecoveryJobStatus::Imaging => {
                matches!(
                    last.event_type.as_str(),
                    "recovery_authorized" | "imaging_resumed"
                ) && self.completed_at.is_none()
                    && self.failure.is_none()
            }
            RecoveryJobStatus::Paused => {
                matches!(
                    last.event_type.as_str(),
                    "imaging_paused" | "imaging_interrupted"
                ) && self.completed_at.is_none()
            }
            RecoveryJobStatus::ImageComplete => {
                matches!(
                    last.event_type.as_str(),
                    "image_completed" | "testdisk_analyzed"
                ) && self.completed_at.is_some()
                    && self.failure.is_none()
            }
            RecoveryJobStatus::Extracting => {
                last.event_type == "extraction_started"
                    && self.completed_at.is_none()
                    && self.recovery_method.is_some()
                    && self.recovery_output_directory.is_some()
            }
            RecoveryJobStatus::Completed => {
                last.event_type == "recovery_completed"
                    && self.completed_at.is_some()
                    && self.failure.is_none()
                    && self.recovery_result.as_ref().is_some_and(|result| {
                        Some(result.method) == self.recovery_method
                            && Some(result.output_directory.as_str())
                                == self.recovery_output_directory.as_deref()
                            && last.message == recovery_completed_message(result)
                    })
            }
            RecoveryJobStatus::Cancelled => {
                last.event_type == "recovery_cancelled" && self.completed_at.is_some()
            }
            RecoveryJobStatus::Failed => {
                last.event_type == "recovery_failed"
                    && self.completed_at.is_some()
                    && self.failure.is_some()
            }
        }
    }

    pub fn validate_artifact_binding(&self) -> Result<(), String> {
        let job_directory = Path::new(&self.job_directory);
        let destination_mount = self
            .destination
            .mount_path
            .as_deref()
            .ok_or_else(|| "recovery destination has no mount path".to_string())?;
        let expected_directory = Path::new(destination_mount)
            .join("DZap-Recovery")
            .join(&self.id);
        if job_directory != expected_directory {
            return Err("recovery job directory no longer matches its destination".to_string());
        }
        let expected_artifacts = [
            (&self.image_path, "source.img"),
            (&self.map_path, "source.map"),
            (&self.log_path, "ddrescue.log"),
        ];
        for (artifact, file_name) in expected_artifacts {
            if Path::new(artifact) != job_directory.join(file_name) {
                return Err(format!(
                    "recovery artifact {artifact} is outside its bound job directory"
                ));
            }
        }
        let metadata = std::fs::symlink_metadata(job_directory).map_err(|error| {
            format!(
                "failed to inspect recovery job directory {}: {error}",
                job_directory.display()
            )
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("recovery job directory is not a physical directory".to_string());
        }
        Ok(())
    }

    pub fn image_allocated_bytes(&self) -> Result<u64, String> {
        self.validate_artifact_binding()?;
        let metadata = std::fs::symlink_metadata(&self.image_path).map_err(|error| {
            format!(
                "failed to inspect recovery image {}: {error}",
                self.image_path
            )
        })?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("recovery image is not a regular file".to_string());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(metadata.blocks().saturating_mul(512))
        }
        #[cfg(not(unix))]
        {
            Ok(metadata.len())
        }
    }
}

#[derive(Clone)]
pub struct RecoveryJobStore {
    jobs: Arc<Mutex<HashMap<String, RecoveryJob>>>,
    directory: Option<Arc<PathBuf>>,
}

impl RecoveryJobStore {
    pub fn in_memory() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            directory: None,
        }
    }

    pub fn persistent(directory: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("failed to create recovery job directory: {error}"))?;
        set_directory_permissions(&directory);
        let mut jobs = HashMap::new();
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| format!("failed to read recovery job directory: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("failed to read recovery job entry: {error}"))?;
            let path = entry.path();
            if path.is_dir() || path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let encoded = std::fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            let job: RecoveryJob = serde_json::from_str(&encoded)
                .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
            if !valid_recovery_job_id(&job.id)
                || path.file_name().and_then(|value| value.to_str())
                    != Some(format!("{}.json", job.id).as_str())
            {
                return Err(format!(
                    "recovery job identifier does not match {}",
                    path.display()
                ));
            }
            if !job.verify_evidence() {
                return Err(format!(
                    "recovery evidence verification failed for {}",
                    path.display()
                ));
            }
            if jobs.insert(job.id.clone(), job).is_some() {
                return Err("duplicate persisted recovery job identifier".to_string());
            }
        }
        let store = Self {
            jobs: Arc::new(Mutex::new(jobs)),
            directory: Some(Arc::new(directory)),
        };
        store.pause_interrupted_jobs()?;
        Ok(store)
    }

    pub fn create(&self, plan: &RecoveryImagePlan) -> Result<RecoveryJob, String> {
        if plan.decision != RecoveryPlanDecision::Ready {
            return Err("cannot create a recovery job from a blocked image plan".to_string());
        }
        let source_identity = plan
            .source_identity
            .clone()
            .ok_or_else(|| "ready recovery plan has no source identity".to_string())?;
        let destination = plan
            .destination
            .clone()
            .ok_or_else(|| "ready recovery plan has no destination".to_string())?;
        let output_root = plan
            .output_directory
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| "ready recovery plan has no output directory".to_string())?;
        prepare_output_root(&output_root)?;

        let id = new_recovery_job_id();
        let job_directory = output_root.join(&id);
        std::fs::create_dir(&job_directory).map_err(|error| {
            format!(
                "failed to create recovery job directory {}: {error}",
                job_directory.display()
            )
        })?;
        set_directory_permissions(&job_directory);
        let image_path = job_directory.join("source.img");
        create_empty_image(&image_path)?;
        let map_path = job_directory.join("source.map");
        let log_path = job_directory.join("ddrescue.log");
        let now = Utc::now();
        let mut job = RecoveryJob {
            id: id.clone(),
            source_device_path: plan.source_device_path.clone(),
            source_identity,
            destination,
            job_directory: job_directory.display().to_string(),
            image_path: image_path.display().to_string(),
            map_path: map_path.display().to_string(),
            log_path: log_path.display().to_string(),
            status: RecoveryJobStatus::Imaging,
            started_at: now,
            updated_at: now,
            completed_at: None,
            progress_percent: 0.0,
            map_summary: RescueMapSummary {
                total_bytes: plan.image_size_bytes.parse().unwrap_or(0),
                pending_bytes: plan.image_size_bytes.parse().unwrap_or(0),
                ..RescueMapSummary::default()
            },
            last_message: "Recovery imaging authorized; ddrescue is starting.".to_string(),
            failure: None,
            recovery_method: None,
            recovery_output_directory: None,
            recovery_result: None,
            testdisk_analysis: None,
            evidence_hash: String::new(),
            events: Vec::new(),
        };
        let checks = serde_json::to_string(&plan.checks)
            .map_err(|error| format!("failed to encode recovery plan evidence: {error}"))?;
        append_event(
            &mut job,
            "recovery_authorized",
            format!("Recovery image plan approved with checks: {checks}"),
        );
        job.started_at = job.events[0].timestamp;
        self.persist(&job)?;
        self.jobs
            .lock()
            .map_err(|_| "recovery job store lock was poisoned".to_string())?
            .insert(id, job.clone());
        Ok(job)
    }

    pub fn get(&self, id: &str) -> Result<Option<RecoveryJob>, String> {
        Ok(self
            .jobs
            .lock()
            .map_err(|_| "recovery job store lock was poisoned".to_string())?
            .get(id)
            .cloned())
    }

    pub fn list(&self) -> Result<Vec<RecoveryJob>, String> {
        let mut jobs: Vec<_> = self
            .jobs
            .lock()
            .map_err(|_| "recovery job store lock was poisoned".to_string())?
            .values()
            .cloned()
            .collect();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.started_at));
        Ok(jobs)
    }

    pub fn update_progress(
        &self,
        id: &str,
        summary: RescueMapSummary,
        message: impl Into<String>,
    ) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            if job.status != RecoveryJobStatus::Imaging {
                return Err(format!("recovery job {id} is not imaging"));
            }
            job.progress_percent = summary.progress_percent();
            job.map_summary = summary;
            job.last_message = message.into();
            job.updated_at = Utc::now();
            Ok(())
        })
    }

    pub fn resume(&self, id: &str) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            let image_was_completed = job
                .events
                .iter()
                .any(|event| event.event_type == "image_completed");
            let can_resume = job.status == RecoveryJobStatus::Paused
                || (job.status == RecoveryJobStatus::Failed && !image_was_completed);
            if !can_resume {
                return Err(format!("recovery job {id} cannot be resumed"));
            }
            job.status = RecoveryJobStatus::Imaging;
            job.completed_at = None;
            job.failure = None;
            job.last_message = "Resuming ddrescue from the existing map file.".to_string();
            append_event(
                job,
                "imaging_resumed",
                "The source and destination were revalidated; imaging resumed from the map file."
                    .to_string(),
            );
            Ok(())
        })
    }

    pub fn pause(&self, id: &str, message: impl Into<String>) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            if job.status != RecoveryJobStatus::Imaging {
                return Err(format!("recovery job {id} is not imaging"));
            }
            let message = message.into();
            job.status = RecoveryJobStatus::Paused;
            job.last_message = message.clone();
            job.failure = Some(message.clone());
            append_event(job, "imaging_paused", message);
            Ok(())
        })
    }

    pub fn complete_image(
        &self,
        id: &str,
        summary: RescueMapSummary,
    ) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            if job.status != RecoveryJobStatus::Imaging {
                return Err(format!("recovery job {id} is not imaging"));
            }
            job.status = RecoveryJobStatus::ImageComplete;
            job.progress_percent = summary.progress_percent();
            job.map_summary = summary.clone();
            job.failure = None;
            job.last_message = if summary.unreadable_bytes == 0 {
                "Source image completed without unreadable ranges.".to_string()
            } else {
                format!(
                    "Source image completed with {} unreadable bytes recorded in the map file.",
                    summary.unreadable_bytes
                )
            };
            job.completed_at = Some(Utc::now());
            append_event(job, "image_completed", job.last_message.clone());
            Ok(())
        })
    }

    pub fn cancel(&self, id: &str, message: impl Into<String>) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            if !matches!(
                job.status,
                RecoveryJobStatus::Imaging | RecoveryJobStatus::Extracting
            ) {
                return Err(format!("recovery job {id} is not active"));
            }
            let message = message.into();
            job.status = RecoveryJobStatus::Cancelled;
            job.last_message = message.clone();
            job.failure = Some(message.clone());
            job.completed_at = Some(Utc::now());
            append_event(job, "recovery_cancelled", message);
            Ok(())
        })
    }

    pub fn fail(&self, id: &str, message: impl Into<String>) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            let message = message.into();
            job.status = RecoveryJobStatus::Failed;
            job.last_message = message.clone();
            job.failure = Some(message.clone());
            job.completed_at = Some(Utc::now());
            append_event(job, "recovery_failed", message);
            Ok(())
        })
    }

    pub fn begin_recovery(
        &self,
        id: &str,
        method: RecoveryMethod,
        output_directory: String,
    ) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            let image_was_completed = job
                .events
                .iter()
                .any(|event| event.event_type == "image_completed");
            if !image_was_completed
                || !matches!(
                    job.status,
                    RecoveryJobStatus::ImageComplete
                        | RecoveryJobStatus::Completed
                        | RecoveryJobStatus::Cancelled
                        | RecoveryJobStatus::Failed
                )
            {
                return Err(format!(
                    "recovery job {id} does not have a completed source image"
                ));
            }
            let attempt = job
                .events
                .iter()
                .filter(|event| event.event_type == "extraction_started")
                .count()
                + 1;
            let method_name = recovery_method_name(method);
            let expected_output = if method == RecoveryMethod::Photorec {
                Path::new(&job.job_directory).join(format!("{method_name}-files-{attempt}.1"))
            } else {
                Path::new(&job.job_directory).join(format!("{method_name}-files-{attempt}"))
            };
            if Path::new(&output_directory) != expected_output {
                return Err("recovery output is outside its bound job directory".to_string());
            }
            job.status = RecoveryJobStatus::Extracting;
            job.completed_at = None;
            job.failure = None;
            job.recovery_method = Some(method);
            job.recovery_output_directory = Some(output_directory.clone());
            job.recovery_result = None;
            job.last_message = format!("Starting {method:?} recovery from the source image.");
            append_event(
                job,
                "extraction_started",
                recovery_started_message(method, &output_directory),
            );
            Ok(())
        })
    }

    pub fn record_testdisk_analysis(
        &self,
        id: &str,
        analysis: TestdiskAnalysis,
    ) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            if job.status != RecoveryJobStatus::ImageComplete {
                return Err(format!(
                    "recovery job {id} is not ready for TestDisk analysis"
                ));
            }
            let job_directory = Path::new(&job.job_directory);
            let log_path = Path::new(&analysis.log_path);
            let attempt = job
                .events
                .iter()
                .filter(|event| event.event_type == "testdisk_analyzed")
                .count()
                + 1;
            if log_path != job_directory.join(format!("testdisk-{attempt}.log")) {
                return Err("TestDisk log is outside the recovery job directory".to_string());
            }
            job.testdisk_analysis = Some(analysis.clone());
            job.last_message = if analysis.successful {
                "TestDisk finished its read-only partition analysis.".to_string()
            } else {
                "TestDisk could not complete partition analysis; its diagnostics were retained."
                    .to_string()
            };
            append_event(
                job,
                "testdisk_analyzed",
                testdisk_analysis_message(&analysis),
            );
            Ok(())
        })
    }

    pub fn update_recovery_progress(
        &self,
        id: &str,
        recovered_file_count: u64,
        recovered_bytes: u64,
    ) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            if job.status != RecoveryJobStatus::Extracting {
                return Err(format!("recovery job {id} is not extracting files"));
            }
            job.last_message =
                format!("Recovered {recovered_file_count} files ({recovered_bytes} bytes) so far.");
            Ok(())
        })
    }

    pub fn complete_recovery(
        &self,
        id: &str,
        result: RecoveryResult,
    ) -> Result<RecoveryJob, String> {
        self.update(id, |job| {
            if job.status != RecoveryJobStatus::Extracting
                || job.recovery_method != Some(result.method)
                || job.recovery_output_directory.as_deref()
                    != Some(result.output_directory.as_str())
            {
                return Err(format!(
                    "recovery job {id} is not running this recovery method"
                ));
            }
            let job_directory = Path::new(&job.job_directory);
            let manifest_path = Path::new(&result.manifest_path);
            let attempt = job
                .events
                .iter()
                .filter(|event| event.event_type == "extraction_started")
                .count();
            let expected_manifest = job_directory.join(format!(
                "{}-{attempt}.manifest.jsonl",
                recovery_method_name(result.method)
            ));
            if manifest_path.parent() != Some(job_directory)
                || manifest_path != expected_manifest
                || result.manifest_sha256.len() != 64
                || !result
                    .manifest_sha256
                    .chars()
                    .all(|character| character.is_ascii_hexdigit())
            {
                return Err("recovery result has an invalid manifest binding".to_string());
            }
            job.status = RecoveryJobStatus::Completed;
            job.completed_at = Some(Utc::now());
            job.failure = None;
            job.last_message = recovery_completed_message(&result);
            job.recovery_result = Some(result);
            append_event(job, "recovery_completed", job.last_message.clone());
            Ok(())
        })
    }

    fn update<F>(&self, id: &str, update: F) -> Result<RecoveryJob, String>
    where
        F: FnOnce(&mut RecoveryJob) -> Result<(), String>,
    {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| "recovery job store lock was poisoned".to_string())?;
        let current = jobs
            .get(id)
            .ok_or_else(|| format!("recovery job {id} was not found"))?;
        let mut updated = current.clone();
        update(&mut updated)?;
        updated.updated_at = Utc::now();
        self.persist_internal(&updated)?;
        let external_result = self.persist_external(&updated);
        jobs.insert(id.to_string(), updated.clone());
        external_result?;
        Ok(updated)
    }

    fn pause_interrupted_jobs(&self) -> Result<(), String> {
        let ids: Vec<String> = self
            .jobs
            .lock()
            .map_err(|_| "recovery job store lock was poisoned".to_string())?
            .values()
            .filter(|job| {
                matches!(
                    job.status,
                    RecoveryJobStatus::Imaging | RecoveryJobStatus::Extracting
                )
            })
            .map(|job| job.id.clone())
            .collect();
        for id in ids {
            let mut jobs = self
                .jobs
                .lock()
                .map_err(|_| "recovery job store lock was poisoned".to_string())?;
            let mut job = jobs
                .get(&id)
                .cloned()
                .ok_or_else(|| format!("recovery job {id} was not found"))?;
            if job.status == RecoveryJobStatus::Imaging {
                job.status = RecoveryJobStatus::Paused;
                job.completed_at = None;
                job.failure = Some("backend restarted while recovery was active".to_string());
                job.last_message =
                    "Backend restarted; resume using the existing map file.".to_string();
                append_event(
                    &mut job,
                    "imaging_interrupted",
                    "Backend restarted; the resumable map file was preserved.".to_string(),
                );
            } else {
                job.status = RecoveryJobStatus::Failed;
                job.completed_at = Some(Utc::now());
                job.failure = Some("backend restarted during file recovery".to_string());
                job.last_message =
                    "Backend restarted during file recovery; the source image and partial output were preserved."
                        .to_string();
                let message = job.last_message.clone();
                append_event(&mut job, "recovery_failed", message);
            }
            self.persist_internal(&job)?;
            let _ = self.persist_external(&job);
            jobs.insert(id, job);
        }
        Ok(())
    }

    fn persist(&self, job: &RecoveryJob) -> Result<(), String> {
        self.persist_internal(job)?;
        self.persist_external(job)
    }

    fn persist_internal(&self, job: &RecoveryJob) -> Result<(), String> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        let encoded = serde_json::to_vec_pretty(job)
            .map_err(|error| format!("failed to encode recovery job: {error}"))?;
        atomic_write(directory, &format!("{}.json", job.id), &encoded)
    }

    fn persist_external(&self, job: &RecoveryJob) -> Result<(), String> {
        let directory = Path::new(&job.job_directory);
        let encoded = serde_json::to_vec_pretty(job)
            .map_err(|error| format!("failed to encode recovery job: {error}"))?;
        atomic_write(directory, JOB_RECORD_FILE, &encoded)
    }
}

fn recovery_completed_message(result: &RecoveryResult) -> String {
    format!(
        "{} recovery completed into {} with {} files ({} bytes), {} skipped entries, and manifest {} with SHA-256 {}.",
        recovery_method_name(result.method),
        result.output_directory,
        result.recovered_file_count,
        result.recovered_bytes,
        result.skipped_entries,
        result.manifest_path,
        result.manifest_sha256
    )
}

fn recovery_started_message(method: RecoveryMethod, output_directory: &str) -> String {
    format!(
        "Started {} recovery into {output_directory}.",
        recovery_method_name(method)
    )
}

fn recovery_method_name(method: RecoveryMethod) -> &'static str {
    match method {
        RecoveryMethod::FilesystemCopy => "filesystem_copy",
        RecoveryMethod::Photorec => "photorec",
    }
}

fn testdisk_analysis_message(analysis: &TestdiskAnalysis) -> String {
    format!(
        "TestDisk read-only analysis completed at {} with success={}, log={}, SHA-256={}.",
        analysis
            .completed_at
            .to_rfc3339_opts(SecondsFormat::Nanos, true),
        analysis.successful,
        analysis.log_path,
        analysis.log_sha256
    )
}

impl Default for RecoveryJobStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

fn prepare_output_root(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("recovery output directory must be absolute".to_string());
    }
    std::fs::create_dir_all(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!(
            "recovery output root {} is not a physical directory",
            path.display()
        ));
    }
    set_directory_permissions(path);
    Ok(())
}

fn create_empty_image(path: &Path) -> Result<(), String> {
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            format!(
                "failed to create recovery image {}: {error}",
                path.display()
            )
        })?;
    file.sync_all()
        .map_err(|error| format!("failed to sync recovery image {}: {error}", path.display()))
}

fn new_recovery_job_id() -> String {
    let mut random = [0_u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut random);
    format!("recovery-{}", hex::encode(random))
}

pub(crate) fn valid_recovery_job_id(id: &str) -> bool {
    id.len() == 41
        && id.starts_with("recovery-")
        && id[9..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn append_event(job: &mut RecoveryJob, event_type: &str, message: String) {
    let timestamp = Utc::now();
    let sequence = job.events.len() as u64;
    let previous_hash = job.events.last().map(|event| event.event_hash.clone());
    let mut event = RecoveryEvent {
        sequence,
        timestamp,
        event_type: event_type.to_string(),
        message,
        previous_hash,
        event_hash: String::new(),
    };
    event.event_hash = hash_event(job, &event);
    job.evidence_hash = event.event_hash.clone();
    job.updated_at = timestamp;
    job.events.push(event);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryEventHashPayload<'a> {
    job_id: &'a str,
    source_device_path: &'a str,
    source_identity: &'a DeviceIdentity,
    destination_drive_identity: &'a DeviceIdentity,
    destination_device_path: &'a str,
    job_directory: &'a str,
    image_path: &'a str,
    map_path: &'a str,
    log_path: &'a str,
    sequence: u64,
    timestamp: String,
    event_type: &'a str,
    message: &'a str,
    previous_hash: Option<&'a str>,
}

fn hash_event(job: &RecoveryJob, event: &RecoveryEvent) -> String {
    let payload = RecoveryEventHashPayload {
        job_id: &job.id,
        source_device_path: &job.source_device_path,
        source_identity: &job.source_identity,
        destination_drive_identity: &job.destination.drive_identity,
        destination_device_path: &job.destination.device_path,
        job_directory: &job.job_directory,
        image_path: &job.image_path,
        map_path: &job.map_path,
        log_path: &job.log_path,
        sequence: event.sequence,
        timestamp: event.timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true),
        event_type: &event.event_type,
        message: &event.message,
        previous_hash: event.previous_hash.as_deref(),
    };
    let encoded = serde_json::to_vec(&payload).expect("recovery evidence payload is serializable");
    hex::encode(Sha256::digest(encoded))
}

fn atomic_write(directory: &Path, file_name: &str, contents: &[u8]) -> Result<(), String> {
    let destination = directory.join(file_name);
    let temporary = directory.join(format!(".{file_name}.tmp"));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)
        .map_err(|error| format!("failed to open {}: {error}", temporary.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    set_file_permissions(&temporary);
    file.sync_all()
        .map_err(|error| format!("failed to sync {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, &destination).map_err(|error| {
        format!(
            "failed to replace recovery job record {}: {error}",
            destination.display()
        )
    })?;
    std::fs::File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("failed to sync {}: {error}", directory.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) {}

#[cfg(unix)]
fn set_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) {}
