use super::certificate::{generate_certificate_for_job, init_for_tests};
use super::drives::DeviceIdentity;
use super::evidence_export::{
    ExportDestination, export_bundle_to_mount, export_destinations_from_lsblk, validate_bundle,
};
use super::jobs::JobStore;
use super::preflight::{PreflightDecision, WipePlan};
use super::verification::{VerificationResult, VerificationStrategy};
use std::path::PathBuf;

fn temp_directory(test_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dzap-export-{test_name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn completed_job() -> super::jobs::WipeJob {
    let store = JobStore::in_memory();
    let job = store
        .create(&WipePlan {
            decision: PreflightDecision::Ready,
            device_path: "/dev/test-target".to_string(),
            device_model: "Test Target".to_string(),
            device_type: "HDD".to_string(),
            method: "overwrite_1_pass".to_string(),
            identity: Some(DeviceIdentity {
                model: "Test Target".to_string(),
                serial: "TARGET-SERIAL".to_string(),
                wwn: "0x1234".to_string(),
                size_bytes: "4096".to_string(),
                transport: "sata".to_string(),
                major_minor: "8:16".to_string(),
            }),
            checks: Vec::new(),
        })
        .unwrap();
    store.begin_verification(&job.id).unwrap();
    store
        .complete_verification(
            &job.id,
            VerificationResult {
                strategy: VerificationStrategy::FullPatternReadback,
                bytes_checked: 4096,
                readback_sha256: "a".repeat(64),
                expected_pattern: Some("0x00".to_string()),
                firmware_status_sha256: None,
                identity_revalidated: true,
            },
        )
        .unwrap()
}

fn destination(mount_path: &std::path::Path) -> ExportDestination {
    ExportDestination {
        drive_path: "/dev/sdz".to_string(),
        drive_major_minor: "8:240".to_string(),
        device_path: "/dev/sdz1".to_string(),
        device_major_minor: "8:241".to_string(),
        mount_path: Some(mount_path.display().to_string()),
        model: "Evidence USB".to_string(),
        serial: "EXPORT-SERIAL".to_string(),
        transport: "usb".to_string(),
        filesystem: "vfat".to_string(),
        size_bytes: "1048576".to_string(),
    }
}

#[test]
fn destination_discovery_only_returns_safe_mounted_removable_media() {
    let fixture = br#"{
      "blockdevices": [
        {
          "name": "nvme0n1", "type": "disk", "tran": "nvme", "rm": false,
          "ro": false, "mountpoints": [null], "maj:min": "259:0",
          "children": [{"name": "nvme0n1p1", "type": "part", "ro": false,
            "mountpoints": ["/"], "fstype": "ext4", "maj:min": "259:1"}]
        },
        {
          "name": "sdb", "type": "disk", "model": "DZap Boot", "serial": "BOOT",
          "tran": "usb", "rm": true, "ro": false, "mountpoints": [null],
          "maj:min": "8:16", "children": [{"name": "sdb1", "type": "part",
            "ro": false, "mountpoints": ["/run/archiso/bootmnt"], "fstype": "iso9660",
            "maj:min": "8:17"}]
        },
        {
          "name": "sdc", "type": "disk", "model": "Evidence USB ",
          "serial": " EXPORT-SERIAL", "size": 2097152, "tran": "usb", "rm": false,
          "ro": false, "mountpoints": [null], "maj:min": "8:32",
          "children": [{"name": "sdc1", "type": "part", "size": 1048576,
            "ro": false, "mountpoints": ["/run/media/dzap/EVIDENCE"], "fstype": "vfat",
            "maj:min": "8:33"}, {"name": "sdc2", "type": "part", "size": 524288,
            "ro": false, "mountpoints": [null], "fstype": "ext4", "maj:min": "8:34"},
            {"name": "sdc3", "type": "part", "size": 524288, "ro": false,
            "mountpoints": [null], "fstype": "ntfs", "maj:min": "8:35"}]
        },
        {
          "name": "sdd", "type": "disk", "model": "Read only", "tran": "usb",
          "rm": true, "ro": true, "mountpoints": ["/mnt/readonly"], "maj:min": "8:48"
        }
      ]
    }"#;

    let destinations = export_destinations_from_lsblk(fixture).unwrap();
    assert_eq!(destinations.len(), 2);
    let found = destinations
        .iter()
        .find(|destination| destination.device_path == "/dev/sdc1")
        .unwrap();
    assert_eq!(found.drive_path, "/dev/sdc");
    assert_eq!(found.device_path, "/dev/sdc1");
    assert_eq!(
        found.mount_path.as_deref(),
        Some("/run/media/dzap/EVIDENCE")
    );
    assert_eq!(found.model, "Evidence USB");
    assert_eq!(found.serial, "EXPORT-SERIAL");
    assert_eq!(found.drive_major_minor, "8:32");
    assert_eq!(found.device_major_minor, "8:33");
    let unmounted = destinations
        .iter()
        .find(|destination| destination.device_path == "/dev/sdc2")
        .unwrap();
    assert_eq!(unmounted.mount_path, None);
    assert_eq!(unmounted.filesystem, "ext4");
}

#[test]
fn export_bundle_is_verified_after_write_and_idempotent() {
    init_for_tests();
    let mount_path = temp_directory("valid");
    std::fs::create_dir_all(&mount_path).unwrap();
    let job = completed_job();
    let certificate = generate_certificate_for_job(&job).unwrap();
    let destination = destination(&mount_path);

    let first = export_bundle_to_mount(&job, &certificate, &destination, &mount_path).unwrap();
    assert!(!first.already_existed);
    assert_eq!(first.job_id, job.id);
    let bundle_path = PathBuf::from(&first.bundle_path);
    let manifest = validate_bundle(&bundle_path).unwrap();
    assert_eq!(manifest.data.job_id, job.id);
    assert_eq!(manifest.data.evidence_format_version, 1);
    assert_eq!(manifest.data.qr_payload_file, "certificate.json");
    assert_eq!(manifest.data.files.len(), 4);
    assert_eq!(manifest.signature.len(), 512);

    let second = export_bundle_to_mount(&job, &certificate, &destination, &mount_path).unwrap();
    assert!(second.already_existed);
    assert_eq!(second.bundle_path, first.bundle_path);
    assert_eq!(second.exported_at, first.exported_at);

    std::fs::remove_dir_all(mount_path).ok();
}

#[test]
fn export_bundle_detects_tampering_during_readback() {
    init_for_tests();
    let mount_path = temp_directory("tampered");
    std::fs::create_dir_all(&mount_path).unwrap();
    let job = completed_job();
    let certificate = generate_certificate_for_job(&job).unwrap();
    let destination = destination(&mount_path);
    let result = export_bundle_to_mount(&job, &certificate, &destination, &mount_path).unwrap();
    let bundle_path = PathBuf::from(result.bundle_path);

    let original_job = std::fs::read(bundle_path.join("job.json")).unwrap();
    std::fs::write(bundle_path.join("job.json"), b"{}\n").unwrap();
    let error = validate_bundle(&bundle_path).unwrap_err();
    assert!(
        error.contains("hash validation failed"),
        "unexpected: {error}"
    );
    let retry_error =
        export_bundle_to_mount(&job, &certificate, &destination, &mount_path).unwrap_err();
    assert!(
        retry_error.contains("hash validation failed"),
        "unexpected: {retry_error}"
    );

    std::fs::write(bundle_path.join("job.json"), original_job).unwrap();
    let manifest_path = bundle_path.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["data"]["applicationVersion"] = serde_json::json!("forged-version");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let error = validate_bundle(&bundle_path).unwrap_err();
    assert!(
        error.contains("manifest signature validation failed"),
        "unexpected: {error}"
    );

    std::fs::remove_dir_all(mount_path).ok();
}

#[test]
fn wipe_target_cannot_receive_its_own_evidence() {
    init_for_tests();
    let mount_path = temp_directory("same-target");
    std::fs::create_dir_all(&mount_path).unwrap();
    let job = completed_job();
    let certificate = generate_certificate_for_job(&job).unwrap();
    let mut destination = destination(&mount_path);
    destination.drive_path = job.device_path.clone();
    destination.drive_major_minor = job.identity.major_minor.clone();

    let error = export_bundle_to_mount(&job, &certificate, &destination, &mount_path).unwrap_err();
    assert!(error.contains("wipe target"), "unexpected: {error}");

    std::fs::remove_dir_all(mount_path).ok();
}
