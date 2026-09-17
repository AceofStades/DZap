use super::drives::{BlockDependency, Drive, DriveType};
use super::recovery::*;
use std::fs::{File, remove_file};

fn drive() -> Drive {
    Drive {
        name: "/dev/test-recovery".to_string(),
        model: "Recovery Source".to_string(),
        serial: "RECOVERY-1".to_string(),
        wwn: "0x1234".to_string(),
        size: (512 * 1024).to_string(),
        transport: "usb".to_string(),
        major_minor: "8:48".to_string(),
        drive_type: DriveType::Usb,
        is_mounted: false,
        is_frozen: false,
        is_os_drive: false,
        active_dependencies: Vec::new(),
        partitions: Vec::new(),
    }
}

#[test]
fn signature_scan_includes_partition_tables_filesystems_and_encryption() {
    let input = br#"{
        "blockdevices": [{
            "path": "/dev/sdb",
            "pttype": "gpt",
            "fstype": null,
            "children": [
                {"path": "/dev/sdb1", "pttype": null, "fstype": "crypto_LUKS"},
                {"path": "/dev/sdb2", "pttype": null, "fstype": "ext4"}
            ]
        }]
    }"#;

    let signatures = signatures_from_lsblk(input, "/dev/sdb").unwrap();
    assert_eq!(signatures.len(), 3);
    assert_eq!(signatures[0].kind, "partition_table");
    assert_eq!(signatures[0].value, "gpt");
    assert_eq!(signatures[1].device_path, "/dev/sdb1");
    assert!(is_encrypted_signature(&signatures[1].value));
    assert_eq!(signatures[2].value, "ext4");
}

#[test]
fn smart_scan_extracts_ata_and_nvme_damage_indicators() {
    let input = br#"{
        "smart_status": {"passed": false},
        "ata_smart_attributes": {"table": [
            {"id": 5, "raw": {"value": 7}},
            {"id": 187, "raw": {"value": "2"}},
            {"id": 197, "raw": {"value": 3}},
            {"id": 198, "raw": {"value": 4}}
        ]},
        "nvme_smart_health_information_log": {
            "media_errors": 9,
            "critical_warning": 1
        }
    }"#;

    let smart = smart_evidence_from_json(input).unwrap();
    assert!(smart.available);
    assert_eq!(smart.passed, Some(false));
    assert_eq!(smart.reallocated_sectors, Some(7));
    assert_eq!(smart.reported_uncorrectable, Some(2));
    assert_eq!(smart.pending_sectors, Some(3));
    assert_eq!(smart.offline_uncorrectable, Some(4));
    assert_eq!(smart.nvme_media_errors, Some(9));
    assert_eq!(smart.nvme_critical_warning, Some(1));
    assert!(smart.reports_damage());
}

#[test]
fn degraded_encrypted_source_recommends_imaging_and_unlocking() {
    let mut source = drive();
    source.active_dependencies.push(BlockDependency {
        name: "/dev/mapper/secure".to_string(),
        device_type: "crypt".to_string(),
    });
    let assessment = build_assessment(
        &source,
        Ok(vec![RecoverySignature {
            device_path: "/dev/test-recovery1".to_string(),
            kind: "filesystem".to_string(),
            value: "crypto_LUKS".to_string(),
        }]),
        SmartEvidence {
            available: true,
            passed: Some(false),
            pending_sectors: Some(2),
            ..SmartEvidence::default()
        },
        ContentSample {
            source_opened: true,
            requested_samples: 5,
            completed_samples: 4,
            sampled_bytes: 4 * 64 * 1024,
            zero_bytes: 0,
            ff_bytes: 0,
            read_errors: vec!["read failed".to_string()],
        },
    );

    assert_eq!(assessment.decision, RecoveryDecision::Caution);
    assert_eq!(assessment.encryption, EncryptionState::Detected);
    assert_eq!(assessment.media_condition, MediaCondition::Degraded);
    assert_eq!(assessment.content_state, ContentState::StructuredData);
    assert_eq!(
        assessment
            .checks
            .iter()
            .find(|check| check.code == "active_block_dependencies")
            .unwrap()
            .status,
        RecoveryCheckStatus::Warning
    );
    assert!(
        assessment
            .recommendations
            .iter()
            .any(|message| message.contains("ddrescue"))
    );
    assert!(
        assessment
            .recommendations
            .iter()
            .any(|message| message.contains("read-only decrypted mapping"))
    );
}

#[test]
fn zero_filled_samples_are_only_classified_as_likely_blank() {
    let path = std::env::temp_dir().join(format!(
        "dzap-recovery-zero-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let file = File::create(&path).unwrap();
    file.set_len(512 * 1024).unwrap();
    drop(file);

    let sample = sample_device(path.to_str().unwrap(), 512 * 1024);
    remove_file(&path).unwrap();
    assert_eq!(sample.requested_samples, 5);
    assert_eq!(sample.completed_samples, 5);
    assert!(sample.read_errors.is_empty());

    let assessment = build_assessment(
        &drive(),
        Ok(Vec::new()),
        SmartEvidence {
            available: true,
            passed: Some(true),
            ..SmartEvidence::default()
        },
        sample,
    );
    assert_eq!(assessment.content_state, ContentState::LikelyBlank);
    assert_eq!(assessment.decision, RecoveryDecision::Caution);
    let content_check = assessment
        .checks
        .iter()
        .find(|check| check.code == "recoverable_content")
        .unwrap();
    assert!(content_check.message.contains("cannot prove a secure wipe"));
}

#[test]
fn blank_samples_stay_unknown_when_signature_scan_fails() {
    let assessment = build_assessment(
        &drive(),
        Err("signature probe unavailable".to_string()),
        SmartEvidence::default(),
        ContentSample {
            source_opened: true,
            requested_samples: 5,
            completed_samples: 5,
            sampled_bytes: 5 * 64 * 1024,
            zero_bytes: 5 * 64 * 1024,
            ..ContentSample::default()
        },
    );

    assert_eq!(assessment.content_state, ContentState::Unknown);
    assert_eq!(assessment.encryption, EncryptionState::Unknown);
    assert_eq!(assessment.decision, RecoveryDecision::Caution);
}

#[test]
fn protected_system_source_is_blocked_without_read_probes() {
    let mut source = drive();
    source.is_os_drive = true;
    let assessment = protected_device_assessment(&source);

    assert_eq!(assessment.decision, RecoveryDecision::Blocked);
    assert_eq!(assessment.sample.requested_samples, 0);
    assert!(assessment.signatures.is_empty());
    assert_eq!(
        assessment
            .checks
            .iter()
            .find(|check| check.code == "protected_system")
            .unwrap()
            .status,
        RecoveryCheckStatus::Blocked
    );
}
