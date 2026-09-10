use crate::core::ata::AtaEraseMode;
use crate::core::drives::{Drive, DriveType, MobileDevice};
use crate::core::wiper::*;
use std::io::Write;

fn drive(drive_type: DriveType) -> Drive {
    Drive {
        name: "/dev/sda".to_string(),
        model: "Test".to_string(),
        serial: "SERIAL".to_string(),
        wwn: "WWN".to_string(),
        size: "1000".to_string(),
        transport: "sata".to_string(),
        major_minor: "8:0".to_string(),
        drive_type,
        is_mounted: false,
        is_frozen: false,
        is_os_drive: false,
        active_dependencies: vec![],
        partitions: vec![],
    }
}

#[test]
fn wipe_method_names_are_stable() {
    assert_eq!(wipe_method_name("nvme_format"), "Purge: NVMe Format");
    assert_eq!(
        wipe_method_name("nvme_sanitize_crypto"),
        "Purge: NVMe Sanitize (Crypto Erase)"
    );
    assert_eq!(
        wipe_method_name("nvme_sanitize_block"),
        "Purge: NVMe Sanitize (Block Erase)"
    );
    assert_eq!(
        wipe_method_name("nvme_sanitize_overwrite"),
        "Purge: NVMe Sanitize (Overwrite)"
    );
    assert_eq!(
        wipe_method_name("overwrite_1_pass"),
        "Clear: 1-Pass Overwrite"
    );
    assert_eq!(
        wipe_method_name("sata_secure_erase"),
        "Purge: ATA Secure Erase"
    );
    assert_eq!(
        wipe_method_name("sata_secure_erase_enhanced"),
        "Purge: ATA Enhanced Secure Erase"
    );
    assert_eq!(
        wipe_method_name("overwrite_3_pass"),
        "Purge: 3-Pass Overwrite"
    );
    assert_eq!(
        wipe_method_name("overwrite_2_pass"),
        "Clear: 2-Pass Overwrite"
    );
    assert_eq!(
        wipe_method_name("android_factory_reset"),
        "Clear: Factory Reset"
    );
    assert_eq!(wipe_method_name("something_else"), "Unknown");
}

#[test]
fn methods_are_scoped_to_compatible_drive_types() {
    let ids_of = |t: DriveType| -> Vec<String> {
        get_wipe_methods_for_drive(&drive(t))
            .into_iter()
            .map(|m| m.id)
            .collect()
    };
    assert_eq!(
        ids_of(DriveType::Nvme),
        [
            "nvme_sanitize_crypto",
            "nvme_sanitize_block",
            "nvme_sanitize_overwrite",
            "nvme_format",
            "overwrite_1_pass"
        ]
    );
    assert_eq!(
        ids_of(DriveType::Ssd),
        [
            "sata_secure_erase_enhanced",
            "sata_secure_erase",
            "overwrite_1_pass"
        ]
    );
    assert_eq!(
        ids_of(DriveType::Hdd),
        ["overwrite_1_pass", "overwrite_3_pass"]
    );
    for t in [DriveType::Usb, DriveType::Unknown] {
        assert_eq!(ids_of(t), ["overwrite_2_pass"]);
    }
}

#[test]
fn non_ata_ssd_does_not_offer_ata_security_erase() {
    let mut target = drive(DriveType::Ssd);
    target.transport = "sas".to_string();

    let methods = get_wipe_methods_for_drive(&target)
        .into_iter()
        .map(|method| method.id)
        .collect::<Vec<_>>();

    assert_eq!(methods, ["overwrite_1_pass"]);
}

#[test]
fn mobile_methods_are_blocked_without_verifiable_reset_support() {
    let device = MobileDevice {
        name: "Pixel".to_string(),
        model: "Pixel".to_string(),
        serial: "SERIAL".to_string(),
        device_type: "Android".to_string(),
    };
    assert!(get_wipe_methods_for_mobile(&device).is_empty());
}

#[test]
fn wipe_config_accepts_go_and_frontend_json_casing() {
    // The Go server used encoding/json (case-insensitive) and the frontend
    // sends PascalCase — both must keep working.
    let pascal: WipeConfig = serde_json::from_str(
        r#"{"DevicePath":"/dev/sda","Method":"overwrite_1_pass","DeviceSerial":"S","DeviceType":"HDD","DeviceModel":"M","ExpectedIdentity":{"model":"M","serial":"S","wwn":"W","sizeBytes":"1000","transport":"sata","majorMinor":"8:0"}}"#,
    )
    .unwrap();
    assert_eq!(pascal.device_path, "/dev/sda");
    assert_eq!(pascal.method, "overwrite_1_pass");
    assert_eq!(pascal.device_serial, "S");
    assert_eq!(pascal.device_type, "HDD");
    assert_eq!(pascal.device_model.as_deref(), Some("M"));
    assert_eq!(pascal.expected_identity.as_ref().unwrap().wwn, "W");

    let camel: WipeConfig =
        serde_json::from_str(r#"{"devicePath":"/dev/sdb","method":"nvme_format"}"#).unwrap();
    assert_eq!(camel.device_path, "/dev/sdb");
    assert_eq!(camel.device_serial, "");
    assert_eq!(camel.device_model, None);
    assert_eq!(camel.expected_identity, None);
}

#[test]
fn overwrite_pass_writes_pattern_to_full_extent() {
    let dir = std::env::temp_dir().join(format!("dzap-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("disk.img");
    let size: u64 = 1024 * 1024; // 1 MiB fake device
    std::fs::File::create(&path).unwrap().set_len(size).unwrap();

    let controls = WipeControls {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        paused: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let config = WipeConfig {
        device_path: path.to_string_lossy().to_string(),
        method: "overwrite_1_pass".to_string(),
        device_serial: String::new(),
        device_type: String::new(),
        device_model: Some("Fake".to_string()),
        expected_identity: None,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    overwrite_pass(&controls, &config, 0xA5, 1, 1, &tx).unwrap();
    drop(tx);

    // Every byte of the fake device must now be the pattern.
    let data = std::fs::read(&path).unwrap();
    assert_eq!(data.len(), size as usize);
    assert!(data.iter().all(|&b| b == 0xA5));

    // The final progress message must report pass completion at 100%.
    let mut last: Option<serde_json::Value> = None;
    while let Ok(msg) = rx.try_recv() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&msg)
            && v.get("progress").is_some()
        {
            last = Some(v);
        }
    }
    let last = last.expect("expected at least one JSON progress message");
    assert_eq!(last["progress"], serde_json::json!(100.0));
    assert_eq!(last["status"], serde_json::json!("Pass 1/1 complete"));
    assert_eq!(last["sectorNumber"], serde_json::json!(size));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn overwrite_pass_does_not_extend_partial_final_block() {
    let dir = std::env::temp_dir().join(format!("dzap-test-partial-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("disk.img");
    let size = 128 * 1024 + 17;
    std::fs::File::create(&path)
        .unwrap()
        .set_len(size as u64)
        .unwrap();

    let controls = WipeControls {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        paused: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let config = WipeConfig {
        device_path: path.to_string_lossy().to_string(),
        method: "overwrite_1_pass".to_string(),
        device_serial: String::new(),
        device_type: String::new(),
        device_model: None,
        expected_identity: None,
    };
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    overwrite_pass(&controls, &config, 0x3C, 1, 1, &tx).unwrap();

    let data = std::fs::read(&path).unwrap();
    assert_eq!(data.len(), size, "wipe must preserve the target extent");
    assert!(data.iter().all(|&byte| byte == 0x3C));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn overwrite_pass_aborts_when_cancelled() {
    let dir = std::env::temp_dir().join(format!("dzap-test-abort-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("disk.img");
    // 2 GiB sparse file: writing would take long enough that we never finish.
    std::fs::File::create(&path)
        .unwrap()
        .set_len(2 * 1024 * 1024 * 1024)
        .unwrap();

    let controls = WipeControls {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        paused: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    // Pre-cancel: the very first loop iteration must bail out.
    controls
        .cancel
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let config = WipeConfig {
        device_path: path.to_string_lossy().to_string(),
        method: "overwrite_1_pass".to_string(),
        device_serial: String::new(),
        device_type: String::new(),
        device_model: None,
        expected_identity: None,
    };
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let err = overwrite_pass(&controls, &config, 0x00, 1, 1, &tx).unwrap_err();
    assert_eq!(err, "wipe aborted");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn pause_and_abort_unknown_devices_error() {
    let err = pause_wipe("/dev/definitely-not-wiping").unwrap_err();
    assert!(err.contains("no active wipe found"), "got: {err}");
    let err = abort_wipe("/dev/definitely-not-wiping").unwrap_err();
    assert!(err.contains("no active wipe found"), "got: {err}");
}

#[test]
fn command_runner_drains_output_while_the_process_is_running() {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = run_command(&worker_cancel, "sh", &["-c", "head -c 1048576 /dev/zero"]);
        let _ = result_tx.send(result);
    });

    match result_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(result) => result.unwrap(),
        Err(error) => {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            panic!("command runner blocked on a full output pipe: {error}");
        }
    }
}

#[test]
fn failed_ata_erase_attempts_temporary_password_cleanup() {
    let (progress, _messages) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut calls = Vec::new();

    let error = sanitize_sata_with(
        "/dev/test-ata",
        AtaEraseMode::Enhanced,
        &progress,
        |phase, arguments| {
            calls.push((phase, arguments.to_vec()));
            if arguments.contains(&"--security-erase-enhanced".to_string()) {
                Err("erase rejected".to_string())
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();

    assert!(error.contains("ATA erase command failed: erase rejected"));
    assert!(error.contains("temporary ATA security password was disabled"));
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].0, AtaCommandPhase::Destructive);
    assert!(calls[0].1.contains(&"--security-set-pass".to_string()));
    assert_eq!(calls[1].0, AtaCommandPhase::Destructive);
    assert!(
        calls[1]
            .1
            .contains(&"--security-erase-enhanced".to_string())
    );
    assert_eq!(calls[2].0, AtaCommandPhase::Recovery);
    assert!(calls[2].1.contains(&"--security-disable".to_string()));
}

#[test]
fn register_unregister_cycle() {
    let controls = register_wipe("/dev/testdzap");
    assert!(active_wipes().lock().unwrap().contains_key("/dev/testdzap"));
    // Pausing a registered wipe toggles its paused flag.
    pause_wipe("/dev/testdzap").unwrap();
    assert!(controls.paused.load(std::sync::atomic::Ordering::SeqCst));
    pause_wipe("/dev/testdzap").unwrap();
    assert!(!controls.paused.load(std::sync::atomic::Ordering::SeqCst));
    // Aborting sets cancel and removes the entry.
    abort_wipe("/dev/testdzap").unwrap();
    assert!(controls.cancel.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!active_wipes().lock().unwrap().contains_key("/dev/testdzap"));
}

#[test]
fn device_reservation_blocks_overlapping_jobs_until_released() {
    let device = format!("/dev/testdzap-reservation-{}", std::process::id());
    let first = reserve_device(&device).unwrap();

    let error = match reserve_device(&device) {
        Ok(_) => panic!("overlapping reservation was accepted"),
        Err(error) => error,
    };
    assert!(error.contains("already active"), "unexpected: {error}");

    drop(first);
    let replacement = reserve_device(&device).unwrap();
    drop(replacement);
}

#[test]
fn android_factory_reset_runs_for_serial_and_reports_progress() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut requested_serial = None;
    sanitize_android_with("FAKE_SERIAL_123", &tx, |serial| {
        requested_serial = Some(serial.to_string());
        Ok(())
    })
    .unwrap();
    drop(tx);

    assert_eq!(requested_serial.as_deref(), Some("FAKE_SERIAL_123"));
    assert_eq!(
        rx.try_recv().unwrap(),
        "Executing Android Factory Reset (NIST Clear) on device FAKE_SERIAL_123..."
    );
    assert_eq!(
        rx.try_recv().unwrap(),
        "Reboot to recovery command sent. The device will now perform a factory reset."
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn android_factory_reset_propagates_reboot_failure() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let err = sanitize_android_with("SERIAL", &tx, |_| Err("adb failed".to_string())).unwrap_err();
    drop(tx);

    assert_eq!(err, "adb failed");
    assert!(rx.try_recv().unwrap().contains("SERIAL"));
    assert!(
        rx.try_recv().is_err(),
        "success message must not be emitted"
    );
}

// Guard against a classic regression: the overwrite loop must not deadlock
// when the progress receiver has been dropped (client disconnected).
#[test]
fn overwrite_pass_survives_dropped_progress_receiver() {
    let dir = std::env::temp_dir().join(format!("dzap-test-drop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("disk.img");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&vec![0u8; 256 * 1024]).unwrap();
    drop(f);

    let controls = WipeControls {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        paused: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let config = WipeConfig {
        device_path: path.to_string_lossy().to_string(),
        method: "overwrite_1_pass".to_string(),
        device_serial: String::new(),
        device_type: String::new(),
        device_model: None,
        expected_identity: None,
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    drop(rx); // receiver gone

    overwrite_pass(&controls, &config, 0xFF, 1, 1, &tx).unwrap();

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn wipe_progress_uses_frontend_field_names_and_omits_absent_values() {
    let progress = WipeProgress {
        device_id: "/dev/test".to_string(),
        device_model: None,
        method: "overwrite_1_pass".to_string(),
        method_name: None,
        status: "Pass 1/1".to_string(),
        progress: 50.0,
        current_pass: 1,
        total_passes: 1,
        speed: "2.00 MB/s".to_string(),
        eta: "1s".to_string(),
        error: None,
        sector_number: 65536,
    };

    let json = serde_json::to_value(progress).unwrap();
    assert_eq!(json["deviceId"], serde_json::json!("/dev/test"));
    assert_eq!(json["currentPass"], serde_json::json!(1));
    assert_eq!(json["totalPasses"], serde_json::json!(1));
    assert_eq!(json["sectorNumber"], serde_json::json!(65536));
    assert!(json.get("deviceModel").is_none());
    assert!(json.get("methodName").is_none());
    assert!(json.get("error").is_none());
}

#[test]
fn two_pass_overwrite_uses_complement_pattern_and_sends_completion() {
    let dir = std::env::temp_dir().join(format!("dzap-test-two-pass-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("disk.img");
    let size = 128 * 1024 + 23;
    std::fs::File::create(&path)
        .unwrap()
        .set_len(size as u64)
        .unwrap();
    let config = WipeConfig {
        device_path: path.to_string_lossy().to_string(),
        method: "overwrite_2_pass".to_string(),
        device_serial: String::new(),
        device_type: "USB Drive".to_string(),
        device_model: Some("Test USB".to_string()),
        expected_identity: None,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    sanitize_overwrite_two_pass(config.clone(), &tx).unwrap();
    drop(tx);

    let data = std::fs::read(&path).unwrap();
    assert_eq!(data.len(), size);
    assert!(data.iter().all(|&byte| byte == 0xAA));
    assert!(
        !active_wipes()
            .lock()
            .unwrap()
            .contains_key(&config.device_path)
    );

    let messages: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(
        messages.first().unwrap(),
        "Executing Pass 1/2 (Pattern: 0x55)..."
    );
    assert!(
        messages
            .iter()
            .any(|msg| msg == "Executing Pass 2/2 (Pattern: 0xAA)...")
    );
    let completion: serde_json::Value = serde_json::from_str(messages.last().unwrap()).unwrap();
    assert_eq!(completion["status"], serde_json::json!("done"));
    assert_eq!(completion["progress"], serde_json::json!(100.0));
    assert_eq!(
        completion["deviceId"],
        serde_json::json!(config.device_path)
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn three_pass_overwrite_cycles_patterns_and_cleans_up_registration() {
    let dir = std::env::temp_dir().join(format!("dzap-test-three-pass-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("disk.img");
    std::fs::File::create(&path).unwrap().set_len(4096).unwrap();
    let config = WipeConfig {
        device_path: path.to_string_lossy().to_string(),
        method: "overwrite_3_pass".to_string(),
        device_serial: String::new(),
        device_type: "HDD".to_string(),
        device_model: None,
        expected_identity: None,
    };
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    sanitize_overwrite(config.clone(), 3, &tx).unwrap();
    drop(tx);

    assert!(
        std::fs::read(&path)
            .unwrap()
            .iter()
            .all(|&byte| byte == 0x55)
    );
    assert!(
        !active_wipes()
            .lock()
            .unwrap()
            .contains_key(&config.device_path)
    );
    let messages: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    for expected in [
        "Executing Pass 1/3 (Pattern: 0x00)...",
        "Executing Pass 2/3 (Pattern: 0xFF)...",
        "Executing Pass 3/3 (Pattern: 0x55)...",
    ] {
        assert!(messages.iter().any(|message| message == expected));
    }

    std::fs::remove_dir_all(&dir).ok();
}
