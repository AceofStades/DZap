use server::core::nvme::{
    NvmeSanitizeAction, NvmeSanitizeCapabilities, controller_path, sanitize_arguments,
    validate_sanitize_log,
};
use server::core::verification::{VerificationResult, VerificationStrategy};

#[test]
fn sanicap_uses_the_nvme_spec_bit_order() {
    let crypto = NvmeSanitizeCapabilities::from_json(r#"{"sanicap":1}"#).unwrap();
    assert!(crypto.supports(NvmeSanitizeAction::CryptoErase));
    assert!(!crypto.supports(NvmeSanitizeAction::BlockErase));
    assert!(!crypto.supports(NvmeSanitizeAction::Overwrite));

    let block = NvmeSanitizeCapabilities::from_json(r#"{"sanicap":2}"#).unwrap();
    assert!(!block.supports(NvmeSanitizeAction::CryptoErase));
    assert!(block.supports(NvmeSanitizeAction::BlockErase));
    assert!(!block.supports(NvmeSanitizeAction::Overwrite));

    let overwrite = NvmeSanitizeCapabilities::from_json(r#"{"sanicap":4}"#).unwrap();
    assert!(!overwrite.supports(NvmeSanitizeAction::CryptoErase));
    assert!(!overwrite.supports(NvmeSanitizeAction::BlockErase));
    assert!(overwrite.supports(NvmeSanitizeAction::Overwrite));

    let purge = NvmeSanitizeCapabilities::from_json(r#"{"sanicap":"0x21"}"#).unwrap();
    assert!(purge.supports(NvmeSanitizeAction::CryptoErase));
    assert!(purge.supports_purge_reporting());
}

#[test]
fn namespace_paths_resolve_to_the_controller_character_device() {
    assert_eq!(controller_path("/dev/nvme0n1").unwrap(), "/dev/nvme0");
    assert_eq!(controller_path("nvme12n34").unwrap(), "/dev/nvme12");
    assert_eq!(controller_path("/dev/nvme3c7n2").unwrap(), "/dev/nvme3");
    assert_eq!(controller_path("/dev/nvme5").unwrap(), "/dev/nvme5");
    assert!(
        controller_path("/dev/sda")
            .unwrap_err()
            .contains("not an NVMe")
    );
    assert!(controller_path("/dev/nvme0n1p1").is_err());
}

#[test]
fn sanitize_command_uses_the_requested_supported_action_and_waits() {
    let capabilities = NvmeSanitizeCapabilities::from_json(r#"{"sanicap":33}"#).unwrap();
    let arguments = sanitize_arguments(
        "/dev/nvme0n1",
        NvmeSanitizeAction::CryptoErase,
        capabilities,
    )
    .unwrap();

    assert_eq!(
        arguments,
        [
            "sanitize",
            "/dev/nvme0",
            "--sanact=0x04",
            "--wait",
            "--preq"
        ]
    );
    let error = sanitize_arguments("/dev/nvme0n1", NvmeSanitizeAction::BlockErase, capabilities)
        .unwrap_err();
    assert!(error.contains("does not advertise"));
}

fn sanitize_log(status: u16, command: u32, progress: u16) -> Vec<u8> {
    let mut log = vec![0_u8; 512];
    log[0..2].copy_from_slice(&progress.to_le_bytes());
    log[2..4].copy_from_slice(&status.to_le_bytes());
    log[4..8].copy_from_slice(&command.to_le_bytes());
    log
}

#[test]
fn sanitize_log_must_prove_completion_action_and_requested_purge() {
    let completed_purge = sanitize_log(0x0801, 0x0804, 0xffff);
    validate_sanitize_log(&completed_purge, NvmeSanitizeAction::CryptoErase, true).unwrap();

    for (log, expected_error) in [
        (sanitize_log(0x0802, 0x0804, 0x4000), "status was 2"),
        (sanitize_log(0x0801, 0x0802, 0xffff), "does not match"),
        (sanitize_log(0x0001, 0x0004, 0xffff), "purge-request"),
        (sanitize_log(0x0801, 0x0804, 0xfffe), "reported progress"),
    ] {
        let error = validate_sanitize_log(&log, NvmeSanitizeAction::CryptoErase, true).unwrap_err();
        assert!(error.contains(expected_error), "unexpected: {error}");
    }
    assert!(validate_sanitize_log(&[0_u8; 7], NvmeSanitizeAction::CryptoErase, false).is_err());
}

#[test]
fn verification_policy_binds_nvme_sanitize_evidence_to_the_method() {
    let evidence = VerificationResult {
        strategy: VerificationStrategy::NvmeSanitizeStatusAndSamples,
        bytes_checked: 4096,
        readback_sha256: "a".repeat(64),
        expected_pattern: None,
        firmware_status_sha256: Some("b".repeat(64)),
        identity_revalidated: true,
    };

    evidence
        .validate_for_job("nvme_sanitize_crypto", 4096)
        .unwrap();
    assert!(evidence.validate_for_job("nvme_format", 4096).is_err());
}
