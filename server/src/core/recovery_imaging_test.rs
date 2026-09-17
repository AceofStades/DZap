use super::recovery_imaging::{
    ddrescue_arguments, parse_ddrescue_map, register_recovery_control, request_recovery_pause,
    run_ddrescue_with_program,
};
use super::recovery_jobs::{RecoveryJobStatus, RecoveryJobStore};
use super::recovery_jobs_test::ready_plan;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dzap-imaging-{name}-{}-{}",
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

#[test]
fn map_parser_accounts_for_rescued_bad_and_pending_ranges() {
    let summary = parse_ddrescue_map(
        "# Mapfile\n0x0000 0x0200 +\n0x0200 0x0100 -\n0x0300 0x0080 ?\n0x0380 0x0040 *\n",
        1024,
    )
    .unwrap();

    assert_eq!(summary.rescued_bytes, 512);
    assert_eq!(summary.unreadable_bytes, 256);
    assert_eq!(summary.pending_bytes, 256);
    assert_eq!(summary.total_bytes, 1024);
    assert_eq!(summary.progress_percent(), 75.0);
}

#[test]
fn ddrescue_arguments_bind_source_image_and_map_without_force() {
    let directory = TestDirectory::new("arguments");
    let store = RecoveryJobStore::in_memory();
    let job = store.create(&ready_plan(&directory.0, 1024)).unwrap();

    let arguments = ddrescue_arguments(&job);

    assert_eq!(
        arguments,
        vec![
            OsString::from("--no-scrape"),
            OsString::from("--retry-passes=3"),
            OsString::from("--sparse"),
            OsString::from(&job.source_device_path),
            OsString::from(&job.image_path),
            OsString::from(&job.map_path),
        ]
    );
    assert!(!arguments.iter().any(|argument| argument == "--force"));
}

#[cfg(unix)]
fn fake_ddrescue(directory: &Path, name: &str, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = directory.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn successful_ddrescue_process_completes_the_image_job() {
    let directory = TestDirectory::new("success");
    let program = fake_ddrescue(
        &directory.0,
        "ddrescue-success",
        "printf '0x0000 0x0400 +\\n' > \"$6\"\nprintf 'recovered' > \"$5\"",
    );
    let store = RecoveryJobStore::in_memory();
    let job = store.create(&ready_plan(&directory.0, 1024)).unwrap();
    let control = register_recovery_control(&job.id).unwrap();
    let (progress_tx, _progress_rx) = tokio::sync::mpsc::unbounded_channel();

    let completed =
        run_ddrescue_with_program(&program, job, &store, &progress_tx, &control).unwrap();

    assert_eq!(completed.status, RecoveryJobStatus::ImageComplete);
    assert_eq!(completed.map_summary.rescued_bytes, 1024);
    assert_eq!(completed.map_summary.pending_bytes, 0);
    assert!(completed.verify_evidence());
}

#[cfg(unix)]
#[test]
fn failed_ddrescue_process_keeps_a_resumable_job() {
    let directory = TestDirectory::new("failure");
    let program = fake_ddrescue(
        &directory.0,
        "ddrescue-failure",
        "printf '0x0000 0x0200 +\\n0x0200 0x0200 ?\\n' > \"$6\"\necho 'device read error' >&2\nexit 1",
    );
    let store = RecoveryJobStore::in_memory();
    let job = store.create(&ready_plan(&directory.0, 1024)).unwrap();
    let control = register_recovery_control(&job.id).unwrap();
    let (progress_tx, _progress_rx) = tokio::sync::mpsc::unbounded_channel();

    let paused = run_ddrescue_with_program(&program, job, &store, &progress_tx, &control).unwrap();

    assert_eq!(paused.status, RecoveryJobStatus::Paused);
    assert_eq!(paused.map_summary.pending_bytes, 512);
    assert!(Path::new(&paused.map_path).is_file());
    assert!(paused.last_message.contains("device read error"));
    assert_eq!(
        store.resume(&paused.id).unwrap().status,
        RecoveryJobStatus::Imaging
    );
}

#[test]
fn duplicate_registration_does_not_discard_the_live_control() {
    let job_id = format!("recovery-{:032x}", rand::random::<u128>());
    let guard = register_recovery_control(&job_id).unwrap();

    assert!(register_recovery_control(&job_id).is_err());
    assert!(request_recovery_pause(&job_id).is_ok());
    drop(guard);
    assert!(request_recovery_pause(&job_id).is_err());
}
