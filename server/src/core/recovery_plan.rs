use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::drives::{DeviceIdentity, Drive, detect_storage_drives};
use super::evidence_export::{self, ExportDestination};
use super::wiper::{device_is_reserved, reserve_device};

const IMAGE_RESERVE_BYTES: u64 = 64 * 1024 * 1024;
const FAT32_MAX_FILE_BYTES: u64 = (4 * 1024 * 1024 * 1024) - 1;
const RECOVERY_DIRECTORY: &str = "DZap-Recovery";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryDestination {
    pub drive_path: String,
    pub device_path: String,
    pub device_major_minor: String,
    pub mount_path: Option<String>,
    pub filesystem: String,
    pub size_bytes: String,
    pub drive_identity: DeviceIdentity,
    pub filesystem_size_bytes: Option<String>,
    pub available_bytes: Option<String>,
    pub read_only: Option<bool>,
    pub capacity_error: Option<String>,
}

impl RecoveryDestination {
    fn as_export_destination(&self) -> ExportDestination {
        ExportDestination {
            drive_path: self.drive_path.clone(),
            drive_major_minor: self.drive_identity.major_minor.clone(),
            device_path: self.device_path.clone(),
            device_major_minor: self.device_major_minor.clone(),
            mount_path: self.mount_path.clone(),
            model: self.drive_identity.model.clone(),
            serial: self.drive_identity.serial.clone(),
            transport: self.drive_identity.transport.clone(),
            filesystem: self.filesystem.clone(),
            size_bytes: self.size_bytes.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryMountRequest {
    pub source_device_path: String,
    pub expected_source_identity: DeviceIdentity,
    pub destination: RecoveryDestination,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryPlanRequest {
    pub source_device_path: String,
    pub expected_source_identity: DeviceIdentity,
    pub destination: RecoveryDestination,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPlanDecision {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPlanCheckStatus {
    Passed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryPlanCheck {
    pub code: String,
    pub status: RecoveryPlanCheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryImagePlan {
    pub decision: RecoveryPlanDecision,
    pub source_device_path: String,
    pub source_identity: Option<DeviceIdentity>,
    pub destination: Option<RecoveryDestination>,
    pub image_size_bytes: String,
    pub reserve_bytes: String,
    pub required_bytes: String,
    pub output_directory: Option<String>,
    pub checks: Vec<RecoveryPlanCheck>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FilesystemCapacity {
    pub(crate) size_bytes: u64,
    pub(crate) available_bytes: u64,
    pub(crate) read_only: bool,
}

pub fn detect_recovery_destinations(
    source_device_path: &str,
) -> Result<Vec<RecoveryDestination>, String> {
    let drives = detect_storage_drives()?;
    let candidates = evidence_export::detect_export_destinations()?;
    recovery_destinations_from_candidates(source_device_path, &drives, candidates, |path| {
        filesystem_capacity(path)
    })
}

pub fn mount_recovery_destination(
    request: &RecoveryMountRequest,
) -> Result<RecoveryDestination, String> {
    let drives = detect_storage_drives()?;
    let source = current_source(
        &drives,
        &request.source_device_path,
        &request.expected_source_identity,
    )?;
    let destination_drive = current_destination_drive(&drives, &request.destination)?;
    ensure_separate_drives(source, destination_drive)?;

    let _source_reservation = reserve_device(&source.name)?;
    let _destination_reservation = reserve_device(&destination_drive.name)?;
    let mounted =
        evidence_export::mount_export_destination(&request.destination.as_export_destination())?;
    destination_from_export(&mounted, destination_drive, filesystem_capacity)
}

pub fn plan_recovery_image(request: &RecoveryPlanRequest) -> Result<RecoveryImagePlan, String> {
    let drives = detect_storage_drives()?;
    let source = drives
        .iter()
        .find(|drive| drive.name == request.source_device_path);
    let candidates = evidence_export::detect_export_destinations()?;
    let destinations =
        recovery_destinations_from_candidates("", &drives, candidates, filesystem_capacity)?;
    let destination = destinations.iter().find(|candidate| {
        candidate.device_path == request.destination.device_path
            && candidate.device_major_minor == request.destination.device_major_minor
    });

    let source_reservation_error = match source {
        Some(drive) if device_is_reserved(&drive.name)? => Some(format!(
            "another storage operation is already active for device {}",
            drive.name
        )),
        Some(_) => None,
        None => Some("recovery source is unavailable".to_string()),
    };
    let destination_reservation_error = match destination {
        Some(candidate) if device_is_reserved(&candidate.drive_path)? => Some(format!(
            "another storage operation is already active for device {}",
            candidate.drive_path
        )),
        Some(_) => None,
        None => Some("recovery destination is unavailable".to_string()),
    };

    Ok(build_image_plan(
        request,
        source,
        destination,
        source_reservation_error,
        destination_reservation_error,
    ))
}

fn current_source<'a>(
    drives: &'a [Drive],
    path: &str,
    expected_identity: &DeviceIdentity,
) -> Result<&'a Drive, String> {
    let source = drives
        .iter()
        .find(|drive| drive.name == path)
        .ok_or_else(|| "recovery source changed or disappeared".to_string())?;
    if &source.identity() != expected_identity {
        return Err("recovery source identity changed; assess it again".to_string());
    }
    if source.is_os_drive {
        return Err(
            "the running system or DZap boot media cannot be a recovery source".to_string(),
        );
    }
    Ok(source)
}

fn current_destination_drive<'a>(
    drives: &'a [Drive],
    destination: &RecoveryDestination,
) -> Result<&'a Drive, String> {
    let drive = drives
        .iter()
        .find(|drive| drive.name == destination.drive_path)
        .ok_or_else(|| "recovery destination changed or disappeared".to_string())?;
    if drive.identity() != destination.drive_identity {
        return Err("recovery destination identity changed; select it again".to_string());
    }
    if drive.is_os_drive {
        return Err(
            "the running system or DZap boot media cannot be a recovery destination".to_string(),
        );
    }
    Ok(drive)
}

fn ensure_separate_drives(source: &Drive, destination: &Drive) -> Result<(), String> {
    if source.name == destination.name
        || (!source.major_minor.is_empty() && source.major_minor == destination.major_minor)
    {
        return Err(
            "the recovery source and destination must be different physical drives".to_string(),
        );
    }
    Ok(())
}

pub(crate) fn recovery_destinations_from_candidates<F>(
    source_device_path: &str,
    drives: &[Drive],
    candidates: Vec<ExportDestination>,
    mut capacity_for: F,
) -> Result<Vec<RecoveryDestination>, String>
where
    F: FnMut(&Path) -> Result<FilesystemCapacity, String>,
{
    let mut destinations = Vec::new();
    for candidate in candidates {
        if candidate.drive_path == source_device_path {
            continue;
        }
        let Some(drive) = drives
            .iter()
            .find(|drive| drive.name == candidate.drive_path)
        else {
            continue;
        };
        if drive.is_os_drive {
            continue;
        }
        destinations.push(destination_from_export(
            &candidate,
            drive,
            &mut capacity_for,
        )?);
    }
    destinations.sort_by(|left, right| left.device_path.cmp(&right.device_path));
    Ok(destinations)
}

fn destination_from_export<F>(
    candidate: &ExportDestination,
    drive: &Drive,
    mut capacity_for: F,
) -> Result<RecoveryDestination, String>
where
    F: FnMut(&Path) -> Result<FilesystemCapacity, String>,
{
    let (filesystem_size_bytes, available_bytes, read_only, capacity_error) =
        match candidate.mount_path.as_deref() {
            Some(mount_path) => match capacity_for(Path::new(mount_path)) {
                Ok(capacity) => (
                    Some(capacity.size_bytes.to_string()),
                    Some(capacity.available_bytes.to_string()),
                    Some(capacity.read_only),
                    None,
                ),
                Err(error) => (None, None, None, Some(error)),
            },
            None => (None, None, None, None),
        };

    Ok(RecoveryDestination {
        drive_path: candidate.drive_path.clone(),
        device_path: candidate.device_path.clone(),
        device_major_minor: candidate.device_major_minor.clone(),
        mount_path: candidate.mount_path.clone(),
        filesystem: candidate.filesystem.clone(),
        size_bytes: candidate.size_bytes.clone(),
        drive_identity: drive.identity(),
        filesystem_size_bytes,
        available_bytes,
        read_only,
        capacity_error,
    })
}

fn filesystem_capacity(path: &Path) -> Result<FilesystemCapacity, String> {
    let path_bytes = path.as_os_str().as_bytes();
    let path = CString::new(path_bytes)
        .map_err(|_| "destination mount path contains a null byte".to_string())?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a valid null-terminated string and `stats` points to writable memory.
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(format!(
            "failed to read destination capacity: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: a successful `statvfs` call initialized the complete structure.
    let stats = unsafe { stats.assume_init() };
    let block_size = if stats.f_frsize > 0 {
        stats.f_frsize
    } else {
        stats.f_bsize
    };
    Ok(FilesystemCapacity {
        size_bytes: stats.f_blocks.saturating_mul(block_size),
        available_bytes: stats.f_bavail.saturating_mul(block_size),
        read_only: stats.f_flag & libc::ST_RDONLY != 0,
    })
}

fn plan_check(
    code: &str,
    passed: bool,
    passed_message: impl Into<String>,
    blocked_message: impl Into<String>,
) -> RecoveryPlanCheck {
    RecoveryPlanCheck {
        code: code.to_string(),
        status: if passed {
            RecoveryPlanCheckStatus::Passed
        } else {
            RecoveryPlanCheckStatus::Blocked
        },
        message: if passed {
            passed_message.into()
        } else {
            blocked_message.into()
        },
    }
}

pub(crate) fn build_image_plan(
    request: &RecoveryPlanRequest,
    source: Option<&Drive>,
    destination: Option<&RecoveryDestination>,
    source_reservation_error: Option<String>,
    destination_reservation_error: Option<String>,
) -> RecoveryImagePlan {
    let source_size = source
        .and_then(|drive| drive.size.parse::<u64>().ok())
        .unwrap_or(0);
    let required_bytes = source_size.saturating_add(IMAGE_RESERVE_BYTES);
    let source_identity_matches = source
        .map(|drive| drive.identity() == request.expected_source_identity)
        .unwrap_or(false);
    let source_size_known = source_size > 0;
    let source_safe = source.map(|drive| !drive.is_os_drive).unwrap_or(false);
    let source_quiescent = source
        .map(|drive| !drive.is_mounted && drive.active_dependencies.is_empty())
        .unwrap_or(false);
    let destination_identity_matches = destination
        .map(|candidate| candidate.drive_identity == request.destination.drive_identity)
        .unwrap_or(false);
    let destination_volume_matches = destination
        .map(|candidate| {
            candidate.drive_path == request.destination.drive_path
                && candidate.device_path == request.destination.device_path
                && candidate.device_major_minor == request.destination.device_major_minor
                && candidate.mount_path == request.destination.mount_path
                && candidate.filesystem == request.destination.filesystem
                && candidate.size_bytes == request.destination.size_bytes
        })
        .unwrap_or(false);
    let separate_drives = match (source, destination) {
        (Some(source), Some(destination)) => {
            source.name != destination.drive_path
                && (source.major_minor.is_empty()
                    || source.major_minor != destination.drive_identity.major_minor)
        }
        _ => false,
    };
    let mounted = destination
        .and_then(|candidate| candidate.mount_path.as_ref())
        .is_some();
    let writable = destination.and_then(|candidate| candidate.read_only) == Some(false);
    let file_size_supported = destination
        .map(|candidate| candidate.filesystem != "vfat" || source_size <= FAT32_MAX_FILE_BYTES)
        .unwrap_or(false);
    let enough_space = destination
        .and_then(|candidate| candidate.available_bytes.as_deref())
        .and_then(|bytes| bytes.parse::<u64>().ok())
        .map(|available| available >= required_bytes)
        .unwrap_or(false);

    let mut checks = vec![
        plan_check(
            "source_present",
            source.is_some(),
            "Recovery source is present.",
            "Recovery source changed or disappeared; assess it again.",
        ),
        plan_check(
            "source_identity",
            source_identity_matches,
            "Recovery source identity matches the assessment.",
            "Recovery source identity changed; assess and select it again.",
        ),
        plan_check(
            "source_size",
            source_size_known,
            format!("Recovery source size is {source_size} bytes."),
            "Recovery source size is unavailable or zero.",
        ),
        plan_check(
            "protected_source",
            source_safe,
            "Recovery source is not the running system or DZap boot media.",
            "The running system or DZap boot media cannot be used as a recovery source.",
        ),
        plan_check(
            "source_quiescent",
            source_quiescent,
            "Recovery source is unmounted and has no active logical descendants.",
            "Unmount the source and deactivate writable RAID, LVM, crypt, or device-mapper descendants before imaging.",
        ),
        plan_check(
            "destination_present",
            destination.is_some(),
            "Recovery destination is present and eligible.",
            "Recovery destination changed, disappeared, or is no longer eligible.",
        ),
        plan_check(
            "destination_identity",
            destination_identity_matches,
            "Recovery destination identity matches the selection.",
            "Recovery destination identity changed; select it again.",
        ),
        plan_check(
            "destination_volume",
            destination_volume_matches,
            "Destination volume identity and mount path match the selection.",
            "Destination volume, filesystem, or mount path changed; select it again.",
        ),
        plan_check(
            "separate_destination",
            separate_drives,
            "Source and destination are different physical drives.",
            "Recovery output must use a different physical drive from the source.",
        ),
        plan_check(
            "destination_mounted",
            mounted,
            "Recovery destination filesystem is mounted.",
            "Mount the selected recovery destination before building the image plan.",
        ),
        plan_check(
            "destination_writable",
            writable,
            "Recovery destination is mounted read-write.",
            destination
                .and_then(|candidate| candidate.capacity_error.clone())
                .unwrap_or_else(|| {
                    "Recovery destination is read-only or its mount state could not be verified."
                        .to_string()
                }),
        ),
        plan_check(
            "image_file_size",
            file_size_supported,
            "Destination filesystem supports an image file as large as the source.",
            "FAT32 cannot store this source as one image file; use exFAT or ext4.",
        ),
        plan_check(
            "destination_capacity",
            enough_space,
            format!(
                "Destination has enough free space for the source image plus a {} MiB reserve.",
                IMAGE_RESERVE_BYTES / 1024 / 1024
            ),
            format!(
                "Destination needs at least {required_bytes} available bytes for the image and recovery metadata."
            ),
        ),
        plan_check(
            "source_available",
            source_reservation_error.is_none(),
            "No other storage operation is using the source.",
            source_reservation_error
                .unwrap_or_else(|| "Recovery source is unavailable.".to_string()),
        ),
        plan_check(
            "destination_available",
            destination_reservation_error.is_none(),
            "No other storage operation is using the destination.",
            destination_reservation_error
                .unwrap_or_else(|| "Recovery destination is unavailable.".to_string()),
        ),
    ];

    if source.is_none() {
        for check in &mut checks[1..5] {
            check.status = RecoveryPlanCheckStatus::Blocked;
        }
    }

    let decision = if checks
        .iter()
        .all(|check| check.status == RecoveryPlanCheckStatus::Passed)
    {
        RecoveryPlanDecision::Ready
    } else {
        RecoveryPlanDecision::Blocked
    };
    let output_directory = destination
        .and_then(|candidate| candidate.mount_path.as_deref())
        .map(|mount_path| {
            Path::new(mount_path)
                .join(RECOVERY_DIRECTORY)
                .display()
                .to_string()
        });

    RecoveryImagePlan {
        decision,
        source_device_path: request.source_device_path.clone(),
        source_identity: source.map(Drive::identity),
        destination: destination.cloned(),
        image_size_bytes: source_size.to_string(),
        reserve_bytes: IMAGE_RESERVE_BYTES.to_string(),
        required_bytes: required_bytes.to_string(),
        output_directory,
        checks,
    }
}
