use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtaEraseMode {
    Normal,
    Enhanced,
}

impl AtaEraseMode {
    pub fn from_method_id(method: &str) -> Option<Self> {
        match method {
            "sata_secure_erase" => Some(Self::Normal),
            "sata_secure_erase_enhanced" => Some(Self::Enhanced),
            _ => None,
        }
    }

    pub fn method_id(self) -> &'static str {
        match self {
            Self::Normal => "sata_secure_erase",
            Self::Enhanced => "sata_secure_erase_enhanced",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Normal => "Purge: ATA Secure Erase",
            Self::Enhanced => "Purge: ATA Enhanced Secure Erase",
        }
    }

    pub fn erase_option(self) -> &'static str {
        match self {
            Self::Normal => "--security-erase",
            Self::Enhanced => "--security-erase-enhanced",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtaSecurityCapabilities {
    pub security_supported: bool,
    pub enhanced_erase_supported: bool,
}

impl AtaSecurityCapabilities {
    pub fn from_hdparm_output(output: &str) -> Result<Self, String> {
        let mut in_security_section = false;
        let mut security_supported = false;
        let mut enhanced_erase_supported = false;
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
                "supported" => security_supported = true,
                "not supported" => security_supported = false,
                "supported: enhanced erase" => enhanced_erase_supported = true,
                _ => {}
            }
        }
        if !in_security_section {
            return Err("hdparm output did not contain an ATA Security section".to_string());
        }
        Ok(Self {
            security_supported,
            enhanced_erase_supported,
        })
    }

    pub fn supports(self, mode: AtaEraseMode) -> bool {
        self.security_supported && (mode == AtaEraseMode::Normal || self.enhanced_erase_supported)
    }
}

pub fn probe_security_capabilities(path: &str) -> Result<AtaSecurityCapabilities, String> {
    let output = Command::new("hdparm")
        .args(["-I", path])
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
    AtaSecurityCapabilities::from_hdparm_output(&text)
}

pub fn set_password_arguments(path: &str) -> Vec<String> {
    vec![
        "--user-master".to_string(),
        "u".to_string(),
        "--security-set-pass".to_string(),
        "dZap".to_string(),
        path.to_string(),
    ]
}

pub fn erase_arguments(path: &str, mode: AtaEraseMode) -> Vec<String> {
    vec![
        "--user-master".to_string(),
        "u".to_string(),
        mode.erase_option().to_string(),
        "dZap".to_string(),
        path.to_string(),
    ]
}

pub fn disable_password_arguments(path: &str) -> Vec<String> {
    vec![
        "--user-master".to_string(),
        "u".to_string(),
        "--security-disable".to_string(),
        "dZap".to_string(),
        path.to_string(),
    ]
}
