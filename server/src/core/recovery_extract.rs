use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use zeroize::Zeroize;

use super::recovery_imaging::{
    RecoveryControlGuard, RecoveryRequestedAction, send_interrupt, stop_child,
};
use super::recovery_jobs::{
    RecoveryJob, RecoveryJobStatus, RecoveryJobStore, RecoveryMethod, RecoveryResult,
    TestdiskAnalysis,
};

const RECOVERY_RESERVE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryVolume {
    pub id: String,
    pub kind: String,
    pub size_bytes: String,
    pub filesystem: Option<String>,
    pub label: Option<String>,
    pub encryption: Option<String>,
    pub filesystem_copy_supported: bool,
    pub photorec_supported: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRunRequest {
    pub method: RecoveryMethod,
    pub volume_id: String,
    #[serde(default)]
    pub passphrase: Option<String>,
}

impl Drop for RecoveryRunRequest {
    fn drop(&mut self) {
        if let Some(passphrase) = self.passphrase.as_mut() {
            passphrase.zeroize();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DetectedVolume {
    pub(crate) public: RecoveryVolume,
    device_path: String,
}

pub fn inspect_recovery_image(job: &RecoveryJob) -> Result<Vec<RecoveryVolume>, String> {
    validate_completed_image(job)?;
    let mut attachment = LoopAttachment::open(Path::new(&job.image_path))?;
    let volumes = detect_attached_volumes(&attachment.device_path)?;
    attachment.detach()?;
    Ok(volumes.into_iter().map(|volume| volume.public).collect())
}

pub fn analyze_recovery_image(
    job: &RecoveryJob,
    store: &RecoveryJobStore,
) -> Result<TestdiskAnalysis, String> {
    analyze_recovery_image_with_program(Path::new("testdisk"), job, store)
}

pub(crate) fn testdisk_arguments(image_path: &Path) -> Vec<OsString> {
    vec![
        OsString::from("/list"),
        image_path.as_os_str().to_os_string(),
    ]
}

pub(crate) fn analyze_recovery_image_with_program(
    program: &Path,
    job: &RecoveryJob,
    store: &RecoveryJobStore,
) -> Result<TestdiskAnalysis, String> {
    validate_completed_image(job)?;
    if job.status != RecoveryJobStatus::ImageComplete {
        return Err("TestDisk analysis requires a newly completed image".to_string());
    }
    let attempt = job
        .events
        .iter()
        .filter(|event| event.event_type == "testdisk_analyzed")
        .count()
        + 1;
    let log_path = Path::new(&job.job_directory).join(format!("testdisk-{attempt}.log"));
    remove_unrecorded_log(&log_path)?;
    let output = Command::new(program)
        .args(testdisk_arguments(Path::new(&job.image_path)))
        .current_dir(&job.job_directory)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("failed to start TestDisk: {error}"))?;
    let mut log = create_new_file(&log_path)?;
    log.write_all(b"=== TestDisk stdout ===\n")
        .and_then(|()| log.write_all(&output.stdout))
        .and_then(|()| log.write_all(b"\n=== TestDisk stderr ===\n"))
        .and_then(|()| log.write_all(&output.stderr))
        .map_err(|error| format!("failed to write TestDisk analysis log: {error}"))?;
    log.sync_all()
        .map_err(|error| format!("failed to sync TestDisk analysis log: {error}"))?;
    drop(log);
    let summary_source = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let summary: String = String::from_utf8_lossy(summary_source)
        .chars()
        .take(2_000)
        .collect();
    let analysis = TestdiskAnalysis {
        completed_at: Utc::now(),
        successful: output.status.success(),
        log_path: log_path.display().to_string(),
        log_sha256: hash_path(&log_path)?,
        summary: summary.trim().to_string(),
    };
    store.record_testdisk_analysis(&job.id, analysis.clone())?;
    Ok(analysis)
}

fn remove_unrecorded_log(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            std::fs::remove_file(path).map_err(|error| {
                format!(
                    "failed to remove incomplete TestDisk log {}: {error}",
                    path.display()
                )
            })
        }
        Ok(_) => Err(format!(
            "incomplete TestDisk log {} is not a regular file",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "failed to inspect incomplete TestDisk log {}: {error}",
            path.display()
        )),
    }
}

pub fn run_recovery(
    job: RecoveryJob,
    mut request: RecoveryRunRequest,
    store: &RecoveryJobStore,
    progress_tx: &UnboundedSender<String>,
    control: &RecoveryControlGuard,
) -> Result<RecoveryJob, String> {
    validate_completed_image(&job)?;
    let attempt = job
        .events
        .iter()
        .filter(|event| event.event_type == "extraction_started")
        .count()
        + 1;
    let job_directory = Path::new(&job.job_directory);
    let method_name = match request.method {
        RecoveryMethod::FilesystemCopy => "filesystem_copy",
        RecoveryMethod::Photorec => "photorec",
    };
    let output_base = job_directory.join(format!("{method_name}-files-{attempt}"));
    let output_directory = if request.method == RecoveryMethod::Photorec {
        PathBuf::from(format!("{}.1", output_base.display()))
    } else {
        output_base.clone()
    };
    if output_base.exists() || output_directory.exists() {
        return Err("recovery attempt output already exists".to_string());
    }
    let manifest_path = job_directory.join(format!("{method_name}-{attempt}.manifest.jsonl"));
    let active_job = store.begin_recovery(
        &job.id,
        request.method,
        output_directory.display().to_string(),
    )?;
    if request.method == RecoveryMethod::FilesystemCopy {
        create_private_directory(&output_directory)?;
    }

    let mut attachment = LoopAttachment::open(Path::new(&job.image_path))?;
    let volumes = detect_attached_volumes(&attachment.device_path)?;
    let selected = volumes
        .into_iter()
        .find(|volume| volume.public.id == request.volume_id)
        .ok_or_else(|| {
            format!(
                "recovery volume {} changed or disappeared",
                request.volume_id
            )
        })?;

    let mut unlocked = None;
    let (recovery_device, filesystem) = if let Some(encryption) = &selected.public.encryption {
        let mut passphrase = request
            .passphrase
            .take()
            .ok_or_else(|| format!("{encryption} recovery requires an unlock secret"))?;
        let mapping = CryptoMapping::open(
            &selected.device_path,
            encryption,
            &active_job.id,
            &mut passphrase,
        )?;
        let filesystem = probe_filesystem(&mapping.device_path)?;
        let device_path = mapping.device_path.clone();
        unlocked = Some(mapping);
        (device_path, filesystem)
    } else {
        if let Some(mut passphrase) = request.passphrase.take() {
            passphrase.zeroize();
        }
        (
            selected.device_path.clone(),
            selected.public.filesystem.clone(),
        )
    };

    let result = match request.method {
        RecoveryMethod::FilesystemCopy => {
            let filesystem = filesystem
                .as_deref()
                .ok_or_else(|| "selected volume has no mountable filesystem".to_string())?;
            if !filesystem_copy_supported(filesystem) {
                return Err(format!(
                    "filesystem {filesystem} is not supported for read-only file copy"
                ));
            }
            let mut mounted = ReadOnlyMount::open(&recovery_device, filesystem, &active_job.id)?;
            let scan = scan_tree(&mounted.path)?;
            ensure_recovery_capacity(job_directory, scan.regular_bytes)?;
            let manifest = copy_tree(
                &mounted.path,
                &output_directory,
                &manifest_path,
                store,
                &active_job.id,
                progress_tx,
                control,
            )?;
            mounted.unmount()?;
            RecoveryResult {
                method: request.method,
                output_directory: output_directory.display().to_string(),
                manifest_path: manifest_path.display().to_string(),
                manifest_sha256: manifest.sha256,
                recovered_file_count: manifest.file_count,
                recovered_bytes: manifest.bytes,
                skipped_entries: manifest.skipped_entries,
            }
        }
        RecoveryMethod::Photorec => {
            ensure_recovery_capacity(
                job_directory,
                selected.public.size_bytes.parse().unwrap_or(0),
            )?;
            let log_path = job_directory.join(format!("photorec-{attempt}.log"));
            let working_directory = job_directory.join(format!("photorec-work-{attempt}"));
            create_private_directory(&working_directory)?;
            run_photorec(
                &recovery_device,
                &output_base,
                &output_directory,
                &log_path,
                &working_directory,
                filesystem.as_deref(),
                store,
                &active_job.id,
                progress_tx,
                control,
            )?;
            let manifest = hash_tree(&output_directory, &manifest_path)?;
            RecoveryResult {
                method: request.method,
                output_directory: output_directory.display().to_string(),
                manifest_path: manifest_path.display().to_string(),
                manifest_sha256: manifest.sha256,
                recovered_file_count: manifest.file_count,
                recovered_bytes: manifest.bytes,
                skipped_entries: manifest.skipped_entries,
            }
        }
    };

    if let Some(mut mapping) = unlocked {
        mapping.close()?;
    }
    attachment.detach()?;
    store.complete_recovery(&job.id, result)
}

fn validate_completed_image(job: &RecoveryJob) -> Result<(), String> {
    if !job.verify_evidence() {
        return Err("recovery job evidence verification failed".to_string());
    }
    if !job
        .events
        .iter()
        .any(|event| event.event_type == "image_completed")
        || !matches!(
            job.status,
            RecoveryJobStatus::ImageComplete
                | RecoveryJobStatus::Completed
                | RecoveryJobStatus::Cancelled
                | RecoveryJobStatus::Failed
        )
    {
        return Err("recovery job does not have a completed source image".to_string());
    }
    job.validate_artifact_binding()?;
    let metadata = std::fs::symlink_metadata(&job.image_path)
        .map_err(|error| format!("failed to inspect recovery image: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("recovery image is not a regular file".to_string());
    }
    Ok(())
}

struct LoopAttachment {
    device_path: String,
    active: bool,
}

impl LoopAttachment {
    fn open(image_path: &Path) -> Result<Self, String> {
        let output = Command::new("losetup")
            .args(["--find", "--show", "--read-only", "--partscan"])
            .arg(image_path)
            .output()
            .map_err(|error| format!("failed to start losetup: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "failed to attach recovery image read-only: {}",
                command_diagnostic(&output)
            ));
        }
        let device_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let valid_loop_path = device_path.strip_prefix("/dev/loop").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit())
        });
        if !valid_loop_path {
            return Err("losetup returned an invalid loop-device path".to_string());
        }
        let _ = Command::new("udevadm").arg("settle").status();
        Ok(Self {
            device_path,
            active: true,
        })
    }

    fn detach(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        let output = Command::new("losetup")
            .args(["--detach", &self.device_path])
            .output()
            .map_err(|error| format!("failed to start losetup detach: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "failed to detach {}: {}",
                self.device_path,
                command_diagnostic(&output)
            ));
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for LoopAttachment {
    fn drop(&mut self) {
        if self.active {
            let _ = Command::new("losetup")
                .args(["--detach", &self.device_path])
                .status();
        }
    }
}

fn detect_attached_volumes(loop_device: &str) -> Result<Vec<DetectedVolume>, String> {
    let output = Command::new("lsblk")
        .args([
            "--json",
            "--bytes",
            "--paths",
            "--output",
            "PATH,TYPE,FSTYPE,SIZE,LABEL,PARTN",
            loop_device,
        ])
        .output()
        .map_err(|error| format!("failed to start lsblk for recovery image: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "failed to inspect recovery image volumes: {}",
            command_diagnostic(&output)
        ));
    }
    parse_lsblk_volumes(&output.stdout)
}

pub(crate) fn parse_lsblk_volumes(contents: &[u8]) -> Result<Vec<DetectedVolume>, String> {
    let root: serde_json::Value = serde_json::from_slice(contents)
        .map_err(|error| format!("failed to parse recovery lsblk output: {error}"))?;
    let devices = root
        .get("blockdevices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "recovery lsblk output has no blockdevices array".to_string())?;
    let mut volumes = Vec::new();
    for device in devices {
        collect_volume_nodes(device, true, &mut volumes)?;
    }
    if volumes.is_empty() {
        return Err("recovery image exposes no usable volumes".to_string());
    }
    Ok(volumes)
}

fn collect_volume_nodes(
    node: &serde_json::Value,
    is_root: bool,
    volumes: &mut Vec<DetectedVolume>,
) -> Result<(), String> {
    let device_path = json_string(node.get("path"))
        .ok_or_else(|| "recovery volume is missing its device path".to_string())?;
    let device_type = json_string(node.get("type")).unwrap_or_else(|| "unknown".to_string());
    if is_root || device_type == "part" {
        let filesystem = json_string(node.get("fstype"));
        let encryption = encryption_kind(filesystem.as_deref());
        let part_number = json_u64(node.get("partn"));
        let id = if is_root {
            "whole-disk".to_string()
        } else if let Some(number) = part_number {
            format!("partition-{number}")
        } else {
            format!("volume-{}", volumes.len())
        };
        let size = json_u64(node.get("size")).unwrap_or(0);
        let can_copy =
            encryption.is_some() || filesystem.as_deref().is_some_and(filesystem_copy_supported);
        volumes.push(DetectedVolume {
            public: RecoveryVolume {
                id,
                kind: if is_root { "whole_disk" } else { "partition" }.to_string(),
                size_bytes: size.to_string(),
                filesystem,
                label: json_string(node.get("label")),
                encryption,
                filesystem_copy_supported: can_copy,
                photorec_supported: size > 0,
            },
            device_path,
        });
    }
    if let Some(children) = node.get("children").and_then(serde_json::Value::as_array) {
        for child in children {
            collect_volume_nodes(child, false, volumes)?;
        }
    }
    Ok(())
}

fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(value) if !value.is_empty() => Some(value.clone()),
        _ => None,
    }
}

fn json_u64(value: Option<&serde_json::Value>) -> Option<u64> {
    match value? {
        serde_json::Value::Number(value) => value.as_u64(),
        serde_json::Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn encryption_kind(filesystem: Option<&str>) -> Option<String> {
    match filesystem?.to_ascii_lowercase().as_str() {
        "crypto_luks" => Some("luks".to_string()),
        "bitlocker" | "bitlk" => Some("bitlk".to_string()),
        _ => None,
    }
}

fn filesystem_copy_supported(filesystem: &str) -> bool {
    matches!(
        filesystem.to_ascii_lowercase().as_str(),
        "ext2" | "ext3" | "ext4" | "xfs" | "btrfs" | "vfat" | "exfat" | "ntfs" | "ntfs3"
    )
}

struct CryptoMapping {
    name: String,
    device_path: String,
    active: bool,
}

pub(crate) fn cryptsetup_open_arguments(
    source: &str,
    encryption: &str,
    mapping_name: &str,
) -> Vec<OsString> {
    vec![
        OsString::from("open"),
        OsString::from("--readonly"),
        OsString::from("--batch-mode"),
        OsString::from("--type"),
        OsString::from(encryption),
        OsString::from("--key-file=-"),
        OsString::from(source),
        OsString::from(mapping_name),
    ]
}

impl CryptoMapping {
    fn open(
        source: &str,
        encryption: &str,
        job_id: &str,
        passphrase: &mut String,
    ) -> Result<Self, String> {
        let suffix: String = job_id
            .chars()
            .filter(|character| character.is_ascii_hexdigit())
            .rev()
            .take(16)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let name = format!("dzap-recovery-{suffix}-{:08x}", rand::random::<u32>());
        let mut child = match Command::new("cryptsetup")
            .args(cryptsetup_open_arguments(source, encryption, &name))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                passphrase.zeroize();
                return Err(format!("failed to start cryptsetup: {error}"));
            }
        };
        let write_result = child
            .stdin
            .take()
            .ok_or_else(|| "cryptsetup stdin was unavailable".to_string())
            .and_then(|mut stdin| {
                stdin
                    .write_all(passphrase.as_bytes())
                    .map_err(|error| format!("failed to provide cryptsetup secret: {error}"))
            });
        passphrase.zeroize();
        if let Err(error) = write_result {
            stop_child(&mut child);
            return Err(error);
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("failed to wait for cryptsetup: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "failed to unlock encrypted recovery volume read-only: {}",
                command_diagnostic(&output)
            ));
        }
        Ok(Self {
            device_path: format!("/dev/mapper/{name}"),
            name,
            active: true,
        })
    }

    fn close(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        let output = Command::new("cryptsetup")
            .args(["close", &self.name])
            .output()
            .map_err(|error| format!("failed to start cryptsetup close: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "failed to close encrypted recovery mapping: {}",
                command_diagnostic(&output)
            ));
        }
        self.active = false;
        Ok(())
    }
}

impl Drop for CryptoMapping {
    fn drop(&mut self) {
        if self.active {
            let _ = Command::new("cryptsetup")
                .args(["close", &self.name])
                .status();
        }
    }
}

fn probe_filesystem(device_path: &str) -> Result<Option<String>, String> {
    let output = Command::new("blkid")
        .args([
            "--probe",
            "--output",
            "value",
            "--match-tag",
            "TYPE",
            device_path,
        ])
        .output()
        .map_err(|error| format!("failed to start blkid: {error}"))?;
    if output.status.success() {
        let filesystem = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Ok((!filesystem.is_empty()).then_some(filesystem));
    }
    if output.status.code() == Some(2) {
        return Ok(None);
    }
    Err(format!(
        "failed to inspect unlocked filesystem: {}",
        command_diagnostic(&output)
    ))
}

struct ReadOnlyMount {
    path: PathBuf,
    active: bool,
}

impl ReadOnlyMount {
    fn open(source: &str, filesystem: &str, job_id: &str) -> Result<Self, String> {
        let root = Path::new("/run/dzap/recovery-mounts");
        std::fs::create_dir_all(root)
            .map_err(|error| format!("failed to create recovery mount root: {error}"))?;
        let root_metadata = std::fs::symlink_metadata(root)
            .map_err(|error| format!("failed to inspect recovery mount root: {error}"))?;
        if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
            return Err("recovery mount root is not a physical directory".to_string());
        }
        set_directory_permissions(root);
        let suffix = job_id.trim_start_matches("recovery-");
        let path = root.join(format!(
            "{}-{}",
            &suffix[..suffix.len().min(16)],
            rand::random::<u64>()
        ));
        create_private_directory(&path)?;
        let options = mount_options(filesystem)?;
        let output = Command::new("mount")
            .args(["--types", filesystem, "--options", options, source])
            .arg(&path)
            .output()
            .map_err(|error| format!("failed to start read-only mount: {error}"))?;
        if !output.status.success() {
            let _ = std::fs::remove_dir(&path);
            return Err(format!(
                "failed to mount recovered filesystem read-only: {}",
                command_diagnostic(&output)
            ));
        }
        Ok(Self { path, active: true })
    }

    fn unmount(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        let output = Command::new("umount")
            .arg(&self.path)
            .output()
            .map_err(|error| format!("failed to start recovery unmount: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "failed to unmount recovery filesystem: {}",
                command_diagnostic(&output)
            ));
        }
        self.active = false;
        std::fs::remove_dir(&self.path)
            .map_err(|error| format!("failed to remove recovery mount point: {error}"))?;
        Ok(())
    }
}

impl Drop for ReadOnlyMount {
    fn drop(&mut self) {
        if self.active {
            let _ = Command::new("umount").arg(&self.path).status();
            let _ = std::fs::remove_dir(&self.path);
        }
    }
}

fn mount_options(filesystem: &str) -> Result<&'static str, String> {
    match filesystem.to_ascii_lowercase().as_str() {
        "ext3" | "ext4" => Ok("ro,noload,nodev,nosuid,noexec"),
        "xfs" => Ok("ro,norecovery,nodev,nosuid,noexec"),
        "ext2" | "btrfs" | "vfat" | "exfat" | "ntfs" | "ntfs3" => Ok("ro,nodev,nosuid,noexec"),
        _ => Err(format!(
            "filesystem {filesystem} is not supported for read-only mounting"
        )),
    }
}

#[derive(Default)]
struct TreeSummary {
    file_count: u64,
    regular_bytes: u64,
    skipped_entries: u64,
}

fn scan_tree(root: &Path) -> Result<TreeSummary, String> {
    let mut summary = TreeSummary::default();
    visit_tree(root, root, &mut |_, path, metadata| {
        if metadata.is_file() {
            summary.file_count = summary.file_count.saturating_add(1);
            summary.regular_bytes = summary.regular_bytes.saturating_add(metadata.len());
        } else if !metadata.is_dir() {
            summary.skipped_entries = summary.skipped_entries.saturating_add(1);
        }
        let _ = path;
        Ok(())
    })?;
    Ok(summary)
}

fn visit_tree<F>(root: &Path, directory: &Path, visitor: &mut F) -> Result<(), String>
where
    F: FnMut(&Path, &Path, &std::fs::Metadata) -> Result<(), String>,
{
    let mut entries = std::fs::read_dir(directory)
        .map_err(|error| {
            format!(
                "failed to read recovered directory {}: {error}",
                directory.display()
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read recovered directory entry: {error}"))?;
    entries.sort_by(|left, right| {
        left.file_name()
            .as_bytes()
            .cmp(right.file_name().as_bytes())
    });
    for entry in entries {
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "failed to inspect recovered entry {}: {error}",
                path.display()
            )
        })?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "recovered path escaped its mounted root".to_string())?;
        visitor(relative, &path, &metadata)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            visit_tree(root, &path, visitor)?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileHashRecord {
    relative_path: String,
    relative_path_bytes_hex: String,
    size_bytes: u64,
    sha256: String,
}

pub(crate) struct ManifestSummary {
    pub(crate) file_count: u64,
    pub(crate) bytes: u64,
    pub(crate) skipped_entries: u64,
    pub(crate) sha256: String,
}

pub(crate) fn copy_tree(
    source_root: &Path,
    destination_root: &Path,
    manifest_path: &Path,
    store: &RecoveryJobStore,
    job_id: &str,
    progress_tx: &UnboundedSender<String>,
    control: &RecoveryControlGuard,
) -> Result<ManifestSummary, String> {
    let temporary_manifest = PathBuf::from(format!("{}.partial", manifest_path.display()));
    let mut manifest = create_new_file(&temporary_manifest)?;
    let mut file_count = 0_u64;
    let mut bytes = 0_u64;
    let mut skipped_entries = 0_u64;
    let mut last_progress = Instant::now();
    visit_tree(
        source_root,
        source_root,
        &mut |relative, source, metadata| {
            if control.requested_action() == RecoveryRequestedAction::Cancel {
                return Err("recovery file copy cancelled by the operator".to_string());
            }
            let destination = destination_root.join(relative);
            if metadata.is_dir() {
                create_private_directory(&destination)?;
            } else if metadata.is_file() && !metadata.file_type().is_symlink() {
                let record = copy_and_hash_file(source, &destination, relative, control)?;
                serde_json::to_writer(&mut manifest, &record).map_err(|error| {
                    format!("failed to encode recovery manifest record: {error}")
                })?;
                manifest
                    .write_all(b"\n")
                    .map_err(|error| format!("failed to write recovery manifest: {error}"))?;
                file_count = file_count.saturating_add(1);
                bytes = bytes.saturating_add(record.size_bytes);
            } else {
                skipped_entries = skipped_entries.saturating_add(1);
            }
            if last_progress.elapsed() >= Duration::from_secs(1) {
                let updated = store.update_recovery_progress(job_id, file_count, bytes)?;
                send_recovery_progress(progress_tx, &updated, file_count, bytes);
                last_progress = Instant::now();
            }
            Ok(())
        },
    )?;
    finalize_manifest(
        manifest,
        &temporary_manifest,
        manifest_path,
        file_count,
        bytes,
        skipped_entries,
    )
}

fn hash_tree(root: &Path, manifest_path: &Path) -> Result<ManifestSummary, String> {
    let metadata = std::fs::symlink_metadata(root).map_err(|error| {
        format!(
            "failed to inspect PhotoRec output {}: {error}",
            root.display()
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("PhotoRec output is not a physical directory".to_string());
    }
    let temporary_manifest = PathBuf::from(format!("{}.partial", manifest_path.display()));
    let mut manifest = create_new_file(&temporary_manifest)?;
    let mut file_count = 0_u64;
    let mut bytes = 0_u64;
    let mut skipped_entries = 0_u64;
    visit_tree(root, root, &mut |relative, path, metadata| {
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            let record = hash_file(path, relative)?;
            serde_json::to_writer(&mut manifest, &record)
                .map_err(|error| format!("failed to encode recovery manifest record: {error}"))?;
            manifest
                .write_all(b"\n")
                .map_err(|error| format!("failed to write recovery manifest: {error}"))?;
            file_count = file_count.saturating_add(1);
            bytes = bytes.saturating_add(record.size_bytes);
        } else if !metadata.is_dir() {
            skipped_entries = skipped_entries.saturating_add(1);
        }
        Ok(())
    })?;
    finalize_manifest(
        manifest,
        &temporary_manifest,
        manifest_path,
        file_count,
        bytes,
        skipped_entries,
    )
}

fn copy_and_hash_file(
    source: &Path,
    destination: &Path,
    relative: &Path,
    control: &RecoveryControlGuard,
) -> Result<FileHashRecord, String> {
    let mut source_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|error| {
            format!(
                "failed to open recovered file {}: {error}",
                source.display()
            )
        })?;
    let mut destination_file = create_new_file(destination)?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        if control.requested_action() == RecoveryRequestedAction::Cancel {
            return Err("recovery file copy cancelled by the operator".to_string());
        }
        let read = source_file.read(&mut buffer).map_err(|error| {
            format!(
                "failed to read recovered file {}: {error}",
                source.display()
            )
        })?;
        if read == 0 {
            break;
        }
        destination_file
            .write_all(&buffer[..read])
            .map_err(|error| {
                format!(
                    "failed to write recovered file {}: {error}",
                    destination.display()
                )
            })?;
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }
    destination_file.sync_all().map_err(|error| {
        format!(
            "failed to sync recovered file {}: {error}",
            destination.display()
        )
    })?;
    Ok(file_record(relative, size, hex::encode(hasher.finalize())))
}

fn hash_file(path: &Path, relative: &Path) -> Result<FileHashRecord, String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| format!("failed to open recovered file {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            format!("failed to hash recovered file {}: {error}", path.display())
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }
    Ok(file_record(relative, size, hex::encode(hasher.finalize())))
}

fn file_record(relative: &Path, size: u64, sha256: String) -> FileHashRecord {
    FileHashRecord {
        relative_path: relative.to_string_lossy().to_string(),
        relative_path_bytes_hex: hex::encode(relative.as_os_str().as_bytes()),
        size_bytes: size,
        sha256,
    }
}

fn finalize_manifest(
    manifest: File,
    temporary_path: &Path,
    final_path: &Path,
    file_count: u64,
    bytes: u64,
    skipped_entries: u64,
) -> Result<ManifestSummary, String> {
    manifest
        .sync_all()
        .map_err(|error| format!("failed to sync recovery manifest: {error}"))?;
    drop(manifest);
    std::fs::rename(temporary_path, final_path)
        .map_err(|error| format!("failed to finalize recovery manifest: {error}"))?;
    let parent = final_path
        .parent()
        .ok_or_else(|| "recovery manifest has no parent directory".to_string())?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("failed to sync recovery manifest directory: {error}"))?;
    let sha256 = hash_path(final_path)?;
    Ok(ManifestSummary {
        file_count,
        bytes,
        skipped_entries,
        sha256,
    })
}

fn hash_path(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("failed to open {} for hashing: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn create_new_file(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    set_file_permissions(path);
    Ok(file)
}

fn create_private_directory(path: &Path) -> Result<(), String> {
    std::fs::create_dir(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    set_directory_permissions(path);
    Ok(())
}

fn ensure_recovery_capacity(path: &Path, output_bytes: u64) -> Result<(), String> {
    let available = available_bytes(path)?;
    let required = output_bytes.saturating_add(RECOVERY_RESERVE_BYTES);
    if available < required {
        return Err(format!(
            "recovery output needs {required} available bytes, but only {available} remain"
        ));
    }
    Ok(())
}

fn available_bytes(path: &Path) -> Result<u64, String> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "recovery destination path contains a null byte".to_string())?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is null-terminated and `stats` points to writable memory.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(format!(
            "failed to read recovery destination capacity: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: successful statvfs initialized the structure.
    let stats = unsafe { stats.assume_init() };
    let block_size = if stats.f_frsize > 0 {
        stats.f_frsize
    } else {
        stats.f_bsize
    };
    Ok(stats.f_bavail.saturating_mul(block_size))
}

pub(crate) fn photorec_arguments(
    source: &OsStr,
    output_base: &Path,
    log_path: &Path,
    filesystem: Option<&str>,
) -> Vec<OsString> {
    let command = if filesystem.is_some_and(|filesystem| {
        matches!(
            filesystem.to_ascii_lowercase().as_str(),
            "ext2" | "ext3" | "ext4"
        )
    }) {
        "partition_none,options,mode_ext2,fileopt,everything,enable,search"
    } else {
        "partition_none,fileopt,everything,enable,search"
    };
    vec![
        OsString::from("/log"),
        OsString::from("/logname"),
        log_path.as_os_str().to_os_string(),
        OsString::from("/d"),
        output_base.as_os_str().to_os_string(),
        OsString::from("/cmd"),
        source.to_os_string(),
        OsString::from(command),
    ]
}

#[allow(clippy::too_many_arguments)]
fn run_photorec(
    source: &str,
    output_base: &Path,
    output_directory: &Path,
    log_path: &Path,
    working_directory: &Path,
    filesystem: Option<&str>,
    store: &RecoveryJobStore,
    job_id: &str,
    progress_tx: &UnboundedSender<String>,
    control: &RecoveryControlGuard,
) -> Result<(), String> {
    let console_path = PathBuf::from(format!("{}.console", log_path.display()));
    let console = create_new_file(&console_path)?;
    let console_stdout = console
        .try_clone()
        .map_err(|error| format!("failed to clone PhotoRec log handle: {error}"))?;
    let mut child = Command::new("photorec")
        .args(photorec_arguments(
            OsStr::new(source),
            output_base,
            log_path,
            filesystem,
        ))
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::from(console_stdout))
        .stderr(Stdio::from(console))
        .spawn()
        .map_err(|error| format!("failed to start PhotoRec: {error}"))?;
    let mut last_progress = Instant::now();
    loop {
        if control.requested_action() == RecoveryRequestedAction::Cancel {
            if let Err(error) = send_interrupt(&mut child) {
                stop_child(&mut child);
                return Err(error);
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    _ => {
                        stop_child(&mut child);
                        break;
                    }
                }
            }
            return Err("PhotoRec recovery cancelled by the operator".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                return Err(format!(
                    "PhotoRec exited with {status}; inspect {}",
                    console_path.display()
                ));
            }
            Ok(None) => {}
            Err(error) => {
                stop_child(&mut child);
                return Err(format!("failed to inspect PhotoRec process: {error}"));
            }
        }
        if last_progress.elapsed() >= Duration::from_secs(2) && output_directory.exists() {
            if let Ok(summary) = scan_tree(output_directory) {
                let updated = match store.update_recovery_progress(
                    job_id,
                    summary.file_count,
                    summary.regular_bytes,
                ) {
                    Ok(updated) => updated,
                    Err(error) => {
                        stop_child(&mut child);
                        return Err(error);
                    }
                };
                send_recovery_progress(
                    progress_tx,
                    &updated,
                    summary.file_count,
                    summary.regular_bytes,
                );
            }
            last_progress = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryProgress<'a> {
    operation: &'static str,
    status: &'static str,
    job_id: &'a str,
    device_id: &'a str,
    recovered_file_count: u64,
    recovered_bytes: u64,
    message: &'a str,
}

fn send_recovery_progress(
    progress_tx: &UnboundedSender<String>,
    job: &RecoveryJob,
    recovered_file_count: u64,
    recovered_bytes: u64,
) {
    let progress = RecoveryProgress {
        operation: "recovery_extraction",
        status: "extracting",
        job_id: &job.id,
        device_id: &job.source_device_path,
        recovered_file_count,
        recovered_bytes,
        message: &job.last_message,
    };
    if let Ok(encoded) = serde_json::to_string(&progress) {
        let _ = progress_tx.send(encoded);
    }
}

fn command_diagnostic(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return stdout;
    }
    output.status.to_string()
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
