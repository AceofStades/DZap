use serde::{Deserialize, Serialize};
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::process::Command;

use super::drives::{DeviceIdentity, Drive, detect_storage_drives};
use super::wiper::reserve_device;

const SAMPLE_SIZE: u64 = 64 * 1024;
const SAMPLE_COUNT: u64 = 5;
const SAMPLE_ALIGNMENT: u64 = 4096;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAssessmentRequest {
    #[serde(alias = "DevicePath")]
    pub device_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDecision {
    Ready,
    Caution,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryCheckStatus {
    Passed,
    Warning,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionState {
    NotDetected,
    Detected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaCondition {
    Healthy,
    Degraded,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentState {
    StructuredData,
    NonBlank,
    LikelyBlank,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCheck {
    pub code: String,
    pub status: RecoveryCheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySignature {
    pub device_path: String,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartEvidence {
    pub available: bool,
    pub passed: Option<bool>,
    pub reallocated_sectors: Option<u64>,
    pub pending_sectors: Option<u64>,
    pub offline_uncorrectable: Option<u64>,
    pub reported_uncorrectable: Option<u64>,
    pub nvme_media_errors: Option<u64>,
    pub nvme_critical_warning: Option<u64>,
}

impl SmartEvidence {
    pub(crate) fn reports_damage(&self) -> bool {
        self.passed == Some(false)
            || [
                self.reallocated_sectors,
                self.pending_sectors,
                self.offline_uncorrectable,
                self.reported_uncorrectable,
                self.nvme_media_errors,
                self.nvme_critical_warning,
            ]
            .into_iter()
            .flatten()
            .any(|value| value > 0)
    }

    fn has_health_signal(&self) -> bool {
        self.passed.is_some()
            || [
                self.reallocated_sectors,
                self.pending_sectors,
                self.offline_uncorrectable,
                self.reported_uncorrectable,
                self.nvme_media_errors,
                self.nvme_critical_warning,
            ]
            .into_iter()
            .any(|value| value.is_some())
    }

    fn damage_summary(&self) -> String {
        let mut fields = Vec::new();
        for (name, value) in [
            ("reallocated", self.reallocated_sectors),
            ("pending", self.pending_sectors),
            ("offline uncorrectable", self.offline_uncorrectable),
            ("reported uncorrectable", self.reported_uncorrectable),
            ("NVMe media errors", self.nvme_media_errors),
            ("NVMe critical warning", self.nvme_critical_warning),
        ] {
            if let Some(value) = value
                && value > 0
            {
                fields.push(format!("{name}: {value}"));
            }
        }
        if self.passed == Some(false) {
            fields.push("SMART overall status: failed".to_string());
        }
        fields.join(", ")
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentSample {
    pub source_opened: bool,
    pub requested_samples: usize,
    pub completed_samples: usize,
    pub sampled_bytes: u64,
    pub zero_bytes: u64,
    pub ff_bytes: u64,
    pub read_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAssessment {
    pub decision: RecoveryDecision,
    pub device_path: String,
    pub device_model: String,
    pub device_type: String,
    pub identity: Option<DeviceIdentity>,
    pub checks: Vec<RecoveryCheck>,
    pub signatures: Vec<RecoverySignature>,
    pub encryption: EncryptionState,
    pub media_condition: MediaCondition,
    pub content_state: ContentState,
    pub smart: SmartEvidence,
    pub sample: ContentSample,
    pub recommendations: Vec<String>,
}

pub fn assess_recovery(request: &RecoveryAssessmentRequest) -> Result<RecoveryAssessment, String> {
    let drives = detect_storage_drives()?;
    let Some(drive) = drives
        .iter()
        .find(|drive| drive.name == request.device_path)
    else {
        return Ok(missing_device_assessment(&request.device_path));
    };

    if drive.is_os_drive {
        return Ok(protected_device_assessment(drive));
    }

    let _reservation = match reserve_device(&drive.name) {
        Ok(reservation) => reservation,
        Err(error) => return Ok(busy_device_assessment(drive, error)),
    };

    let signatures = detect_signatures(&drive.name);
    let smart = read_smart_evidence(&drive.name);
    let sample = sample_device(&drive.name, drive.size.parse::<u64>().unwrap_or(0));

    Ok(build_assessment(drive, signatures, smart, sample))
}

fn check(code: &str, status: RecoveryCheckStatus, message: impl Into<String>) -> RecoveryCheck {
    RecoveryCheck {
        code: code.to_string(),
        status,
        message: message.into(),
    }
}

fn missing_device_assessment(device_path: &str) -> RecoveryAssessment {
    RecoveryAssessment {
        decision: RecoveryDecision::Blocked,
        device_path: device_path.to_string(),
        device_model: String::new(),
        device_type: String::new(),
        identity: None,
        checks: vec![check(
            "device_exists",
            RecoveryCheckStatus::Blocked,
            "The requested path is not a currently detected whole storage drive.",
        )],
        signatures: Vec::new(),
        encryption: EncryptionState::Unknown,
        media_condition: MediaCondition::Unknown,
        content_state: ContentState::Unknown,
        smart: SmartEvidence::default(),
        sample: ContentSample::default(),
        recommendations: vec![
            "Refresh the device list and select the source drive again.".to_string(),
        ],
    }
}

pub(crate) fn protected_device_assessment(drive: &Drive) -> RecoveryAssessment {
    RecoveryAssessment {
        decision: RecoveryDecision::Blocked,
        device_path: drive.name.clone(),
        device_model: drive.model.clone(),
        device_type: drive.drive_type.to_string(),
        identity: Some(drive.identity()),
        checks: vec![
            check(
                "device_exists",
                RecoveryCheckStatus::Passed,
                "Device is present.",
            ),
            check(
                "protected_system",
                RecoveryCheckStatus::Blocked,
                "The source contains the running system or DZap boot media and was not probed.",
            ),
        ],
        signatures: Vec::new(),
        encryption: EncryptionState::Unknown,
        media_condition: MediaCondition::Unknown,
        content_state: ContentState::Unknown,
        smart: SmartEvidence::default(),
        sample: ContentSample::default(),
        recommendations: vec![
            "Select a source drive that does not contain the running system or DZap boot media."
                .to_string(),
        ],
    }
}

fn busy_device_assessment(drive: &Drive, error: String) -> RecoveryAssessment {
    RecoveryAssessment {
        decision: RecoveryDecision::Blocked,
        device_path: drive.name.clone(),
        device_model: drive.model.clone(),
        device_type: drive.drive_type.to_string(),
        identity: Some(drive.identity()),
        checks: vec![
            check(
                "device_exists",
                RecoveryCheckStatus::Passed,
                "Device is present.",
            ),
            check("active_operation", RecoveryCheckStatus::Blocked, error),
        ],
        signatures: Vec::new(),
        encryption: EncryptionState::Unknown,
        media_condition: MediaCondition::Unknown,
        content_state: ContentState::Unknown,
        smart: SmartEvidence::default(),
        sample: ContentSample::default(),
        recommendations: vec![
            "Wait for the active operation to finish, then assess the source again.".to_string(),
        ],
    }
}

pub(crate) fn build_assessment(
    drive: &Drive,
    signatures: Result<Vec<RecoverySignature>, String>,
    smart: SmartEvidence,
    sample: ContentSample,
) -> RecoveryAssessment {
    let (signatures, signature_error) = match signatures {
        Ok(signatures) => (signatures, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    let encrypted_mapping = drive
        .active_dependencies
        .iter()
        .any(|dependency| dependency.device_type.eq_ignore_ascii_case("crypt"));
    let encrypted_signature = signatures
        .iter()
        .any(|signature| is_encrypted_signature(&signature.value));
    let encryption = if signature_error.is_some() {
        EncryptionState::Unknown
    } else if encrypted_mapping || encrypted_signature {
        EncryptionState::Detected
    } else {
        EncryptionState::NotDetected
    };

    let media_condition =
        if smart.reports_damage() || (sample.source_opened && !sample.read_errors.is_empty()) {
            MediaCondition::Degraded
        } else if smart.available && smart.has_health_signal() {
            MediaCondition::Healthy
        } else {
            MediaCondition::Unknown
        };

    let all_samples_completed = sample.requested_samples > 0
        && sample.completed_samples == sample.requested_samples
        && sample.read_errors.is_empty();
    let uniform_blank_pattern = sample.sampled_bytes > 0
        && (sample.zero_bytes == sample.sampled_bytes || sample.ff_bytes == sample.sampled_bytes);
    let content_state = if !signatures.is_empty() {
        ContentState::StructuredData
    } else if signature_error.is_none() && all_samples_completed && uniform_blank_pattern {
        ContentState::LikelyBlank
    } else if signature_error.is_some() && uniform_blank_pattern {
        ContentState::Unknown
    } else if sample.completed_samples > 0 {
        ContentState::NonBlank
    } else {
        ContentState::Unknown
    };

    let mut checks = vec![
        check(
            "device_exists",
            RecoveryCheckStatus::Passed,
            "Device is present.",
        ),
        check(
            "protected_system",
            RecoveryCheckStatus::Passed,
            "Device does not contain the running system or DZap boot media.",
        ),
        check(
            "mounted_source",
            if drive.is_mounted {
                RecoveryCheckStatus::Warning
            } else {
                RecoveryCheckStatus::Passed
            },
            if drive.is_mounted {
                "The source or one of its children is mounted. Unmount it before imaging or recovery to prevent changes during the operation."
            } else {
                "The source and its children are unmounted."
            },
        ),
        check(
            "active_block_dependencies",
            if drive.active_dependencies.is_empty() {
                RecoveryCheckStatus::Passed
            } else {
                RecoveryCheckStatus::Warning
            },
            if drive.active_dependencies.is_empty() {
                "The source has no active logical storage descendants.".to_string()
            } else {
                format!(
                    "The source backs active logical storage: {}. Deactivate writable mappings before imaging or recovery.",
                    drive
                        .active_dependencies
                        .iter()
                        .map(|dependency| format!(
                            "{} ({})",
                            dependency.name, dependency.device_type
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        ),
    ];

    checks.push(match signature_error {
        Some(error) => check(
            "storage_signatures",
            RecoveryCheckStatus::Unknown,
            format!("Filesystem and partition signatures could not be inspected: {error}"),
        ),
        None if signatures.is_empty() => check(
            "storage_signatures",
            RecoveryCheckStatus::Warning,
            "No filesystem or partition-table signature was detected.",
        ),
        None => check(
            "storage_signatures",
            RecoveryCheckStatus::Passed,
            format!("Detected {} storage signature(s).", signatures.len()),
        ),
    });

    checks.push(match encryption {
        EncryptionState::Detected => check(
            "encryption",
            RecoveryCheckStatus::Warning,
            "Encrypted storage was detected. Recovery requires the correct unlock material and must operate on a read-only decrypted mapping.",
        ),
        EncryptionState::NotDetected => check(
            "encryption",
            RecoveryCheckStatus::Passed,
            "No supported encryption signature or active encrypted mapping was detected.",
        ),
        EncryptionState::Unknown => check(
            "encryption",
            RecoveryCheckStatus::Unknown,
            "Encryption status could not be determined from the available signatures.",
        ),
    });

    checks.push(match media_condition {
        MediaCondition::Degraded => {
            let mut details = smart.damage_summary();
            if !sample.read_errors.is_empty() {
                if !details.is_empty() {
                    details.push_str(", ");
                }
                details.push_str(&format!(
                    "sample read errors: {}",
                    sample.read_errors.len()
                ));
            }
            check(
                "media_health",
                RecoveryCheckStatus::Warning,
                format!("Damage indicators were detected ({details}). Image the source before scanning it."),
            )
        }
        MediaCondition::Healthy => check(
            "media_health",
            RecoveryCheckStatus::Passed,
            "Available SMART data and sparse read probes show no damage indicators. This is not a full surface test.",
        ),
        MediaCondition::Unknown => check(
            "media_health",
            RecoveryCheckStatus::Unknown,
            "SMART health was unavailable. Successful sparse reads cannot rule out bad sectors elsewhere on the source.",
        ),
    });

    checks.push(match content_state {
        ContentState::StructuredData => check(
            "recoverable_content",
            RecoveryCheckStatus::Passed,
            "Recognized filesystem or partition metadata was detected.",
        ),
        ContentState::NonBlank => check(
            "recoverable_content",
            RecoveryCheckStatus::Warning,
            "Sparse samples contain non-blank bytes without recognized storage metadata. Encrypted, damaged, or randomly overwritten content can look the same, so this does not prove recoverability.",
        ),
        ContentState::LikelyBlank => check(
            "recoverable_content",
            RecoveryCheckStatus::Warning,
            "All sparse samples contain one blank pattern and no storage signatures were found. This is consistent with blank or wiped media, but sampling cannot prove a secure wipe.",
        ),
        ContentState::Unknown => check(
            "recoverable_content",
            RecoveryCheckStatus::Unknown,
            "The assessment could not obtain enough readable data to classify the source content.",
        ),
    });

    let mut recommendations = Vec::new();
    if drive.is_mounted {
        recommendations
            .push("Unmount the source and its child volumes before starting recovery.".to_string());
    }
    if !drive.active_dependencies.is_empty() {
        recommendations.push(
            "Deactivate writable RAID, LVM, encrypted, or device-mapper descendants before preserving the physical source."
                .to_string(),
        );
    }
    if media_condition == MediaCondition::Degraded {
        recommendations.push(
            "Create a sector-by-sector image on a separate healthy drive with ddrescue, then scan the image instead of repeatedly reading the source."
                .to_string(),
        );
    }
    if encryption == EncryptionState::Detected {
        recommendations.push(
            "Provide the correct password, recovery key, or key file and create a read-only decrypted mapping before filesystem recovery."
                .to_string(),
        );
    }
    if content_state == ContentState::LikelyBlank {
        recommendations.push(
            "Treat recovery probability as low; a later deep-carving scan may still be attempted on an image."
                .to_string(),
        );
    } else if content_state == ContentState::StructuredData
        && media_condition != MediaCondition::Degraded
        && encryption != EncryptionState::Detected
    {
        recommendations.push(
            "Create an image or read-only snapshot, then try filesystem-aware recovery before raw file carving."
                .to_string(),
        );
    } else if content_state == ContentState::NonBlank {
        recommendations.push(
            "Preserve an image and inspect it with filesystem reconstruction or file-carving tools; non-blank samples alone do not establish what remains."
                .to_string(),
        );
    }
    recommendations.push(
        "Write recovered files only to a separate destination drive; never write them back to the source."
            .to_string(),
    );

    let decision = if checks
        .iter()
        .any(|check| check.status == RecoveryCheckStatus::Blocked)
    {
        RecoveryDecision::Blocked
    } else if checks.iter().any(|check| {
        matches!(
            check.status,
            RecoveryCheckStatus::Warning | RecoveryCheckStatus::Unknown
        )
    }) {
        RecoveryDecision::Caution
    } else {
        RecoveryDecision::Ready
    };

    RecoveryAssessment {
        decision,
        device_path: drive.name.clone(),
        device_model: drive.model.clone(),
        device_type: drive.drive_type.to_string(),
        identity: Some(drive.identity()),
        checks,
        signatures,
        encryption,
        media_condition,
        content_state,
        smart,
        sample,
        recommendations,
    }
}

pub(crate) fn is_encrypted_signature(signature: &str) -> bool {
    let normalized = signature.to_ascii_lowercase();
    normalized.contains("crypto_luks")
        || normalized.contains("bitlocker")
        || normalized == "luks"
        || normalized == "fve-fs"
}

#[derive(Debug, Deserialize)]
struct SignatureInventory {
    blockdevices: Vec<SignatureNode>,
}

#[derive(Debug, Deserialize)]
struct SignatureNode {
    path: String,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    pttype: Option<String>,
    #[serde(default)]
    children: Vec<SignatureNode>,
}

fn detect_signatures(device_path: &str) -> Result<Vec<RecoverySignature>, String> {
    let output = Command::new("lsblk")
        .args(["-J", "-p", "-o", "PATH,FSTYPE,PTTYPE"])
        .output()
        .map_err(|error| format!("lsblk command failed: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "lsblk command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    signatures_from_lsblk(&output.stdout, device_path)
}

pub(crate) fn signatures_from_lsblk(
    stdout: &[u8],
    device_path: &str,
) -> Result<Vec<RecoverySignature>, String> {
    let inventory: SignatureInventory = serde_json::from_slice(stdout)
        .map_err(|error| format!("failed to parse lsblk signature data: {error}"))?;
    let root = inventory
        .blockdevices
        .iter()
        .find(|node| node.path == device_path)
        .ok_or_else(|| format!("device {device_path} was absent from signature scan"))?;
    let mut signatures = Vec::new();
    collect_signatures(root, &mut signatures);
    Ok(signatures)
}

fn collect_signatures(node: &SignatureNode, signatures: &mut Vec<RecoverySignature>) {
    if let Some(partition_table) = node.pttype.as_deref().filter(|value| !value.is_empty()) {
        signatures.push(RecoverySignature {
            device_path: node.path.clone(),
            kind: "partition_table".to_string(),
            value: partition_table.to_string(),
        });
    }
    if let Some(filesystem) = node.fstype.as_deref().filter(|value| !value.is_empty()) {
        signatures.push(RecoverySignature {
            device_path: node.path.clone(),
            kind: "filesystem".to_string(),
            value: filesystem.to_string(),
        });
    }
    for child in &node.children {
        collect_signatures(child, signatures);
    }
}

fn read_smart_evidence(device_path: &str) -> SmartEvidence {
    let Ok(output) = Command::new("smartctl")
        .args(["-a", "-j", device_path])
        .output()
    else {
        return SmartEvidence::default();
    };
    smart_evidence_from_json(&output.stdout).unwrap_or_default()
}

pub(crate) fn smart_evidence_from_json(stdout: &[u8]) -> Result<SmartEvidence, String> {
    let value: serde_json::Value = serde_json::from_slice(stdout)
        .map_err(|error| format!("failed to parse smartctl JSON: {error}"))?;
    let mut evidence = SmartEvidence {
        available: true,
        passed: value
            .pointer("/smart_status/passed")
            .and_then(serde_json::Value::as_bool),
        nvme_media_errors: json_u64(&value, "/nvme_smart_health_information_log/media_errors"),
        nvme_critical_warning: json_u64(
            &value,
            "/nvme_smart_health_information_log/critical_warning",
        ),
        ..SmartEvidence::default()
    };

    if let Some(attributes) = value
        .pointer("/ata_smart_attributes/table")
        .and_then(serde_json::Value::as_array)
    {
        for attribute in attributes {
            let Some(id) = attribute.get("id").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let raw = attribute.pointer("/raw/value").and_then(value_as_u64);
            match id {
                5 => evidence.reallocated_sectors = raw,
                187 => evidence.reported_uncorrectable = raw,
                197 => evidence.pending_sectors = raw,
                198 => evidence.offline_uncorrectable = raw,
                _ => {}
            }
        }
    }
    Ok(evidence)
}

fn json_u64(value: &serde_json::Value, pointer: &str) -> Option<u64> {
    value.pointer(pointer).and_then(value_as_u64)
}

fn value_as_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

pub(crate) fn sample_device(device_path: &str, size: u64) -> ContentSample {
    let offsets = sample_offsets(size);
    let mut sample = ContentSample {
        requested_samples: offsets.len(),
        ..ContentSample::default()
    };
    if offsets.is_empty() {
        sample
            .read_errors
            .push("device size is unavailable or zero".to_string());
        return sample;
    }

    let file = match File::open(device_path) {
        Ok(file) => file,
        Err(error) => {
            sample
                .read_errors
                .push(format!("could not open the source read-only: {error}"));
            return sample;
        }
    };
    sample.source_opened = true;

    for offset in offsets {
        let length = SAMPLE_SIZE.min(size.saturating_sub(offset)) as usize;
        let mut buffer = vec![0_u8; length];
        match read_exact_at(&file, &mut buffer, offset) {
            Ok(()) => {
                sample.completed_samples += 1;
                sample.sampled_bytes += buffer.len() as u64;
                sample.zero_bytes += buffer.iter().filter(|byte| **byte == 0).count() as u64;
                sample.ff_bytes += buffer.iter().filter(|byte| **byte == 0xff).count() as u64;
            }
            Err(error) => sample
                .read_errors
                .push(format!("read at byte offset {offset} failed: {error}")),
        }
    }
    sample
}

fn sample_offsets(size: u64) -> Vec<u64> {
    if size == 0 {
        return Vec::new();
    }
    let max_offset = size.saturating_sub(SAMPLE_SIZE);
    let mut offsets = (0..SAMPLE_COUNT)
        .map(|index| {
            let raw = max_offset.saturating_mul(index) / (SAMPLE_COUNT - 1);
            raw / SAMPLE_ALIGNMENT * SAMPLE_ALIGNMENT
        })
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    offsets.dedup();
    offsets
}

fn read_exact_at(file: &File, mut buffer: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !buffer.is_empty() {
        let read = file.read_at(buffer, offset)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "source ended during sample",
            ));
        }
        offset += read as u64;
        buffer = &mut buffer[read..];
    }
    Ok(())
}
