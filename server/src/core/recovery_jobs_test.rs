use super::drives::DeviceIdentity;
use super::recovery_jobs::{RecoveryJobStatus, RecoveryJobStore, RecoveryMethod, RescueMapSummary};
use super::recovery_plan::{RecoveryDestination, RecoveryImagePlan, RecoveryPlanDecision};
use std::path::{Path, PathBuf};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dzap-recovery-{name}-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn identity(path: &str, size: u64) -> DeviceIdentity {
    DeviceIdentity {
        model: format!("Test {path}"),
        serial: format!("SERIAL-{path}"),
        wwn: format!("WWN-{path}"),
        size_bytes: size.to_string(),
        transport: "usb".to_string(),
        major_minor: path.to_string(),
    }
}

pub(super) fn ready_plan(destination_mount: &Path, size: u64) -> RecoveryImagePlan {
    let source_identity = identity("8:0", size);
    let destination_identity = identity("8:16", size * 4);
    RecoveryImagePlan {
        decision: RecoveryPlanDecision::Ready,
        source_device_path: "/dev/dzap-test-source".to_string(),
        source_identity: Some(source_identity),
        destination: Some(RecoveryDestination {
            drive_path: "/dev/dzap-test-target".to_string(),
            device_path: "/dev/dzap-test-target1".to_string(),
            device_major_minor: "8:17".to_string(),
            mount_path: Some(destination_mount.display().to_string()),
            filesystem: "ext4".to_string(),
            size_bytes: (size * 4).to_string(),
            drive_identity: destination_identity,
            filesystem_size_bytes: Some((size * 4).to_string()),
            available_bytes: Some((size * 3).to_string()),
            read_only: Some(false),
            capacity_error: None,
        }),
        image_size_bytes: size.to_string(),
        reserve_bytes: "0".to_string(),
        required_bytes: size.to_string(),
        output_directory: Some(
            destination_mount
                .join("DZap-Recovery")
                .display()
                .to_string(),
        ),
        checks: Vec::new(),
    }
}

#[test]
fn create_records_bound_artifacts_and_hash_chained_authorization() {
    let external = TestDirectory::new("create-external");
    let internal = TestDirectory::new("create-internal");
    let store = RecoveryJobStore::persistent(internal.0.clone()).unwrap();

    let job = store.create(&ready_plan(&external.0, 4096)).unwrap();

    assert_eq!(job.status, RecoveryJobStatus::Imaging);
    assert!(job.verify_evidence());
    assert_eq!(job.events.len(), 1);
    assert_eq!(job.events[0].event_type, "recovery_authorized");
    assert!(Path::new(&job.image_path).is_file());
    assert!(Path::new(&job.job_directory).join("job.json").is_file());
    assert!(internal.0.join(format!("{}.json", job.id)).is_file());
}

#[test]
fn progress_pause_resume_and_completion_survive_reload() {
    let external = TestDirectory::new("lifecycle-external");
    let internal = TestDirectory::new("lifecycle-internal");
    let store = RecoveryJobStore::persistent(internal.0.clone()).unwrap();
    let job = store.create(&ready_plan(&external.0, 1000)).unwrap();

    let progress = RescueMapSummary {
        rescued_bytes: 600,
        unreadable_bytes: 100,
        pending_bytes: 300,
        total_bytes: 1000,
    };
    let updated = store
        .update_progress(&job.id, progress, "Imaging test source")
        .unwrap();
    assert_eq!(updated.progress_percent, 70.0);
    assert_eq!(
        store.pause(&job.id, "operator pause").unwrap().status,
        RecoveryJobStatus::Paused
    );
    assert_eq!(
        store.resume(&job.id).unwrap().status,
        RecoveryJobStatus::Imaging
    );

    let complete = store
        .complete_image(
            &job.id,
            RescueMapSummary {
                rescued_bytes: 900,
                unreadable_bytes: 100,
                pending_bytes: 0,
                total_bytes: 1000,
            },
        )
        .unwrap();
    assert_eq!(complete.status, RecoveryJobStatus::ImageComplete);
    assert!(complete.verify_evidence());

    let reloaded = RecoveryJobStore::persistent(internal.0.clone())
        .unwrap()
        .get(&job.id)
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.status, RecoveryJobStatus::ImageComplete);
    assert_eq!(reloaded.evidence_hash, complete.evidence_hash);
}

#[test]
fn active_job_becomes_resumable_after_backend_restart() {
    let external = TestDirectory::new("restart-external");
    let internal = TestDirectory::new("restart-internal");
    let job = RecoveryJobStore::persistent(internal.0.clone())
        .unwrap()
        .create(&ready_plan(&external.0, 1024))
        .unwrap();

    let restarted = RecoveryJobStore::persistent(internal.0.clone())
        .unwrap()
        .get(&job.id)
        .unwrap()
        .unwrap();

    assert_eq!(restarted.status, RecoveryJobStatus::Paused);
    assert_eq!(
        restarted.events.last().unwrap().event_type,
        "imaging_interrupted"
    );
    assert!(restarted.verify_evidence());
    assert!(Path::new(&restarted.image_path).is_file());
}

#[test]
fn interrupted_file_recovery_preserves_image_for_another_method() {
    let external = TestDirectory::new("extract-restart-external");
    let internal = TestDirectory::new("extract-restart-internal");
    let store = RecoveryJobStore::persistent(internal.0.clone()).unwrap();
    let job = store.create(&ready_plan(&external.0, 1024)).unwrap();
    let job = store
        .complete_image(
            &job.id,
            RescueMapSummary {
                rescued_bytes: 1024,
                unreadable_bytes: 0,
                pending_bytes: 0,
                total_bytes: 1024,
            },
        )
        .unwrap();
    let first_output = Path::new(&job.job_directory).join("filesystem_copy-files-1");
    store
        .begin_recovery(
            &job.id,
            RecoveryMethod::FilesystemCopy,
            first_output.display().to_string(),
        )
        .unwrap();

    let restarted_store = RecoveryJobStore::persistent(internal.0.clone()).unwrap();
    let interrupted = restarted_store.get(&job.id).unwrap().unwrap();
    assert_eq!(interrupted.status, RecoveryJobStatus::Failed);
    assert!(interrupted.verify_evidence());
    assert!(Path::new(&interrupted.image_path).is_file());

    let second_output = Path::new(&job.job_directory).join("photorec-files-2.1");
    let retried = restarted_store
        .begin_recovery(
            &job.id,
            RecoveryMethod::Photorec,
            second_output.display().to_string(),
        )
        .unwrap();
    assert_eq!(retried.status, RecoveryJobStatus::Extracting);
    assert_eq!(retried.recovery_method, Some(RecoveryMethod::Photorec));
    assert!(retried.verify_evidence());
}

#[test]
fn missing_destination_mirror_still_updates_the_internal_failure_record() {
    let external = TestDirectory::new("missing-external-record");
    let internal = TestDirectory::new("retained-internal-record");
    let store = RecoveryJobStore::persistent(internal.0.clone()).unwrap();
    let job = store.create(&ready_plan(&external.0, 1024)).unwrap();
    std::fs::remove_dir_all(&job.job_directory).unwrap();

    let error = store
        .fail(&job.id, "destination disconnected while imaging")
        .unwrap_err();
    assert!(error.contains("failed to open"));
    let updated = store.get(&job.id).unwrap().unwrap();
    assert_eq!(updated.status, RecoveryJobStatus::Failed);
    assert!(updated.verify_evidence());

    let restarted = RecoveryJobStore::persistent(internal.0.clone()).unwrap();
    assert_eq!(
        restarted.get(&job.id).unwrap().unwrap().status,
        RecoveryJobStatus::Failed
    );
}

#[test]
fn tampered_persistent_evidence_is_rejected() {
    let external = TestDirectory::new("tamper-external");
    let internal = TestDirectory::new("tamper-internal");
    let store = RecoveryJobStore::persistent(internal.0.clone()).unwrap();
    let job = store.create(&ready_plan(&external.0, 1024)).unwrap();
    let record_path = internal.0.join(format!("{}.json", job.id));
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record_path).unwrap()).unwrap();
    record["sourceDevicePath"] = serde_json::json!("/dev/replaced-source");
    std::fs::write(&record_path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();

    let result = RecoveryJobStore::persistent(internal.0.clone());

    assert!(result.is_err());
    assert!(
        result
            .err()
            .unwrap()
            .contains("evidence verification failed")
    );
}

#[cfg(unix)]
#[test]
fn symlink_output_root_is_rejected() {
    use std::os::unix::fs::symlink;

    let parent = TestDirectory::new("symlink");
    let physical = parent.0.join("physical");
    let linked = parent.0.join("linked");
    std::fs::create_dir(&physical).unwrap();
    symlink(&physical, &linked).unwrap();

    let mut plan = ready_plan(&parent.0, 1024);
    plan.output_directory = Some(linked.display().to_string());
    let result = RecoveryJobStore::in_memory().create(&plan);

    assert!(result.is_err());
    assert!(result.err().unwrap().contains("not a physical directory"));
}
