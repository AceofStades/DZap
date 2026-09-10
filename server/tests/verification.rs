use server::core::verification::{
    VerificationStrategy, ata_security_is_disabled, verify_pattern_file,
};
use std::path::PathBuf;

fn temp_file(test_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dzap-verification-{test_name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn full_pattern_readback_hashes_every_approved_byte() {
    const SIZE: usize = 1024 * 1024 + 123;
    let path = temp_file("complete-readback");
    std::fs::write(&path, vec![0_u8; SIZE]).unwrap();

    let result = verify_pattern_file(&path, SIZE as u64, 0x00).unwrap();

    assert_eq!(result.strategy, VerificationStrategy::FullPatternReadback);
    assert_eq!(result.bytes_checked, SIZE as u64);
    assert_eq!(result.expected_pattern.as_deref(), Some("0x00"));
    assert_eq!(
        result.readback_sha256,
        "d38086f6cf00a20681062621c307d733feb803da4a7c7092123f32711f922405"
    );
    assert!(!result.identity_revalidated);
    std::fs::remove_file(path).ok();
}

#[test]
fn full_pattern_readback_reports_the_corrupt_byte_offset() {
    const SIZE: usize = 1024 * 1024 + 32;
    const CORRUPT_OFFSET: usize = 1024 * 1024 + 17;
    let path = temp_file("corrupt-byte");
    let mut contents = vec![0xAA_u8; SIZE];
    contents[CORRUPT_OFFSET] = 0x55;
    std::fs::write(&path, contents).unwrap();

    let error = verify_pattern_file(&path, SIZE as u64, 0xAA).unwrap_err();

    assert!(
        error.contains(&format!("mismatch at byte {CORRUPT_OFFSET}")),
        "unexpected: {error}"
    );
    assert!(error.contains("expected 0xAA, found 0x55"));
    std::fs::remove_file(path).ok();
}

#[test]
fn full_pattern_readback_rejects_a_changed_extent() {
    let path = temp_file("changed-extent");
    std::fs::write(&path, vec![0x55_u8; 32]).unwrap();

    let shorter = verify_pattern_file(&path, 33, 0x55).unwrap_err();
    assert!(shorter.contains("end of device after 32 of 33 bytes"));

    let larger = verify_pattern_file(&path, 31, 0x55).unwrap_err();
    assert!(larger.contains("larger than its approved 31-byte identity"));
    std::fs::remove_file(path).ok();
}

#[test]
fn ata_verification_requires_security_to_be_disabled_and_unlocked() {
    let erased = r#"
Security:
	Master password revision code = 65534
		supported
		not enabled
		not locked
		not frozen
Logical Unit WWN Device Identifier: 5000c50000000001
"#;
    assert!(ata_security_is_disabled(erased));

    for unsafe_status in [
        "Security:\n\t\tenabled\n\t\tnot locked\n",
        "Security:\n\t\tnot enabled\n\t\tlocked\n",
        "Security:\n\t\tnot enabled\n",
        "not enabled\nnot locked\n",
    ] {
        assert!(
            !ata_security_is_disabled(unsafe_status),
            "unsafe status accepted: {unsafe_status:?}"
        );
    }
}
