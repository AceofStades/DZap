use serde_json::Value;
use std::process::Command;

const SANICAP_CRYPTO_ERASE: u32 = 1 << 0;
const SANICAP_BLOCK_ERASE: u32 = 1 << 1;
const SANICAP_OVERWRITE: u32 = 1 << 2;
const SANICAP_PURGE_REPORTING: u32 = 1 << 5;
const SANITIZE_STATUS_MASK: u16 = 0x7;
const SANITIZE_STATUS_COMPLETE: u16 = 0x1;
const SANITIZE_PROGRESS_COMPLETE: u16 = 0xffff;
const SANITIZE_PURGED: u16 = 1 << 11;
const SANITIZE_PREQ: u32 = 1 << 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvmeSanitizeAction {
    CryptoErase,
    BlockErase,
    Overwrite,
}

impl NvmeSanitizeAction {
    pub fn from_method_id(method: &str) -> Option<Self> {
        match method {
            "nvme_sanitize_crypto" => Some(Self::CryptoErase),
            "nvme_sanitize_block" => Some(Self::BlockErase),
            "nvme_sanitize_overwrite" => Some(Self::Overwrite),
            _ => None,
        }
    }

    pub fn method_id(self) -> &'static str {
        match self {
            Self::CryptoErase => "nvme_sanitize_crypto",
            Self::BlockErase => "nvme_sanitize_block",
            Self::Overwrite => "nvme_sanitize_overwrite",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::CryptoErase => "Purge: NVMe Sanitize (Crypto Erase)",
            Self::BlockErase => "Purge: NVMe Sanitize (Block Erase)",
            Self::Overwrite => "Purge: NVMe Sanitize (Overwrite)",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::CryptoErase => {
                "Uses the controller's advertised NVMe cryptographic erase sanitize action."
            }
            Self::BlockErase => {
                "Uses the controller's advertised NVMe block erase sanitize action."
            }
            Self::Overwrite => "Uses the controller's advertised NVMe overwrite sanitize action.",
        }
    }

    pub fn sanitize_action(self) -> u8 {
        match self {
            Self::BlockErase => 0x02,
            Self::Overwrite => 0x03,
            Self::CryptoErase => 0x04,
        }
    }

    fn capability_bit(self) -> u32 {
        match self {
            Self::CryptoErase => SANICAP_CRYPTO_ERASE,
            Self::BlockErase => SANICAP_BLOCK_ERASE,
            Self::Overwrite => SANICAP_OVERWRITE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NvmeSanitizeCapabilities {
    raw: u32,
}

impl NvmeSanitizeCapabilities {
    pub fn from_json(output: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(output)
            .map_err(|error| format!("could not parse NVMe controller JSON: {error}"))?;
        let sanicap = value
            .get("sanicap")
            .ok_or_else(|| "NVMe controller JSON did not contain sanicap".to_string())?;
        let raw = match sanicap {
            Value::Number(number) => number
                .as_u64()
                .ok_or_else(|| "NVMe sanicap was not an unsigned integer".to_string())?,
            Value::String(value) => parse_u32(value)? as u64,
            _ => return Err("NVMe sanicap was not a number or string".to_string()),
        };
        let raw = u32::try_from(raw)
            .map_err(|_| "NVMe sanicap exceeded the 32-bit field width".to_string())?;
        Ok(Self { raw })
    }

    pub fn supports(self, action: NvmeSanitizeAction) -> bool {
        self.raw & action.capability_bit() != 0
    }

    pub fn supports_purge_reporting(self) -> bool {
        self.raw & SANICAP_PURGE_REPORTING != 0
    }
}

fn parse_u32(value: &str) -> Result<u32, String> {
    let value = value.trim();
    let parsed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse::<u32>(), |hex| u32::from_str_radix(hex, 16))
        .map_err(|error| format!("invalid NVMe sanicap value {value:?}: {error}"))?;
    Ok(parsed)
}

pub fn controller_path(device_path: &str) -> Result<String, String> {
    let name = device_path.strip_prefix("/dev/").unwrap_or(device_path);
    if !name.starts_with("nvme") {
        return Err(format!("{device_path} is not an NVMe device path"));
    }

    let controller = if let Some((namespace_prefix, namespace_id)) = name.rsplit_once('n') {
        if !namespace_id.is_empty()
            && namespace_id.bytes().all(|byte| byte.is_ascii_digit())
            && namespace_prefix[4..]
                .bytes()
                .any(|byte| byte.is_ascii_digit())
        {
            namespace_prefix
                .split_once('c')
                .map_or(namespace_prefix, |(controller, _)| controller)
        } else {
            name
        }
    } else {
        name
    };

    if controller.len() == 4 || !controller[4..].bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!(
            "could not derive an NVMe controller from {device_path}"
        ));
    }
    Ok(format!("/dev/{controller}"))
}

pub fn probe_sanitize_capabilities(device_path: &str) -> Result<NvmeSanitizeCapabilities, String> {
    let controller = controller_path(device_path)?;
    let output = Command::new("nvme")
        .args(["id-ctrl", &controller, "-o", "json"])
        .output()
        .map_err(|error| format!("nvme id-ctrl could not start: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "nvme id-ctrl failed with {}: {}",
            output.status,
            stderr.trim()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("nvme id-ctrl returned non-UTF-8 JSON: {error}"))?;
    NvmeSanitizeCapabilities::from_json(&stdout)
}

pub fn sanitize_arguments(
    device_path: &str,
    action: NvmeSanitizeAction,
    capabilities: NvmeSanitizeCapabilities,
) -> Result<Vec<String>, String> {
    if !capabilities.supports(action) {
        return Err(format!(
            "the NVMe controller does not advertise {} support",
            action.display_name()
        ));
    }
    let mut arguments = vec![
        "sanitize".to_string(),
        controller_path(device_path)?,
        format!("--sanact=0x{:02x}", action.sanitize_action()),
        "--wait".to_string(),
    ];
    if capabilities.supports_purge_reporting() {
        arguments.push("--preq".to_string());
    }
    Ok(arguments)
}

pub fn validate_sanitize_log(
    raw: &[u8],
    action: NvmeSanitizeAction,
    require_purge: bool,
) -> Result<(), String> {
    if raw.len() < 8 {
        return Err(format!(
            "NVMe sanitize status log was only {} bytes",
            raw.len()
        ));
    }
    let status = u16::from_le_bytes([raw[2], raw[3]]);
    let status_code = status & SANITIZE_STATUS_MASK;
    if status_code != SANITIZE_STATUS_COMPLETE {
        return Err(format!(
            "NVMe sanitize status was {status_code}, expected successful completion"
        ));
    }
    let progress = u16::from_le_bytes([raw[0], raw[1]]);
    if progress != SANITIZE_PROGRESS_COMPLETE {
        return Err(format!(
            "NVMe sanitize completion log reported progress 0x{progress:04x}"
        ));
    }
    let command = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
    if command & 0x7 != u32::from(action.sanitize_action()) {
        return Err(format!(
            "NVMe sanitize log action {} does not match requested action {}",
            command & 0x7,
            action.sanitize_action()
        ));
    }
    if require_purge && (command & SANITIZE_PREQ == 0 || status & SANITIZE_PURGED == 0) {
        return Err(
            "NVMe controller did not report successful purge-request completion".to_string(),
        );
    }
    Ok(())
}
