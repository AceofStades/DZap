use server::core::ata::{
    AtaEraseMode, AtaSecurityCapabilities, disable_password_arguments, erase_arguments,
    set_password_arguments,
};
use server::core::verification::{VerificationResult, VerificationStrategy};

const ENHANCED_SECURITY: &str = r#"
Security:
	Master password revision code = 65534
		supported
		not enabled
		not locked
		not frozen
		not expired: security count
		supported: enhanced erase
	2min for SECURITY ERASE UNIT. 2min for ENHANCED SECURITY ERASE UNIT.
Logical Unit WWN Device Identifier: 5000c50000000001
"#;

#[test]
fn ata_security_capabilities_are_read_only_from_the_security_section() {
    let capabilities = AtaSecurityCapabilities::from_hdparm_output(ENHANCED_SECURITY).unwrap();
    assert!(capabilities.supports(AtaEraseMode::Normal));
    assert!(capabilities.supports(AtaEraseMode::Enhanced));

    let basic = AtaSecurityCapabilities::from_hdparm_output(
        "Security:\n\t\tsupported\n\t\tnot enabled\nDevice:\n\t\tsupported: enhanced erase\n",
    )
    .unwrap();
    assert!(basic.supports(AtaEraseMode::Normal));
    assert!(!basic.supports(AtaEraseMode::Enhanced));

    assert!(AtaSecurityCapabilities::from_hdparm_output("no security information").is_err());
}

#[test]
fn ata_security_commands_use_hdparms_user_selector_and_exact_mode() {
    assert_eq!(
        set_password_arguments("/dev/sda"),
        [
            "--user-master",
            "u",
            "--security-set-pass",
            "dZap",
            "/dev/sda"
        ]
    );
    assert_eq!(
        erase_arguments("/dev/sda", AtaEraseMode::Enhanced),
        [
            "--user-master",
            "u",
            "--security-erase-enhanced",
            "dZap",
            "/dev/sda"
        ]
    );
    assert_eq!(
        disable_password_arguments("/dev/sda"),
        [
            "--user-master",
            "u",
            "--security-disable",
            "dZap",
            "/dev/sda"
        ]
    );
}

#[test]
fn enhanced_erase_verification_uses_the_ata_firmware_policy() {
    let evidence = VerificationResult {
        strategy: VerificationStrategy::AtaSecurityStatusAndSamples,
        bytes_checked: 4096,
        readback_sha256: "a".repeat(64),
        expected_pattern: None,
        firmware_status_sha256: Some("b".repeat(64)),
        identity_revalidated: true,
    };

    evidence
        .validate_for_job("sata_secure_erase_enhanced", 4096)
        .unwrap();
}
