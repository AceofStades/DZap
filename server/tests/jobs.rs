use server::core::drives::DeviceIdentity;
use server::core::jobs::{JobStore, WipeJobStatus};
use server::core::preflight::{PreflightCheck, PreflightCheckStatus, PreflightDecision, WipePlan};
use server::core::verification::{VerificationResult, VerificationStrategy};
use std::path::PathBuf;

fn approved_plan() -> WipePlan {
    WipePlan {
        decision: PreflightDecision::Ready,
        device_path: "/dev/test-disk".to_string(),
        device_model: "Detected Model".to_string(),
        device_type: "HDD".to_string(),
        method: "overwrite_1_pass".to_string(),
        identity: Some(DeviceIdentity {
            model: "Detected Model".to_string(),
            serial: "DETECTED-SERIAL".to_string(),
            wwn: "0x1234".to_string(),
            size_bytes: "1048576".to_string(),
            transport: "virtio".to_string(),
            major_minor: "253:0".to_string(),
        }),
        checks: vec![PreflightCheck {
            code: "device_identity".to_string(),
            status: PreflightCheckStatus::Passed,
            message: "Device identity matches the approved preflight plan.".to_string(),
        }],
    }
}

fn temp_directory(test_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dzap-{test_name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn verification() -> VerificationResult {
    VerificationResult {
        strategy: VerificationStrategy::FullPatternReadback,
        bytes_checked: 1_048_576,
        readback_sha256: "a".repeat(64),
        expected_pattern: Some("0x00".to_string()),
        firmware_status_sha256: None,
        identity_revalidated: true,
    }
}

#[test]
fn verified_job_binds_approved_identity_and_lifecycle_to_evidence() {
    let store = JobStore::in_memory();
    let created = store.create(&approved_plan()).unwrap();

    assert_eq!(created.status, WipeJobStatus::Running);
    assert_eq!(created.device_model, "Detected Model");
    assert_eq!(created.identity.serial, "DETECTED-SERIAL");
    assert_eq!(created.events[0].event_type, "wipe_authorized");
    assert!(created.events[0].message.contains("device_identity"));
    assert!(created.verify_evidence());

    let mut forged_completion = created.clone();
    forged_completion.status = WipeJobStatus::Verified;
    forged_completion.completed_at = Some(created.started_at);
    assert!(!forged_completion.verify_evidence());

    let verifying = store.begin_verification(&created.id).unwrap();
    assert_eq!(verifying.status, WipeJobStatus::Verifying);
    assert_eq!(verifying.events[1].event_type, "sanitization_completed");
    let completed = store
        .complete_verification(&created.id, verification())
        .unwrap();
    assert_eq!(completed.status, WipeJobStatus::Verified);
    assert_eq!(completed.events.len(), 3);
    assert_eq!(completed.events[2].event_type, "verification_completed");
    assert_eq!(
        completed.events[2].previous_hash.as_deref(),
        Some(verifying.evidence_hash.as_str())
    );
    assert_eq!(completed.evidence_hash, completed.events[2].event_hash);
    assert_eq!(completed.verification, Some(verification()));
    assert!(completed.verify_evidence());
}

#[test]
fn terminal_job_cannot_be_rewritten() {
    let store = JobStore::in_memory();
    let job = store.create(&approved_plan()).unwrap();
    store.fail(&job.id, "device disappeared").unwrap();

    let error = store.fail(&job.id, "second failure").unwrap_err();
    assert!(error.contains("already terminal"), "unexpected: {error}");
    let failed = store.get(&job.id).unwrap().unwrap();
    assert_eq!(failed.status, WipeJobStatus::Failed);
    assert_eq!(failed.failure.as_deref(), Some("device disappeared"));
    assert!(failed.verify_evidence());
}

#[test]
fn malformed_verification_cannot_make_a_job_verified() {
    let store = JobStore::in_memory();
    let job = store.create(&approved_plan()).unwrap();
    store.begin_verification(&job.id).unwrap();

    let mut malformed = verification();
    malformed.identity_revalidated = false;
    let error = store.complete_verification(&job.id, malformed).unwrap_err();

    assert!(error.contains("did not revalidate the device identity"));
    let unchanged = store.get(&job.id).unwrap().unwrap();
    assert_eq!(unchanged.status, WipeJobStatus::Verifying);
    assert!(unchanged.verification.is_none());
    assert!(unchanged.verify_evidence());
}

#[test]
fn persisted_jobs_detect_tampering() {
    let directory = temp_directory("tampered-evidence");
    let store = JobStore::persistent(directory.clone()).unwrap();
    let job = store.create(&approved_plan()).unwrap();
    store.begin_verification(&job.id).unwrap();
    store
        .complete_verification(&job.id, verification())
        .unwrap();

    let path = directory.join(format!("{}.json", job.id));
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, original.replace("Detected Model", "Changed Model")).unwrap();

    let error = match JobStore::persistent(directory.clone()) {
        Ok(_) => panic!("tampered evidence was accepted"),
        Err(error) => error,
    };
    assert!(
        error.contains("evidence verification failed"),
        "unexpected: {error}"
    );
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn restart_marks_unfinished_job_failed() {
    let directory = temp_directory("interrupted-job");
    let first = JobStore::persistent(directory.clone()).unwrap();
    let job = first.create(&approved_plan()).unwrap();
    drop(first);

    let restarted = JobStore::persistent(directory.clone()).unwrap();
    let recovered = restarted.get(&job.id).unwrap().unwrap();
    assert_eq!(recovered.status, WipeJobStatus::Failed);
    assert!(
        recovered
            .failure
            .as_deref()
            .unwrap()
            .contains("backend restarted")
    );
    assert!(recovered.verify_evidence());
    std::fs::remove_dir_all(directory).ok();
}
