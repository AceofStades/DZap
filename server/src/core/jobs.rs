use chrono::{DateTime, SecondsFormat, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::drives::DeviceIdentity;
use super::preflight::WipePlan;
use super::verification::VerificationResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WipeJobStatus {
    Running,
    Verifying,
    Verified,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceEvent {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub message: String,
    pub previous_hash: Option<String>,
    pub event_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WipeJob {
    pub id: String,
    pub device_path: String,
    pub device_model: String,
    pub device_type: String,
    pub identity: DeviceIdentity,
    pub method: String,
    pub status: WipeJobStatus,
    pub started_at: DateTime<Utc>,
    pub sanitization_completed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub failure: Option<String>,
    pub verification: Option<VerificationResult>,
    pub evidence_hash: String,
    pub events: Vec<EvidenceEvent>,
}

impl WipeJob {
    pub fn verify_evidence(&self) -> bool {
        if self.events.first().is_none_or(|event| {
            event.event_type != "wipe_authorized" || event.timestamp != self.started_at
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
        match self.status {
            WipeJobStatus::Running => {
                self.events.len() == 1
                    && self.sanitization_completed_at.is_none()
                    && self.completed_at.is_none()
                    && self.failure.is_none()
                    && self.verification.is_none()
            }
            WipeJobStatus::Verifying => {
                self.events.len() == 2
                    && last.event_type == "sanitization_completed"
                    && self.sanitization_completed_at == Some(last.timestamp)
                    && self.completed_at.is_none()
                    && self.failure.is_none()
                    && self.verification.is_none()
            }
            WipeJobStatus::Verified => self.verification.as_ref().is_some_and(|verification| {
                self.events.len() == 3
                    && self.events[1].event_type == "sanitization_completed"
                    && self.sanitization_completed_at == Some(self.events[1].timestamp)
                    && last.event_type == "verification_completed"
                    && self.completed_at == Some(last.timestamp)
                    && self.failure.is_none()
                    && serde_json::to_string(verification).is_ok_and(|encoded| {
                        last.message == format!("Wipe verification passed: {encoded}")
                    })
            }),
            WipeJobStatus::Failed => {
                let expected_events = if self.sanitization_completed_at.is_some() {
                    3
                } else {
                    2
                };
                self.failure.as_ref().is_some_and(|failure| {
                    self.events.len() == expected_events
                        && self.sanitization_completed_at.is_none_or(|timestamp| {
                            self.events[1].event_type == "sanitization_completed"
                                && self.events[1].timestamp == timestamp
                        })
                        && last.event_type == "wipe_failed"
                        && self.completed_at == Some(last.timestamp)
                        && self.verification.is_none()
                        && last.message == format!("The wipe operation failed: {failure}")
                })
            }
        }
    }
}

#[derive(Clone)]
pub struct JobStore {
    jobs: Arc<Mutex<HashMap<String, WipeJob>>>,
    directory: Option<Arc<PathBuf>>,
}

impl JobStore {
    pub fn in_memory() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            directory: None,
        }
    }

    pub fn persistent(directory: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("failed to create wipe job directory: {error}"))?;
        set_directory_permissions(&directory);

        let mut jobs = HashMap::new();
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| format!("failed to read wipe job directory: {error}"))?
        {
            let entry = entry.map_err(|error| format!("failed to read wipe job entry: {error}"))?;
            let path = entry.path();
            if path.is_dir() || path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }

            let encoded = std::fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            let job: WipeJob = serde_json::from_str(&encoded)
                .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
            if !valid_job_id(&job.id)
                || path.file_name().and_then(|value| value.to_str())
                    != Some(format!("{}.json", job.id).as_str())
            {
                return Err(format!(
                    "wipe job identifier does not match {}",
                    path.display()
                ));
            }
            if !job.verify_evidence() {
                return Err(format!(
                    "wipe evidence verification failed for {}",
                    path.display()
                ));
            }
            if jobs.insert(job.id.clone(), job).is_some() {
                return Err("duplicate persisted wipe job identifier".to_string());
            }
        }

        let store = Self {
            jobs: Arc::new(Mutex::new(jobs)),
            directory: Some(Arc::new(directory)),
        };
        store.fail_interrupted_jobs()?;
        Ok(store)
    }

    pub fn create(&self, plan: &WipePlan) -> Result<WipeJob, String> {
        if !plan.is_ready() {
            return Err("cannot create a wipe job from a blocked preflight plan".to_string());
        }
        let identity = plan
            .identity
            .clone()
            .ok_or_else(|| "approved preflight plan has no device identity".to_string())?;
        let id = new_job_id();
        let now = Utc::now();
        let mut job = WipeJob {
            id: id.clone(),
            device_path: plan.device_path.clone(),
            device_model: plan.device_model.clone(),
            device_type: plan.device_type.clone(),
            identity,
            method: plan.method.clone(),
            status: WipeJobStatus::Running,
            started_at: now,
            sanitization_completed_at: None,
            completed_at: None,
            failure: None,
            verification: None,
            evidence_hash: String::new(),
            events: Vec::new(),
        };
        let checks = serde_json::to_string(&plan.checks)
            .map_err(|error| format!("failed to encode preflight evidence: {error}"))?;
        append_event_at(
            &mut job,
            now,
            "wipe_authorized",
            format!("Safety preflight approved with checks: {checks}"),
        );

        self.persist(&job)?;
        self.jobs
            .lock()
            .map_err(|_| "wipe job store lock was poisoned".to_string())?
            .insert(id, job.clone());
        Ok(job)
    }

    pub fn get(&self, id: &str) -> Result<Option<WipeJob>, String> {
        Ok(self
            .jobs
            .lock()
            .map_err(|_| "wipe job store lock was poisoned".to_string())?
            .get(id)
            .cloned())
    }

    pub fn list(&self) -> Result<Vec<WipeJob>, String> {
        let mut jobs: Vec<_> = self
            .jobs
            .lock()
            .map_err(|_| "wipe job store lock was poisoned".to_string())?
            .values()
            .cloned()
            .collect();
        jobs.sort_by_key(|job| std::cmp::Reverse(job.started_at));
        Ok(jobs)
    }

    pub fn begin_verification(&self, id: &str) -> Result<WipeJob, String> {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| "wipe job store lock was poisoned".to_string())?;
        let current = jobs
            .get(id)
            .ok_or_else(|| format!("wipe job {id} was not found"))?;
        if current.status != WipeJobStatus::Running {
            return Err(format!("wipe job {id} is not running"));
        }

        let mut updated = current.clone();
        let now = Utc::now();
        updated.status = WipeJobStatus::Verifying;
        updated.sanitization_completed_at = Some(now);
        append_event_at(
            &mut updated,
            now,
            "sanitization_completed",
            "The sanitization command completed; readback verification started.".to_string(),
        );
        self.persist(&updated)?;
        jobs.insert(id.to_string(), updated.clone());
        Ok(updated)
    }

    pub fn complete_verification(
        &self,
        id: &str,
        verification: VerificationResult,
    ) -> Result<WipeJob, String> {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| "wipe job store lock was poisoned".to_string())?;
        let current = jobs
            .get(id)
            .ok_or_else(|| format!("wipe job {id} was not found"))?;
        if current.status != WipeJobStatus::Verifying {
            return Err(format!("wipe job {id} is not awaiting verification"));
        }

        let expected_size = current
            .identity
            .size_bytes
            .parse::<u64>()
            .map_err(|error| format!("approved device size is invalid: {error}"))?;
        verification
            .validate_for_job(&current.method, expected_size)
            .map_err(|error| format!("invalid wipe verification evidence: {error}"))?;

        let encoded = serde_json::to_string(&verification)
            .map_err(|error| format!("failed to encode verification evidence: {error}"))?;
        let mut updated = current.clone();
        let now = Utc::now();
        updated.status = WipeJobStatus::Verified;
        updated.completed_at = Some(now);
        updated.verification = Some(verification);
        append_event_at(
            &mut updated,
            now,
            "verification_completed",
            format!("Wipe verification passed: {encoded}"),
        );
        self.persist(&updated)?;
        jobs.insert(id.to_string(), updated.clone());
        Ok(updated)
    }

    pub fn fail(&self, id: &str, error: &str) -> Result<WipeJob, String> {
        let mut jobs = self
            .jobs
            .lock()
            .map_err(|_| "wipe job store lock was poisoned".to_string())?;
        let current = jobs
            .get(id)
            .ok_or_else(|| format!("wipe job {id} was not found"))?;
        if !matches!(
            current.status,
            WipeJobStatus::Running | WipeJobStatus::Verifying
        ) {
            return Err(format!("wipe job {id} is already terminal"));
        }

        let mut updated = current.clone();
        let now = Utc::now();
        updated.status = WipeJobStatus::Failed;
        updated.completed_at = Some(now);
        updated.failure = Some(error.to_string());
        updated.verification = None;
        append_event_at(
            &mut updated,
            now,
            "wipe_failed",
            format!("The wipe operation failed: {error}"),
        );
        self.persist(&updated)?;
        jobs.insert(id.to_string(), updated.clone());
        Ok(updated)
    }

    fn fail_interrupted_jobs(&self) -> Result<(), String> {
        let ids: Vec<String> = self
            .jobs
            .lock()
            .map_err(|_| "wipe job store lock was poisoned".to_string())?
            .values()
            .filter(|job| {
                matches!(
                    job.status,
                    WipeJobStatus::Running | WipeJobStatus::Verifying
                )
            })
            .map(|job| job.id.clone())
            .collect();
        for id in ids {
            self.fail(
                &id,
                "backend restarted before terminal evidence was recorded",
            )?;
        }
        Ok(())
    }

    fn persist(&self, job: &WipeJob) -> Result<(), String> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        let encoded = serde_json::to_vec_pretty(job)
            .map_err(|error| format!("failed to encode wipe job: {error}"))?;
        atomic_write(directory, &format!("{}.json", job.id), &encoded)
    }
}

impl Default for JobStore {
    fn default() -> Self {
        Self::in_memory()
    }
}

fn new_job_id() -> String {
    let mut random = [0_u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut random);
    format!("job-{}", hex::encode(random))
}

fn valid_job_id(id: &str) -> bool {
    id.len() == 36
        && id.starts_with("job-")
        && id[4..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn append_event_at(job: &mut WipeJob, timestamp: DateTime<Utc>, event_type: &str, message: String) {
    let sequence = job.events.len() as u64;
    let previous_hash = job.events.last().map(|event| event.event_hash.clone());
    let mut event = EvidenceEvent {
        sequence,
        timestamp,
        event_type: event_type.to_string(),
        message,
        previous_hash,
        event_hash: String::new(),
    };
    event.event_hash = hash_event(job, &event);
    job.evidence_hash = event.event_hash.clone();
    job.events.push(event);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventHashPayload<'a> {
    job_id: &'a str,
    device_path: &'a str,
    device_model: &'a str,
    device_type: &'a str,
    identity: &'a DeviceIdentity,
    method: &'a str,
    sequence: u64,
    timestamp: String,
    event_type: &'a str,
    message: &'a str,
    previous_hash: Option<&'a str>,
}

fn hash_event(job: &WipeJob, event: &EvidenceEvent) -> String {
    let payload = EventHashPayload {
        job_id: &job.id,
        device_path: &job.device_path,
        device_model: &job.device_model,
        device_type: &job.device_type,
        identity: &job.identity,
        method: &job.method,
        sequence: event.sequence,
        timestamp: event.timestamp.to_rfc3339_opts(SecondsFormat::Nanos, true),
        event_type: &event.event_type,
        message: &event.message,
        previous_hash: event.previous_hash.as_deref(),
    };
    let encoded = serde_json::to_vec(&payload).expect("evidence payload is serializable");
    hex::encode(Sha256::digest(encoded))
}

fn atomic_write(directory: &Path, file_name: &str, contents: &[u8]) -> Result<(), String> {
    let destination = directory.join(file_name);
    let temporary = directory.join(format!(".{file_name}.tmp"));
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("failed to open {}: {error}", temporary.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    set_file_permissions(&temporary);
    file.sync_all()
        .map_err(|error| format!("failed to sync {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, &destination).map_err(|error| {
        format!(
            "failed to replace persisted wipe job {}: {error}",
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
