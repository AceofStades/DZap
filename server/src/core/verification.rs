use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::Command;

use super::drives::detect_storage_drives;
use super::nvme::{self, NvmeSanitizeAction};
use super::wiper::WipeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStrategy {
    FullPatternReadback,
    AtaSecurityStatusAndSamples,
    NvmeFormatStatusAndSamples,
    NvmeSanitizeStatusAndSamples,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub strategy: VerificationStrategy,
    pub bytes_checked: u64,
    pub readback_sha256: String,
    pub expected_pattern: Option<String>,
    pub firmware_status_sha256: Option<String>,
    pub identity_revalidated: bool,
}

impl VerificationResult {
    pub fn validate_for_job(&self, method: &str, expected_size: u64) -> Result<(), String> {
        if expected_size == 0 {
            return Err("verification cannot cover a zero-length device".to_string());
        }
        if !self.identity_revalidated {
            return Err("verification did not revalidate the device identity".to_string());
        }
        if !is_sha256(&self.readback_sha256) {
            return Err("verification readback hash is not a SHA-256 digest".to_string());
        }

        match method {
            "overwrite_1_pass" | "overwrite_2_pass" | "overwrite_3_pass" => {
                let expected_pattern = match method {
                    "overwrite_1_pass" => "0x00",
                    "overwrite_2_pass" => "0xAA",
                    _ => "0x55",
                };
                if self.strategy != VerificationStrategy::FullPatternReadback
                    || self.bytes_checked != expected_size
                    || self.expected_pattern.as_deref() != Some(expected_pattern)
                    || self.firmware_status_sha256.is_some()
                {
                    return Err(format!(
                        "verification evidence does not match {method} over {expected_size} bytes"
                    ));
                }
            }
            "sata_secure_erase"
            | "sata_secure_erase_enhanced"
            | "nvme_format"
            | "nvme_sanitize_crypto"
            | "nvme_sanitize_block"
            | "nvme_sanitize_overwrite" => {
                let expected_strategy =
                    if matches!(method, "sata_secure_erase" | "sata_secure_erase_enhanced") {
                        VerificationStrategy::AtaSecurityStatusAndSamples
                    } else if method == "nvme_format" {
                        VerificationStrategy::NvmeFormatStatusAndSamples
                    } else {
                        VerificationStrategy::NvmeSanitizeStatusAndSamples
                    };
                let sample_size = expected_size.min(64 * 1024);
                let final_offset = expected_size - sample_size;
                let sample_count = match final_offset {
                    0 => 1,
                    1 => 2,
                    _ => 3,
                };
                let expected_sample_bytes = sample_size * sample_count;
                if self.strategy != expected_strategy
                    || self.bytes_checked != expected_sample_bytes
                    || self.expected_pattern.is_some()
                    || self
                        .firmware_status_sha256
                        .as_deref()
                        .is_none_or(|hash| !is_sha256(hash))
                {
                    return Err(format!(
                        "verification evidence does not match firmware method {method}"
                    ));
                }
            }
            _ => return Err(format!("no verification policy exists for method {method}")),
        }

        Ok(())
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn verify_wipe(config: &WipeConfig) -> Result<VerificationResult, String> {
    if config.device_type.eq_ignore_ascii_case("android") {
        return Err(
            "Android factory reset verification is unavailable; no certificate can be issued"
                .to_string(),
        );
    }

    let expected_identity = config
        .expected_identity
        .as_ref()
        .ok_or_else(|| "wipe verification requires the approved device identity".to_string())?;
    let drives = detect_storage_drives()
        .map_err(|error| format!("could not re-detect device for verification: {error}"))?;
    let drive = drives
        .iter()
        .find(|drive| drive.name == config.device_path)
        .ok_or_else(|| "device disappeared before wipe verification".to_string())?;
    if drive.identity() != *expected_identity {
        return Err("device identity changed before wipe verification".to_string());
    }
    let expected_size = expected_identity
        .size_bytes
        .parse::<u64>()
        .map_err(|error| format!("approved device size is invalid: {error}"))?;
    if expected_size == 0 {
        return Err("cannot verify a zero-length device".to_string());
    }

    let mut result = if let Some(action) = NvmeSanitizeAction::from_method_id(&config.method) {
        verify_nvme_sanitize(&config.device_path, expected_size, action)
    } else {
        match config.method.as_str() {
            "overwrite_1_pass" => verify_pattern_file(&config.device_path, expected_size, 0x00),
            "overwrite_2_pass" => verify_pattern_file(&config.device_path, expected_size, 0xAA),
            "overwrite_3_pass" => verify_pattern_file(&config.device_path, expected_size, 0x55),
            "sata_secure_erase" | "sata_secure_erase_enhanced" => {
                verify_ata_erase(&config.device_path, expected_size)
            }
            "nvme_format" => verify_nvme_format(&config.device_path, expected_size),
            method => Err(format!(
                "no verification strategy exists for method {method}"
            )),
        }
    }?;
    result.identity_revalidated = true;
    Ok(result)
}

pub fn verify_pattern_file(
    path: impl AsRef<Path>,
    expected_size: u64,
    expected_byte: u8,
) -> Result<VerificationResult, String> {
    let path = path.as_ref();
    let mut file = File::open(path).map_err(|error| {
        format!(
            "failed to open {} for verification: {error}",
            path.display()
        )
    })?;
    invalidate_block_cache(&file, path)?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut checked = 0_u64;
    while checked < expected_size {
        let wanted = (expected_size - checked).min(buffer.len() as u64) as usize;
        let count = file.read(&mut buffer[..wanted]).map_err(|error| {
            format!(
                "failed to read {} during verification: {error}",
                path.display()
            )
        })?;
        if count == 0 {
            return Err(format!(
                "verification reached end of device after {checked} of {expected_size} bytes"
            ));
        }
        if let Some(index) = buffer[..count]
            .iter()
            .position(|byte| *byte != expected_byte)
        {
            return Err(format!(
                "verification mismatch at byte {}: expected 0x{expected_byte:02X}, found 0x{:02X}",
                checked + index as u64,
                buffer[index]
            ));
        }
        hasher.update(&buffer[..count]);
        checked += count as u64;
    }

    let mut extra = [0_u8; 1];
    if file
        .read(&mut extra)
        .map_err(|error| format!("failed to confirm device extent: {error}"))?
        != 0
    {
        return Err(format!(
            "device is larger than its approved {expected_size}-byte identity"
        ));
    }

    Ok(VerificationResult {
        strategy: VerificationStrategy::FullPatternReadback,
        bytes_checked: checked,
        readback_sha256: hex::encode(hasher.finalize()),
        expected_pattern: Some(format!("0x{expected_byte:02X}")),
        firmware_status_sha256: None,
        identity_revalidated: false,
    })
}

fn verify_ata_erase(path: &str, expected_size: u64) -> Result<VerificationResult, String> {
    let output = command_output("hdparm", &["-I", path])?;
    if !ata_security_is_disabled(&output) {
        return Err("ATA security state is still enabled or locked after secure erase".to_string());
    }
    let status_hash = hex::encode(Sha256::digest(output.as_bytes()));
    let (bytes_checked, readback_sha256) = sample_readback(path, expected_size)?;
    Ok(VerificationResult {
        strategy: VerificationStrategy::AtaSecurityStatusAndSamples,
        bytes_checked,
        readback_sha256,
        expected_pattern: None,
        firmware_status_sha256: Some(status_hash),
        identity_revalidated: false,
    })
}

fn verify_nvme_format(path: &str, expected_size: u64) -> Result<VerificationResult, String> {
    let identify = command_output("nvme", &["id-ns", path, "-H"])?;
    let health = command_output("nvme", &["smart-log", path])?;
    let status_hash = hex::encode(Sha256::digest(format!("{identify}\n{health}").as_bytes()));
    let (bytes_checked, readback_sha256) = sample_readback(path, expected_size)?;
    Ok(VerificationResult {
        strategy: VerificationStrategy::NvmeFormatStatusAndSamples,
        bytes_checked,
        readback_sha256,
        expected_pattern: None,
        firmware_status_sha256: Some(status_hash),
        identity_revalidated: false,
    })
}

fn verify_nvme_sanitize(
    path: &str,
    expected_size: u64,
    action: NvmeSanitizeAction,
) -> Result<VerificationResult, String> {
    let capabilities = nvme::probe_sanitize_capabilities(path)?;
    if !capabilities.supports(action) {
        return Err(format!(
            "the NVMe controller no longer advertises {} support",
            action.display_name()
        ));
    }
    let controller = nvme::controller_path(path)?;
    let sanitize_log =
        command_output_bytes("nvme", &["sanitize-log", &controller, "--raw-binary"])?;
    nvme::validate_sanitize_log(
        &sanitize_log,
        action,
        capabilities.supports_purge_reporting(),
    )?;
    let identify = command_output("nvme", &["id-ns", path, "-H"])?;
    let health = command_output("nvme", &["smart-log", path])?;
    let mut status_hasher = Sha256::new();
    status_hasher.update(b"sanitize-log\0");
    status_hasher.update(&sanitize_log);
    status_hasher.update(b"id-ns\0");
    status_hasher.update(identify.as_bytes());
    status_hasher.update(b"smart-log\0");
    status_hasher.update(health.as_bytes());
    let (bytes_checked, readback_sha256) = sample_readback(path, expected_size)?;
    Ok(VerificationResult {
        strategy: VerificationStrategy::NvmeSanitizeStatusAndSamples,
        bytes_checked,
        readback_sha256,
        expected_pattern: None,
        firmware_status_sha256: Some(hex::encode(status_hasher.finalize())),
        identity_revalidated: false,
    })
}

fn command_output(command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|error| format!("{command} could not start during verification: {error}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!(
            "{command} verification command failed with {}: {}",
            output.status,
            text.trim()
        ));
    }
    Ok(text)
}

fn command_output_bytes(command: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|error| format!("{command} could not start during verification: {error}"))?;
    if !output.status.success() {
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        return Err(format!(
            "{command} verification command failed with {}: {}",
            output.status,
            text.trim()
        ));
    }
    Ok(output.stdout)
}

pub fn ata_security_is_disabled(output: &str) -> bool {
    let mut in_security_section = false;
    let mut disabled = false;
    let mut unlocked = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "Security:" {
            in_security_section = true;
            continue;
        }
        if !in_security_section {
            continue;
        }
        if !line.starts_with(char::is_whitespace) && !trimmed.is_empty() {
            break;
        }
        match trimmed {
            "not enabled" => disabled = true,
            "not locked" => unlocked = true,
            "enabled" => disabled = false,
            "locked" => unlocked = false,
            _ => {}
        }
    }
    in_security_section && disabled && unlocked
}

fn sample_readback(path: &str, expected_size: u64) -> Result<(u64, String), String> {
    let mut file = File::open(path)
        .map_err(|error| format!("failed to open {path} for verification: {error}"))?;
    invalidate_block_cache(&file, Path::new(path))?;
    let sample_size = expected_size.min(64 * 1024);
    let mut offsets = vec![0, expected_size.saturating_sub(sample_size) / 2];
    offsets.push(expected_size - sample_size);
    offsets.sort_unstable();
    offsets.dedup();

    let mut hasher = Sha256::new();
    let mut checked = 0_u64;
    let mut buffer = vec![0_u8; sample_size as usize];
    for offset in offsets {
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| format!("failed to seek during verification: {error}"))?;
        file.read_exact(&mut buffer)
            .map_err(|error| format!("failed to read verification sample at {offset}: {error}"))?;
        hasher.update(offset.to_le_bytes());
        hasher.update(&buffer);
        checked += sample_size;
    }
    Ok((checked, hex::encode(hasher.finalize())))
}

#[cfg(target_os = "linux")]
fn invalidate_block_cache(file: &File, path: &Path) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::FileTypeExt;

    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_block_device() {
        // Linux BLKFLSBUF: flush dirty buffers and invalidate cached block data.
        let result = unsafe { libc::ioctl(file.as_raw_fd(), 0x1261) };
        if result != 0 {
            return Err(format!(
                "failed to flush block cache for {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn invalidate_block_cache(_file: &File, _path: &Path) -> Result<(), String> {
    Ok(())
}
