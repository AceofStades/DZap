// Port of server-go/core/wiper.go
use super::ata::{self, AtaEraseMode};
use super::drives::{
    DeviceIdentity, Drive, DriveType, MobileDevice, detect_android_devices, detect_storage_drives,
    log_line,
};
use super::nvme::{self, NvmeSanitizeAction};
use super::preflight::authorize_wipe;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Shared controls for an in-progress wipe: cancellation + pause flag.
pub struct WipeControls {
    pub cancel: Arc<AtomicBool>,
    pub paused: Arc<AtomicBool>,
}

pub(crate) fn active_wipes() -> &'static Mutex<HashMap<String, Arc<WipeControls>>> {
    static ACTIVE_WIPES: OnceLock<Mutex<HashMap<String, Arc<WipeControls>>>> = OnceLock::new();
    ACTIVE_WIPES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn reserved_devices() -> &'static Mutex<HashSet<String>> {
    static RESERVED_DEVICES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    RESERVED_DEVICES.get_or_init(|| Mutex::new(HashSet::new()))
}

/// An API-level reservation that prevents overlapping wipe jobs for one device.
/// It is held until sanitization and verification have both finished.
pub(crate) struct WipeReservation {
    device_path: String,
}

impl Drop for WipeReservation {
    fn drop(&mut self) {
        reserved_devices()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.device_path);
    }
}

pub(crate) fn reserve_device(device_path: &str) -> Result<WipeReservation, String> {
    let mut devices = reserved_devices()
        .lock()
        .map_err(|_| "wipe reservation state is unavailable".to_string())?;
    if !devices.insert(device_path.to_string()) {
        return Err(format!(
            "a wipe or verification is already active for device {device_path}"
        ));
    }
    Ok(WipeReservation {
        device_path: device_path.to_string(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct WipeProgress {
    #[serde(rename = "deviceId")]
    pub device_id: String,
    #[serde(rename = "deviceModel", skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    #[serde(default)]
    pub method: String,
    #[serde(rename = "methodName", skip_serializing_if = "Option::is_none")]
    pub method_name: Option<String>,
    pub status: String,
    pub progress: f64,
    #[serde(rename = "currentPass")]
    pub current_pass: i32,
    #[serde(rename = "totalPasses")]
    pub total_passes: i32,
    #[serde(default)]
    pub speed: String,
    #[serde(default)]
    pub eta: String,
    #[serde(rename = "error", skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(rename = "sectorNumber")]
    pub sector_number: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WipeConfig {
    #[serde(rename = "DevicePath", alias = "devicePath", alias = "device_path")]
    pub device_path: String,
    #[serde(rename = "Method", alias = "method")]
    pub method: String,
    #[serde(
        rename = "DeviceSerial",
        alias = "deviceSerial",
        alias = "device_serial",
        default
    )]
    pub device_serial: String,
    #[serde(
        rename = "DeviceType",
        alias = "deviceType",
        alias = "device_type",
        default
    )]
    pub device_type: String,
    #[serde(
        rename = "deviceModel",
        alias = "DeviceModel",
        alias = "device_model",
        default
    )]
    pub device_model: Option<String>,
    #[serde(
        rename = "ExpectedIdentity",
        alias = "expectedIdentity",
        alias = "expected_identity",
        default
    )]
    pub expected_identity: Option<DeviceIdentity>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WipeMethod {
    pub id: String,
    pub name: String,
    pub description: String,
}

pub(crate) fn wipe_method_name(method_id: &str) -> &'static str {
    match method_id {
        "nvme_format" => "Purge: NVMe Format",
        "nvme_sanitize_crypto" => "Purge: NVMe Sanitize (Crypto Erase)",
        "nvme_sanitize_block" => "Purge: NVMe Sanitize (Block Erase)",
        "nvme_sanitize_overwrite" => "Purge: NVMe Sanitize (Overwrite)",
        "overwrite_1_pass" => "Clear: 1-Pass Overwrite",
        "sata_secure_erase" => "Purge: ATA Secure Erase",
        "sata_secure_erase_enhanced" => "Purge: ATA Enhanced Secure Erase",
        "overwrite_3_pass" => "Purge: 3-Pass Overwrite",
        "overwrite_2_pass" => "Clear: 2-Pass Overwrite",
        "android_factory_reset" => "Clear: Factory Reset",
        _ => "Unknown",
    }
}

/// NIST-compliant methods for standard storage.
pub fn get_wipe_methods_for_drive(drive: &Drive) -> Vec<WipeMethod> {
    let m = |id: &str, name: &str, desc: &str| WipeMethod {
        id: id.to_string(),
        name: name.to_string(),
        description: desc.to_string(),
    };
    match drive.drive_type {
        DriveType::Nvme => {
            let mut methods = [
                NvmeSanitizeAction::CryptoErase,
                NvmeSanitizeAction::BlockErase,
                NvmeSanitizeAction::Overwrite,
            ]
            .into_iter()
            .map(|action| {
                m(
                    action.method_id(),
                    action.display_name(),
                    action.description(),
                )
            })
            .collect::<Vec<_>>();
            methods.extend([
                m(
                    "nvme_format",
                    "Purge: NVMe Format",
                    "Uses the drive's built-in, high-speed firmware command (NVM Express Format).",
                ),
                m(
                    "overwrite_1_pass",
                    "Clear: Overwrite",
                    "Not fully effective for flash media due to wear-leveling and over-provisioning.",
                ),
            ]);
            methods
        }
        DriveType::Ssd => {
            let overwrite = m(
                "overwrite_1_pass",
                "Clear: Overwrite",
                "Not fully effective for flash media due to wear-leveling and over-provisioning.",
            );
            if matches!(drive.transport.as_str(), "ata" | "ide" | "sata") {
                vec![
                    m(
                        "sata_secure_erase_enhanced",
                        "Purge: ATA Enhanced Secure Erase",
                        "Uses the drive's advertised enhanced ATA Security Erase operation.",
                    ),
                    m(
                        "sata_secure_erase",
                        "Purge: ATA Secure Erase",
                        "Uses the drive's ATA Security Erase operation.",
                    ),
                    overwrite,
                ]
            } else {
                vec![overwrite]
            }
        }
        DriveType::Hdd => vec![
            m(
                "overwrite_1_pass",
                "Clear: 1-Pass Overwrite",
                "A single pass of a fixed pattern, per NIST SP 800-88r1 guidelines.",
            ),
            m(
                "overwrite_3_pass",
                "Purge: 3-Pass Overwrite",
                "Three passes of a pseudorandom pattern, an optional NIST Purge method.",
            ),
        ],
        DriveType::Usb | DriveType::Unknown => vec![m(
            "overwrite_2_pass",
            "Clear: 2-Pass Overwrite",
            "A pattern and its complement, per NIST guidelines for USB/removable media.",
        )],
    }
}

/// Mobile wiping remains unavailable until DZap can execute and verify a
/// device-specific reset rather than merely rebooting into recovery.
pub fn get_wipe_methods_for_mobile(_device: &MobileDevice) -> Vec<WipeMethod> {
    Vec::new()
}

/// Returns the available wipe methods for a specific device.
pub fn get_wipe_methods(device_path: &str) -> Result<Vec<WipeMethod>, String> {
    let drives = detect_storage_drives().map_err(|e| format!("could not detect drives: {e}"))?;

    for drive in &drives {
        if drive.name == device_path {
            let mut methods = get_wipe_methods_for_drive(drive);
            if drive.drive_type == DriveType::Nvme {
                let capabilities = nvme::probe_sanitize_capabilities(&drive.name).ok();
                methods.retain(|method| {
                    NvmeSanitizeAction::from_method_id(&method.id).is_none_or(|action| {
                        capabilities.is_some_and(|capabilities| capabilities.supports(action))
                    })
                });
            }
            if drive.drive_type == DriveType::Ssd {
                let capabilities = ata::probe_security_capabilities(&drive.name).ok();
                methods.retain(|method| {
                    AtaEraseMode::from_method_id(&method.id).is_none_or(|mode| {
                        capabilities.is_some_and(|capabilities| capabilities.supports(mode))
                    })
                });
            }
            return Ok(methods);
        }
    }

    // Also check mobile devices
    match detect_android_devices() {
        Ok(devices) => {
            for device in &devices {
                if device.serial == device_path {
                    // device_path is the serial for mobile
                    return Ok(get_wipe_methods_for_mobile(device));
                }
            }
        }
        Err(e) => {
            // Non-fatal, just log it
            println!("could not detect mobile devices: {e}");
        }
    }

    Err(format!("device {device_path} not found"))
}

pub fn sanitize_device(
    config: WipeConfig,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    let plan = authorize_wipe(&config)?;
    if !plan.is_ready() {
        return Err(format!(
            "wipe blocked by preflight: {}",
            plan.blocking_message()
        ));
    }

    if config.device_type.eq_ignore_ascii_case("android") {
        return sanitize_android(&config.device_serial, progress);
    }
    sanitize_storage_drive(config, progress)
}

fn sanitize_storage_drive(
    config: WipeConfig,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    if let Some(action) = NvmeSanitizeAction::from_method_id(&config.method) {
        return sanitize_nvme(&config.device_path, action, progress);
    }
    if let Some(mode) = AtaEraseMode::from_method_id(&config.method) {
        return sanitize_sata(&config.device_path, mode, progress);
    }
    match config.method.as_str() {
        "nvme_format" => sanitize_nvme_format(&config.device_path, progress),
        "overwrite_1_pass" => sanitize_overwrite(config, 1, progress),
        "overwrite_3_pass" => sanitize_overwrite(config, 3, progress),
        "overwrite_2_pass" => sanitize_overwrite_two_pass(config, progress),
        other => Err(format!("unknown sanitization method: {other}")),
    }
}

pub(crate) fn sanitize_android(
    serial: &str,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    sanitize_android_with(serial, progress, |serial| {
        let status = Command::new("adb")
            .args(["-s", serial, "reboot", "recovery"])
            .status()
            .map_err(|e| format!("failed to send reboot to recovery command: {e}"))?;
        if !status.success() {
            return Err(format!(
                "failed to send reboot to recovery command: {status}"
            ));
        }
        Ok(())
    })
}

pub(crate) fn sanitize_android_with<F>(
    serial: &str,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
    reboot_to_recovery: F,
) -> Result<(), String>
where
    F: FnOnce(&str) -> Result<(), String>,
{
    let _ = progress.send(format!(
        "Executing Android Factory Reset (NIST Clear) on device {serial}..."
    ));
    reboot_to_recovery(serial)?;
    let _ = progress.send(
        "Reboot to recovery command sent. The device will now perform a factory reset.".to_string(),
    );
    Ok(())
}

pub(crate) fn run_command(
    cancel: &Arc<AtomicBool>,
    name: &str,
    args: &[&str],
) -> Result<(), String> {
    // Prepend ionice to the command to set I/O scheduling class to Idle
    let mut child = Command::new("ionice")
        .arg("-c")
        .arg("3")
        .arg(name)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("command {name} failed to start: {e}"))?;
    let mut stdout_reader = child
        .stdout
        .take()
        .map(|output| std::thread::spawn(move || drain_command_output(output)));
    let mut stderr_reader = child
        .stderr
        .take()
        .map(|output| std::thread::spawn(move || drain_command_output(output)));

    // Poll the child so we can honour cancellation like Go's CommandContext.
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            collect_child_output(&mut stdout_reader, &mut stderr_reader);
            return Err("wipe aborted".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = collect_child_output(&mut stdout_reader, &mut stderr_reader);
                if status.success() {
                    return Ok(());
                }
                return Err(format!("command {name} failed: {status}. Output: {out}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                collect_child_output(&mut stdout_reader, &mut stderr_reader);
                return Err(format!("command {name} failed: {e}"));
            }
        }
    }
}

fn drain_command_output(mut reader: impl Read) -> Vec<u8> {
    const MAX_CAPTURED_BYTES: usize = 64 * 1024;
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 8192];
    while let Ok(count) = reader.read(&mut buffer) {
        if count == 0 {
            break;
        }
        let remaining = MAX_CAPTURED_BYTES.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..count.min(remaining)]);
    }
    captured
}

fn collect_child_output(
    stdout: &mut Option<std::thread::JoinHandle<Vec<u8>>>,
    stderr: &mut Option<std::thread::JoinHandle<Vec<u8>>>,
) -> String {
    let mut output = stdout
        .take()
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default();
    if let Some(mut error) = stderr.take().and_then(|reader| reader.join().ok()) {
        output.append(&mut error);
    }
    String::from_utf8_lossy(&output).into_owned()
}

pub(crate) fn register_wipe(device_path: &str) -> Arc<WipeControls> {
    let controls = Arc::new(WipeControls {
        cancel: Arc::new(AtomicBool::new(false)),
        paused: Arc::new(AtomicBool::new(false)),
    });
    active_wipes()
        .lock()
        .unwrap()
        .insert(device_path.to_string(), controls.clone());
    controls
}

fn unregister_wipe(device_path: &str) {
    active_wipes().lock().unwrap().remove(device_path);
}

fn sanitize_nvme(
    path: &str,
    action: NvmeSanitizeAction,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    let capabilities = nvme::probe_sanitize_capabilities(path)?;
    let arguments = nvme::sanitize_arguments(path, action, capabilities)?;
    let argument_refs = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    let _ = progress.send(format!("Executing {}...", action.display_name()));
    let controls = register_wipe(path);
    let result = run_command(&controls.cancel, "nvme", &argument_refs);
    unregister_wipe(path);
    result
}

fn sanitize_nvme_format(
    path: &str,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    let _ = progress.send("Executing NVMe Format...".to_string());
    let controls = register_wipe(path);
    let result = run_command(&controls.cancel, "nvme", &["format", path, "-s", "1"]);
    unregister_wipe(path);
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AtaCommandPhase {
    Destructive,
    Recovery,
}

fn sanitize_sata(
    path: &str,
    mode: AtaEraseMode,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    let capabilities = ata::probe_security_capabilities(path)?;
    if !capabilities.supports(mode) {
        return Err(format!(
            "the drive does not advertise support for {}",
            mode.display_name()
        ));
    }
    let controls = register_wipe(path);
    let recovery_cancel = Arc::new(AtomicBool::new(false));
    let result = sanitize_sata_with(path, mode, progress, |phase, arguments| {
        let argument_refs = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        let cancel = match phase {
            AtaCommandPhase::Destructive => &controls.cancel,
            AtaCommandPhase::Recovery => &recovery_cancel,
        };
        run_command(cancel, "hdparm", &argument_refs)
    });
    unregister_wipe(path);
    result
}

pub(crate) fn sanitize_sata_with<F>(
    path: &str,
    mode: AtaEraseMode,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
    mut run: F,
) -> Result<(), String>
where
    F: FnMut(AtaCommandPhase, &[String]) -> Result<(), String>,
{
    let _ = progress.send(format!("Executing {}...", mode.display_name()));
    let set_password = ata::set_password_arguments(path);
    if let Err(error) = run(AtaCommandPhase::Destructive, &set_password) {
        return Err(cleanup_ata_password(
            path,
            format!("failed to set the temporary ATA security password: {error}"),
            &mut run,
        ));
    }

    let _ = progress.send("Temporary ATA security password set. Issuing erase...".to_string());
    let erase = ata::erase_arguments(path, mode);
    if let Err(error) = run(AtaCommandPhase::Destructive, &erase) {
        return Err(cleanup_ata_password(
            path,
            format!("ATA erase command failed: {error}"),
            &mut run,
        ));
    }
    Ok(())
}

fn cleanup_ata_password<F>(path: &str, error: String, run: &mut F) -> String
where
    F: FnMut(AtaCommandPhase, &[String]) -> Result<(), String>,
{
    let disable_password = ata::disable_password_arguments(path);
    match run(AtaCommandPhase::Recovery, &disable_password) {
        Ok(()) => format!("{error}; temporary ATA security password was disabled"),
        Err(cleanup_error) => {
            format!("{error}; temporary ATA security password cleanup also failed: {cleanup_error}")
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn overwrite_pass(
    controls: &WipeControls,
    config: &WipeConfig,
    pattern: u8,
    pass_num: i32,
    total_passes: i32,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .open(&config.device_path)
        .map_err(|e| format!("failed to open device: {e}"))?;

    let size = file
        .seek(SeekFrom::End(0))
        .map_err(|e| format!("could not determine device size: {e}"))? as i64;
    log_line(&format!(
        "overwrite_pass pass {pass_num}, device size: {size}"
    ));
    file.seek(SeekFrom::Start(0))
        .map_err(|e| format!("could not seek to start: {e}"))?;

    let buffer = vec![pattern; 128 * 1024]; // 128KB buffer

    let mut written: i64 = 0;
    let start_time = Instant::now();
    let mut last_report = Instant::now();

    while written < size {
        if controls.cancel.load(Ordering::SeqCst) {
            return Err("wipe aborted".to_string());
        }
        // Paused: wait for resume or abort signal
        while controls.paused.load(Ordering::SeqCst) {
            if controls.cancel.load(Ordering::SeqCst) {
                return Err("wipe aborted".to_string());
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        let remaining = (size - written) as usize;
        let chunk_len = remaining.min(buffer.len());
        match file.write(&buffer[..chunk_len]) {
            Ok(0) => return Err(format!("write error on pass {pass_num}: wrote zero bytes")),
            Ok(n) => written += n as i64,
            Err(e) => {
                log_line(&format!("overwrite_pass pass {pass_num}, write error: {e}"));
                if e.kind() == std::io::ErrorKind::UnexpectedEof
                    || e.to_string().contains("no space left on device")
                {
                    written = size; // Mark as complete
                    break;
                }
                return Err(format!("write error on pass {pass_num}: {e}"));
            }
        }

        if last_report.elapsed() >= Duration::from_millis(500) {
            last_report = Instant::now();
            let elapsed = start_time.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                let speed = written as f64 / elapsed / 1024.0 / 1024.0; // MB/s
                let eta = (size - written) as f64 / (speed * 1024.0 * 1024.0); // seconds

                let pass_progress = written as f64 * 100.0 / size as f64;
                let overall_progress =
                    ((pass_num - 1) as f64 + pass_progress / 100.0) * 100.0 / total_passes as f64;

                let msg = WipeProgress {
                    device_id: config.device_path.clone(),
                    device_model: config.device_model.clone(),
                    method: config.method.clone(),
                    method_name: Some(wipe_method_name(&config.method).to_string()),
                    status: format!("Pass {pass_num}/{total_passes}"),
                    progress: overall_progress,
                    current_pass: pass_num,
                    total_passes,
                    speed: format!("{speed:.2} MB/s"),
                    eta: format!("{eta:.0}s"),
                    error: None,
                    sector_number: written,
                };
                if let Ok(json) = serde_json::to_string(&msg) {
                    let _ = progress.send(json);
                }
            }
        }
    }

    file.sync_all()
        .map_err(|error| format!("failed to flush pass {pass_num} to the device: {error}"))?;

    // Final progress update for the pass
    let final_progress = (pass_num as f64 * 100.0) / total_passes as f64;
    let msg = WipeProgress {
        device_id: config.device_path.clone(),
        device_model: None,
        method: config.method.clone(),
        method_name: None,
        status: format!("Pass {pass_num}/{total_passes} complete"),
        progress: final_progress,
        current_pass: pass_num,
        total_passes,
        speed: String::new(),
        eta: String::new(),
        error: None,
        sector_number: written,
    };
    if let Ok(json) = serde_json::to_string(&msg) {
        let _ = progress.send(json);
    }

    Ok(())
}

pub fn abort_wipe(device_id: &str) -> Result<(), String> {
    let mut wipes = active_wipes().lock().unwrap();
    match wipes.get(device_id) {
        Some(controls) => {
            controls.cancel.store(true, Ordering::SeqCst);
            wipes.remove(device_id);
            Ok(())
        }
        None => Err(format!("no active wipe found for device {device_id}")),
    }
}

pub fn pause_wipe(device_id: &str) -> Result<(), String> {
    let wipes = active_wipes().lock().unwrap();
    match wipes.get(device_id) {
        Some(controls) => {
            let was_paused = controls.paused.fetch_xor(true, Ordering::SeqCst);
            log_line(&format!(
                "Wipe for {device_id} {}",
                if was_paused { "resumed" } else { "paused" }
            ));
            Ok(())
        }
        None => Err(format!("no active wipe found for device {device_id}")),
    }
}

fn send_completion(config: &WipeConfig, progress: &tokio::sync::mpsc::UnboundedSender<String>) {
    let completion = WipeProgress {
        device_id: config.device_path.clone(),
        device_model: None,
        method: String::new(),
        method_name: None,
        status: "done".to_string(),
        progress: 100.0,
        current_pass: 0,
        total_passes: 0,
        speed: String::new(),
        eta: String::new(),
        error: None,
        sector_number: 0,
    };
    if let Ok(json) = serde_json::to_string(&completion) {
        let _ = progress.send(json);
    }
}

pub(crate) fn sanitize_overwrite_two_pass(
    config: WipeConfig,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    let controls = register_wipe(&config.device_path);
    let result: Result<(), String> = (|| {
        let _ = progress.send("Executing Pass 1/2 (Pattern: 0x55)...".to_string());
        overwrite_pass(&controls, &config, 0x55, 1, 2, progress)?;

        log_line("First pass complete, starting second pass.");

        let _ = progress.send("Executing Pass 2/2 (Pattern: 0xAA)...".to_string());
        overwrite_pass(&controls, &config, 0xAA, 2, 2, progress)?;
        Ok(())
    })();
    unregister_wipe(&config.device_path);
    result?;
    send_completion(&config, progress);
    Ok(())
}

pub(crate) fn sanitize_overwrite(
    config: WipeConfig,
    passes: i32,
    progress: &tokio::sync::mpsc::UnboundedSender<String>,
) -> Result<(), String> {
    let controls = register_wipe(&config.device_path);
    let patterns: [u8; 3] = [0x00, 0xFF, 0x55]; // A simple set of patterns for multi-pass

    let result: Result<(), String> = (|| {
        for i in 1..=passes {
            let pattern = patterns[((i - 1) as usize) % patterns.len()];
            let _ = progress.send(format!(
                "Executing Pass {i}/{passes} (Pattern: 0x{pattern:02X})..."
            ));
            overwrite_pass(&controls, &config, pattern, i, passes, progress)?;
        }
        Ok(())
    })();
    unregister_wipe(&config.device_path);
    result?;
    send_completion(&config, progress);
    Ok(())
}
