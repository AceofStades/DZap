use serde::Serialize;
use std::process::Command;

use super::ata::{self, AtaEraseMode};
use super::drives::{
    DeviceIdentity, Drive, DriveType, MobileDevice, detect_android_devices, detect_storage_drives,
};
use super::nvme::{self, NvmeSanitizeAction};
use super::wiper::{WipeConfig, get_wipe_methods_for_drive, get_wipe_methods_for_mobile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightDecision {
    Ready,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightCheckStatus {
    Passed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightCheck {
    pub code: String,
    pub status: PreflightCheckStatus,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WipePlan {
    pub decision: PreflightDecision,
    pub device_path: String,
    pub device_model: String,
    pub device_type: String,
    pub method: String,
    pub identity: Option<DeviceIdentity>,
    pub checks: Vec<PreflightCheck>,
}

impl WipePlan {
    pub fn is_ready(&self) -> bool {
        self.decision == PreflightDecision::Ready
    }

    pub fn blocking_message(&self) -> String {
        self.checks
            .iter()
            .filter(|check| check.status == PreflightCheckStatus::Blocked)
            .map(|check| check.message.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn add_checks(&mut self, checks: Vec<PreflightCheck>) {
        if checks
            .iter()
            .any(|check| check.status == PreflightCheckStatus::Blocked)
        {
            self.decision = PreflightDecision::Blocked;
        }
        self.checks.extend(checks);
    }
}

fn check(
    code: &str,
    passed: bool,
    passed_message: &str,
    blocked_message: String,
) -> PreflightCheck {
    PreflightCheck {
        code: code.to_string(),
        status: if passed {
            PreflightCheckStatus::Passed
        } else {
            PreflightCheckStatus::Blocked
        },
        message: if passed {
            passed_message.to_string()
        } else {
            blocked_message
        },
    }
}

fn finish_plan(
    config: &WipeConfig,
    device_model: String,
    device_type: String,
    identity: Option<DeviceIdentity>,
    checks: Vec<PreflightCheck>,
) -> WipePlan {
    let decision = if checks
        .iter()
        .any(|check| check.status == PreflightCheckStatus::Blocked)
    {
        PreflightDecision::Blocked
    } else {
        PreflightDecision::Ready
    };

    WipePlan {
        decision,
        device_path: config.device_path.clone(),
        device_model,
        device_type,
        method: config.method.clone(),
        identity,
        checks,
    }
}

fn missing_device_plan(config: &WipeConfig, identifier: &str) -> WipePlan {
    finish_plan(
        config,
        config.device_model.clone().unwrap_or_default(),
        config.device_type.clone(),
        None,
        vec![check(
            "device_exists",
            false,
            "Device is present.",
            format!("Device {identifier} was not found during preflight."),
        )],
    )
}

fn identity_check(config: &WipeConfig, detected: &DeviceIdentity) -> PreflightCheck {
    match &config.expected_identity {
        Some(expected) => check(
            "device_identity",
            expected == detected,
            "Device identity matches the approved preflight plan.",
            "Device identity changed after preflight. Review the device and confirm it again."
                .to_string(),
        ),
        None => check(
            "device_identity",
            false,
            "Device identity matches the approved preflight plan.",
            "No approved device identity was supplied. Run preflight and confirm the device again."
                .to_string(),
        ),
    }
}

fn evaluate_storage_wipe_with_identity(
    config: &WipeConfig,
    drive: &Drive,
    require_identity: bool,
) -> WipePlan {
    let supported_methods = get_wipe_methods_for_drive(drive);
    let method_supported = supported_methods
        .iter()
        .any(|method| method.id == config.method);
    let supported_ids = supported_methods
        .iter()
        .map(|method| method.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let detected_identity = drive.identity();
    let mut checks = vec![
        check("device_exists", true, "Device is present.", String::new()),
        check(
            "method_supported",
            method_supported,
            "Requested wipe method is supported for this device type.",
            format!(
                "Method {} is not supported for {}. Supported methods: {}.",
                config.method, drive.drive_type, supported_ids
            ),
        ),
        check(
            "os_drive",
            !drive.is_os_drive,
            "Device does not contain the running operating system.",
            "The running operating-system drive cannot be wiped.".to_string(),
        ),
        check(
            "mounted",
            !drive.is_mounted,
            "Device and its partitions are unmounted.",
            "The device or one of its partitions is mounted.".to_string(),
        ),
        check(
            "active_block_dependencies",
            drive.active_dependencies.is_empty(),
            "Device does not back an active RAID, LVM, encrypted, or device-mapper volume.",
            format!(
                "The device backs active logical storage: {}. Deactivate it before wiping the physical drive.",
                drive
                    .active_dependencies
                    .iter()
                    .map(|dependency| format!("{} ({})", dependency.name, dependency.device_type))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
        check(
            "frozen",
            !(drive.drive_type == super::drives::DriveType::Ssd && drive.is_frozen),
            "Device is not in an ATA security-frozen state.",
            "The SATA SSD is in an ATA security-frozen state.".to_string(),
        ),
    ];
    if require_identity {
        checks.push(identity_check(config, &detected_identity));
    }

    finish_plan(
        config,
        drive.model.clone(),
        drive.drive_type.to_string(),
        Some(detected_identity),
        checks,
    )
}

pub fn evaluate_storage_wipe(config: &WipeConfig, drive: &Drive) -> WipePlan {
    evaluate_storage_wipe_with_identity(config, drive, false)
}

pub fn authorize_storage_wipe(config: &WipeConfig, drive: &Drive) -> WipePlan {
    evaluate_storage_wipe_with_identity(config, drive, true)
}

fn evaluate_mobile_wipe_with_identity(
    config: &WipeConfig,
    device: &MobileDevice,
    require_identity: bool,
) -> WipePlan {
    let method_supported = get_wipe_methods_for_mobile(device)
        .iter()
        .any(|method| method.id == config.method);
    let detected_identity = device.identity();
    let mut checks = vec![
        check(
            "device_exists",
            true,
            "Device is present and available through ADB.",
            String::new(),
        ),
        check(
            "method_supported",
            method_supported,
            "Requested wipe method is supported for this device type.",
            format!(
                "Method {} is not supported for {} devices.",
                config.method, device.device_type
            ),
        ),
    ];
    if require_identity {
        checks.push(identity_check(config, &detected_identity));
    }

    finish_plan(
        config,
        device.model.clone(),
        device.device_type.clone(),
        Some(detected_identity),
        checks,
    )
}

pub fn evaluate_mobile_wipe(config: &WipeConfig, device: &MobileDevice) -> WipePlan {
    evaluate_mobile_wipe_with_identity(config, device, false)
}

pub fn authorize_mobile_wipe(config: &WipeConfig, device: &MobileDevice) -> WipePlan {
    evaluate_mobile_wipe_with_identity(config, device, true)
}

fn decimal_prefix(value: &str) -> Result<u64, String> {
    let digits: String = value
        .trim_start()
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return Err(format!("missing sector count in {value:?}"));
    }
    digits
        .parse::<u64>()
        .map_err(|error| format!("invalid sector count {digits:?}: {error}"))
}

pub fn parse_hpa_sector_counts(output: &str) -> Result<(u64, u64), String> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("setting seems invalid") || lower.contains("buggy kernel") {
        return Err("hdparm reported an invalid HPA sector count".to_string());
    }

    let line = output
        .lines()
        .find(|line| line.to_ascii_lowercase().contains("max sectors"))
        .ok_or_else(|| "hdparm output did not contain max sectors".to_string())?;
    let (_, counts) = line
        .split_once('=')
        .ok_or_else(|| "hdparm max sectors line did not contain '='".to_string())?;
    let (visible, native) = counts.split_once('/').ok_or_else(|| {
        "hdparm max sectors line did not contain visible/native values".to_string()
    })?;

    Ok((decimal_prefix(visible)?, decimal_prefix(native)?))
}

pub fn parse_dco_real_max_sectors(output: &str) -> Result<u64, String> {
    if output.to_ascii_lowercase().contains("checksum failed") {
        return Err("hdparm reported a failed DCO checksum".to_string());
    }

    let line = output
        .lines()
        .find(|line| line.to_ascii_lowercase().contains("real max sectors"))
        .ok_or_else(|| "hdparm output did not contain DCO real max sectors".to_string())?;
    let (_, value) = line
        .split_once(':')
        .ok_or_else(|| "hdparm DCO sector line did not contain ':'".to_string())?;
    let sectors = decimal_prefix(value)?;
    if sectors > (1_u64 << 48) {
        return Err("hdparm reported an impossible DCO sector count".to_string());
    }
    Ok(sectors)
}

fn evaluate_ata_hidden_area_results(
    hpa_output: Result<String, String>,
    dco_output: Result<String, String>,
) -> Vec<PreflightCheck> {
    let hpa_counts = hpa_output.and_then(|output| parse_hpa_sector_counts(&output));
    let (hpa_check, native_sectors) = match hpa_counts {
        Ok((visible, native)) if visible == native => (
            check(
                "hpa",
                true,
                "No Host Protected Area is active.",
                String::new(),
            ),
            Some(native),
        ),
        Ok((visible, native)) if visible < native => (
            check(
                "hpa",
                false,
                "No Host Protected Area is active.",
                format!(
                    "Host Protected Area detected: only {visible} of {native} sectors are visible. Restore full capacity through a separate recovery workflow before wiping."
                ),
            ),
            Some(native),
        ),
        Ok((visible, native)) => (
            check(
                "hpa",
                false,
                "No Host Protected Area is active.",
                format!("HPA sector counts are inconsistent: visible {visible}, native {native}."),
            ),
            None,
        ),
        Err(error) => (
            check(
                "hpa",
                false,
                "No Host Protected Area is active.",
                format!("Could not verify HPA state: {error}."),
            ),
            None,
        ),
    };

    let dco_check = match (
        dco_output.and_then(|output| parse_dco_real_max_sectors(&output)),
        native_sectors,
    ) {
        (Ok(real), Some(native)) if real == native => check(
            "dco",
            true,
            "No Device Configuration Overlay hides drive capacity.",
            String::new(),
        ),
        (Ok(real), Some(native)) if real > native => check(
            "dco",
            false,
            "No Device Configuration Overlay hides drive capacity.",
            format!(
                "Device Configuration Overlay detected: the drive reports {real} real sectors but only {native} native sectors are exposed. Restore full capacity through a separate recovery workflow before wiping."
            ),
        ),
        (Ok(real), Some(native)) => check(
            "dco",
            false,
            "No Device Configuration Overlay hides drive capacity.",
            format!("DCO sector counts are inconsistent: real {real}, native {native}."),
        ),
        (Ok(_), None) => check(
            "dco",
            false,
            "No Device Configuration Overlay hides drive capacity.",
            "Could not compare DCO capacity because the HPA/native capacity check failed."
                .to_string(),
        ),
        (Err(error), _) => check(
            "dco",
            false,
            "No Device Configuration Overlay hides drive capacity.",
            format!("Could not verify DCO state: {error}."),
        ),
    };

    vec![hpa_check, dco_check]
}

pub fn evaluate_ata_hidden_areas(hpa_output: &str, dco_output: &str) -> Vec<PreflightCheck> {
    evaluate_ata_hidden_area_results(Ok(hpa_output.to_string()), Ok(dco_output.to_string()))
}

fn run_hdparm(args: &[&str]) -> Result<String, String> {
    let output = Command::new("hdparm")
        .args(args)
        .output()
        .map_err(|error| format!("hdparm could not start: {error}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!(
            "hdparm exited with {}: {}",
            output.status,
            text.trim()
        ));
    }
    Ok(text)
}

fn is_ata_drive(drive: &Drive) -> bool {
    matches!(drive.transport.as_str(), "ata" | "ide" | "sata")
}

fn probe_ata_hidden_areas(drive: &Drive) -> Vec<PreflightCheck> {
    evaluate_ata_hidden_area_results(
        run_hdparm(&["-N", &drive.name]),
        run_hdparm(&["--dco-identify", &drive.name]),
    )
}

fn probe_ata_security(method: &str, drive: &Drive) -> Vec<PreflightCheck> {
    let Some(mode) = AtaEraseMode::from_method_id(method) else {
        return Vec::new();
    };
    if drive.drive_type != DriveType::Ssd || !is_ata_drive(drive) {
        return Vec::new();
    }

    vec![match ata::probe_security_capabilities(&drive.name) {
        Ok(capabilities) => check(
            "ata_security_capability",
            capabilities.supports(mode),
            "The drive advertises the requested ATA Security Erase capability.",
            format!(
                "The drive does not advertise support for {}.",
                mode.display_name()
            ),
        ),
        Err(error) => check(
            "ata_security_capability",
            false,
            "The drive advertises the requested ATA Security Erase capability.",
            format!("Could not verify ATA Security Erase capabilities: {error}."),
        ),
    }]
}

fn probe_nvme_sanitize(method: &str, drive: &Drive) -> Vec<PreflightCheck> {
    let Some(action) = NvmeSanitizeAction::from_method_id(method) else {
        return Vec::new();
    };
    if drive.drive_type != DriveType::Nvme {
        return Vec::new();
    }

    let capability = nvme::probe_sanitize_capabilities(&drive.name);
    vec![match capability {
        Ok(capabilities) => check(
            "nvme_sanitize_capability",
            capabilities.supports(action),
            "The NVMe controller advertises the requested sanitize capability.",
            format!(
                "The NVMe controller does not advertise support for {}.",
                action.display_name()
            ),
        ),
        Err(error) => check(
            "nvme_sanitize_capability",
            false,
            "The NVMe controller advertises the requested sanitize capability.",
            format!("Could not verify NVMe sanitize capabilities: {error}."),
        ),
    }]
}

fn build_wipe_plan(config: &WipeConfig, require_identity: bool) -> Result<WipePlan, String> {
    if config.device_type.eq_ignore_ascii_case("android") {
        let identifier = if config.device_serial.is_empty() {
            config.device_path.as_str()
        } else {
            config.device_serial.as_str()
        };
        let devices = detect_android_devices()
            .map_err(|e| format!("could not verify Android device status: {e}"))?;
        return Ok(
            match devices.iter().find(|device| device.serial == identifier) {
                Some(device) => {
                    evaluate_mobile_wipe_with_identity(config, device, require_identity)
                }
                None => missing_device_plan(config, identifier),
            },
        );
    }

    let drives =
        detect_storage_drives().map_err(|e| format!("could not verify drive status: {e}"))?;
    Ok(
        match drives.iter().find(|drive| drive.name == config.device_path) {
            Some(drive) => {
                let mut plan = evaluate_storage_wipe_with_identity(config, drive, require_identity);
                if is_ata_drive(drive) {
                    plan.add_checks(probe_ata_hidden_areas(drive));
                    plan.add_checks(probe_ata_security(&config.method, drive));
                }
                plan.add_checks(probe_nvme_sanitize(&config.method, drive));
                plan
            }
            None => missing_device_plan(config, &config.device_path),
        },
    )
}

pub fn preflight_wipe(config: &WipeConfig) -> Result<WipePlan, String> {
    build_wipe_plan(config, false)
}

pub fn authorize_wipe(config: &WipeConfig) -> Result<WipePlan, String> {
    build_wipe_plan(config, true)
}
