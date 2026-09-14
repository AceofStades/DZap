use serde::Serialize;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

use super::recovery_jobs::{RecoveryJob, RecoveryJobStore, RescueMapSummary};

const ACTION_NONE: u8 = 0;
const ACTION_PAUSE: u8 = 1;
const ACTION_CANCEL: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryRequestedAction {
    None,
    Pause,
    Cancel,
}

fn active_controls() -> &'static Mutex<HashMap<String, Arc<AtomicU8>>> {
    static CONTROLS: OnceLock<Mutex<HashMap<String, Arc<AtomicU8>>>> = OnceLock::new();
    CONTROLS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct RecoveryControlGuard {
    job_id: String,
    action: Arc<AtomicU8>,
}

impl RecoveryControlGuard {
    pub(crate) fn requested_action(&self) -> RecoveryRequestedAction {
        match self.action.load(Ordering::SeqCst) {
            ACTION_PAUSE => RecoveryRequestedAction::Pause,
            ACTION_CANCEL => RecoveryRequestedAction::Cancel,
            _ => RecoveryRequestedAction::None,
        }
    }
}

impl Drop for RecoveryControlGuard {
    fn drop(&mut self) {
        if let Ok(mut controls) = active_controls().lock() {
            controls.remove(&self.job_id);
        }
    }
}

pub fn register_recovery_control(job_id: &str) -> Result<RecoveryControlGuard, String> {
    let action = Arc::new(AtomicU8::new(ACTION_NONE));
    let mut controls = active_controls()
        .lock()
        .map_err(|_| "recovery control state is unavailable".to_string())?;
    if controls.contains_key(job_id) {
        return Err(format!("recovery job {job_id} is already active"));
    }
    controls.insert(job_id.to_string(), action.clone());
    Ok(RecoveryControlGuard {
        job_id: job_id.to_string(),
        action,
    })
}

pub fn request_recovery_pause(job_id: &str) -> Result<(), String> {
    request_action(job_id, ACTION_PAUSE)
}

pub fn request_recovery_cancel(job_id: &str) -> Result<(), String> {
    request_action(job_id, ACTION_CANCEL)
}

fn request_action(job_id: &str, action: u8) -> Result<(), String> {
    let controls = active_controls()
        .lock()
        .map_err(|_| "recovery control state is unavailable".to_string())?;
    let control = controls
        .get(job_id)
        .ok_or_else(|| format!("recovery job {job_id} is not active"))?;
    control.store(action, Ordering::SeqCst);
    Ok(())
}

pub fn ddrescue_arguments(job: &RecoveryJob) -> Vec<OsString> {
    vec![
        OsString::from("--no-scrape"),
        OsString::from("--retry-passes=3"),
        OsString::from("--sparse"),
        OsString::from(&job.source_device_path),
        OsString::from(&job.image_path),
        OsString::from(&job.map_path),
    ]
}

pub fn run_ddrescue(
    job: RecoveryJob,
    store: &RecoveryJobStore,
    progress_tx: &UnboundedSender<String>,
    control: &RecoveryControlGuard,
) -> Result<RecoveryJob, String> {
    run_ddrescue_with_program(Path::new("ddrescue"), job, store, progress_tx, control)
}

pub(crate) fn run_ddrescue_with_program(
    program: &Path,
    job: RecoveryJob,
    store: &RecoveryJobStore,
    progress_tx: &UnboundedSender<String>,
    control: &RecoveryControlGuard,
) -> Result<RecoveryJob, String> {
    validate_regular_artifact(Path::new(&job.image_path), true)?;
    validate_regular_artifact(Path::new(&job.map_path), false)?;
    validate_regular_artifact(Path::new(&job.log_path), false)?;

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&job.log_path)
        .map_err(|error| format!("failed to open ddrescue log {}: {error}", job.log_path))?;
    let log_stdout = log
        .try_clone()
        .map_err(|error| format!("failed to clone ddrescue log handle: {error}"))?;
    let mut child = Command::new(program)
        .args(ddrescue_arguments(&job))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_stdout))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", program.display()))?;

    let mut last_summary = job.map_summary.clone();
    let mut last_progress_write = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);
    let mut signal_sent_at = None;
    loop {
        if let Ok(summary) =
            read_ddrescue_map(Path::new(&job.map_path), job.map_summary.total_bytes)
            && summary != last_summary
            && last_progress_write.elapsed() >= Duration::from_secs(1)
        {
            let updated = match store.update_progress(
                &job.id,
                summary.clone(),
                "ddrescue is imaging the source into the destination file.",
            ) {
                Ok(updated) => updated,
                Err(error) => {
                    stop_child(&mut child);
                    return Err(error);
                }
            };
            send_progress(progress_tx, &updated);
            last_summary = summary;
            last_progress_write = Instant::now();
        }

        let action = control.requested_action();
        if action != RecoveryRequestedAction::None && signal_sent_at.is_none() {
            if let Err(error) = send_interrupt(&mut child) {
                stop_child(&mut child);
                return Err(error);
            }
            signal_sent_at = Some(Instant::now());
        }
        if signal_sent_at.is_some_and(|sent| sent.elapsed() >= Duration::from_secs(10)) {
            stop_child(&mut child);
            let summary = read_ddrescue_map(Path::new(&job.map_path), job.map_summary.total_bytes)
                .unwrap_or(last_summary);
            store.update_progress(
                &job.id,
                summary,
                "ddrescue was stopped after its graceful shutdown deadline.",
            )?;
            return match action {
                RecoveryRequestedAction::Pause => store.pause(
                    &job.id,
                    "Imaging paused after ddrescue was stopped; the map file was retained.",
                ),
                RecoveryRequestedAction::Cancel => store.cancel(
                    &job.id,
                    "Recovery cancelled; partial image and map artifacts were retained.",
                ),
                RecoveryRequestedAction::None => unreachable!("a signal was sent for an action"),
            };
        }

        let child_status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                stop_child(&mut child);
                return Err(format!("failed to inspect ddrescue process: {error}"));
            }
        };
        match child_status {
            Some(status) => {
                let summary =
                    read_ddrescue_map(Path::new(&job.map_path), job.map_summary.total_bytes)
                        .unwrap_or(last_summary);
                return match action {
                    RecoveryRequestedAction::Pause => store.pause(
                        &job.id,
                        "Imaging paused safely; the ddrescue map file can resume this job.",
                    ),
                    RecoveryRequestedAction::Cancel => store.cancel(
                        &job.id,
                        "Recovery cancelled; partial image and map artifacts were retained.",
                    ),
                    _ if status.success() && summary.pending_bytes == 0 => {
                        store.complete_image(&job.id, summary)
                    }
                    _ if status.success() => store.pause(
                        &job.id,
                        format!(
                            "ddrescue exited with {} bytes still pending; the map file is resumable.",
                            summary.pending_bytes
                        ),
                    ),
                    _ => {
                        let diagnostic = read_log_tail(Path::new(&job.log_path), 4096)
                            .unwrap_or_else(|_| format!("ddrescue exited with {status}"));
                        store.pause(
                            &job.id,
                            format!(
                                "ddrescue stopped before completion; the map file is resumable. {}",
                                diagnostic.trim()
                            ),
                        )
                    }
                };
            }
            None => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

pub(crate) fn stop_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn validate_regular_artifact(path: &Path, required: bool) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            Ok(())
        }
        Ok(_) => Err(format!(
            "recovery artifact {} is not a regular file",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => Ok(()),
        Err(error) => Err(format!(
            "failed to inspect recovery artifact {}: {error}",
            path.display()
        )),
    }
}

pub(crate) fn send_interrupt(child: &mut std::process::Child) -> Result<(), String> {
    #[cfg(unix)]
    {
        // SAFETY: `child.id()` is the PID returned by `spawn`; SIGINT asks ddrescue to flush its map.
        if unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) } != 0 {
            return Err(format!(
                "failed to interrupt ddrescue: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        child
            .kill()
            .map_err(|error| format!("failed to stop ddrescue: {error}"))
    }
}

pub fn read_ddrescue_map(path: &Path, total_hint: u64) -> Result<RescueMapSummary, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read ddrescue map {}: {error}", path.display()))?;
    parse_ddrescue_map(&contents, total_hint)
}

pub fn parse_ddrescue_map(contents: &str, total_hint: u64) -> Result<RescueMapSummary, String> {
    let mut summary = RescueMapSummary::default();
    let mut mapped_bytes = 0_u64;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 || fields[2].len() != 1 {
            continue;
        }
        let Some(status) = fields[2].chars().next() else {
            continue;
        };
        if !matches!(status, '+' | '-' | '?' | '*' | '/') {
            continue;
        }
        let Some(size) = parse_map_number(fields[1]) else {
            continue;
        };
        mapped_bytes = mapped_bytes.saturating_add(size);
        match status {
            '+' => summary.rescued_bytes = summary.rescued_bytes.saturating_add(size),
            '-' => summary.unreadable_bytes = summary.unreadable_bytes.saturating_add(size),
            _ => summary.pending_bytes = summary.pending_bytes.saturating_add(size),
        }
    }
    summary.total_bytes = total_hint.max(mapped_bytes);
    if mapped_bytes < summary.total_bytes {
        summary.pending_bytes = summary
            .pending_bytes
            .saturating_add(summary.total_bytes - mapped_bytes);
    }
    if summary.total_bytes == 0 {
        return Err("ddrescue map contains no ranges and source size is unknown".to_string());
    }
    Ok(summary)
}

fn parse_map_number(value: &str) -> Option<u64> {
    value
        .strip_prefix("0x")
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .or_else(|| value.parse().ok())
}

fn read_log_tail(path: &Path, maximum_bytes: u64) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let length = file
        .metadata()
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?
        .len();
    if length > maximum_bytes {
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(length - maximum_bytes))
            .map_err(|error| format!("failed to seek {}: {error}", path.display()))?;
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    Ok(text)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryProgress<'a> {
    operation: &'static str,
    status: &'static str,
    job_id: &'a str,
    device_id: &'a str,
    percentage: f64,
    rescued_bytes: u64,
    unreadable_bytes: u64,
    pending_bytes: u64,
    message: &'a str,
}

fn send_progress(progress_tx: &UnboundedSender<String>, job: &RecoveryJob) {
    let progress = RecoveryProgress {
        operation: "recovery_imaging",
        status: "running",
        job_id: &job.id,
        device_id: &job.source_device_path,
        percentage: job.progress_percent,
        rescued_bytes: job.map_summary.rescued_bytes,
        unreadable_bytes: job.map_summary.unreadable_bytes,
        pending_bytes: job.map_summary.pending_bytes,
        message: &job.last_message,
    };
    if let Ok(encoded) = serde_json::to_string(&progress) {
        let _ = progress_tx.send(encoded);
    }
}
