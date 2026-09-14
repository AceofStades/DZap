use super::recovery_extract::{
    analyze_recovery_image_with_program, copy_tree, cryptsetup_open_arguments, parse_lsblk_volumes,
    photorec_arguments, testdisk_arguments,
};
use super::recovery_imaging::{register_recovery_control, request_recovery_cancel};
use super::recovery_jobs::{
    RecoveryJobStatus, RecoveryJobStore, RecoveryMethod, RecoveryResult, RescueMapSummary,
};
use super::recovery_jobs_test::ready_plan;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "dzap-extract-{name}-{}-{}",
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

fn image_complete_job(directory: &Path) -> (RecoveryJobStore, super::recovery_jobs::RecoveryJob) {
    let store = RecoveryJobStore::in_memory();
    let job = store.create(&ready_plan(directory, 1024)).unwrap();
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
    (store, job)
}

#[test]
fn volume_parser_assigns_stable_partition_ids_and_encryption_types() {
    let volumes = parse_lsblk_volumes(
        br#"{
          "blockdevices": [{
            "path": "/dev/loop7", "type": "loop", "fstype": null,
            "size": 4096, "label": null, "partn": null,
            "children": [
              {"path": "/dev/loop7p1", "type": "part", "fstype": "ext4", "size": "2048", "label": "Files", "partn": 1},
              {"path": "/dev/loop7p2", "type": "part", "fstype": "crypto_LUKS", "size": 1024, "label": null, "partn": "2"},
              {"path": "/dev/loop7p3", "type": "part", "fstype": "BitLocker", "size": 1024, "label": null, "partn": 3}
            ]
          }]
        }"#,
    )
    .unwrap();

    assert_eq!(volumes.len(), 4);
    assert_eq!(volumes[0].public.id, "whole-disk");
    assert_eq!(volumes[1].public.id, "partition-1");
    assert!(volumes[1].public.filesystem_copy_supported);
    assert_eq!(volumes[2].public.encryption.as_deref(), Some("luks"));
    assert_eq!(volumes[3].public.encryption.as_deref(), Some("bitlk"));
}

#[test]
fn photorec_arguments_use_scripted_whole_volume_search() {
    let arguments = photorec_arguments(
        OsStr::new("/dev/mapper/dzap-test"),
        Path::new("/safe/output"),
        Path::new("/safe/photorec.log"),
        Some("ext4"),
    );

    assert_eq!(
        arguments,
        vec![
            OsString::from("/log"),
            OsString::from("/logname"),
            OsString::from("/safe/photorec.log"),
            OsString::from("/d"),
            OsString::from("/safe/output"),
            OsString::from("/cmd"),
            OsString::from("/dev/mapper/dzap-test"),
            OsString::from("partition_none,options,mode_ext2,fileopt,everything,enable,search",),
        ]
    );
}

#[test]
fn testdisk_arguments_request_read_only_image_listing() {
    assert_eq!(
        testdisk_arguments(Path::new("/recovery/source.img")),
        vec![
            OsString::from("/list"),
            OsString::from("/recovery/source.img"),
        ]
    );
}

#[test]
fn encrypted_mapping_arguments_are_read_only_and_contain_no_secret() {
    for encryption in ["luks", "bitlk"] {
        let arguments = cryptsetup_open_arguments("/dev/loop7p2", encryption, "dzap-recovery-test");
        assert_eq!(arguments[0], "open");
        assert!(arguments.iter().any(|argument| argument == "--readonly"));
        assert!(arguments.iter().any(|argument| argument == "--key-file=-"));
        assert!(
            arguments
                .windows(2)
                .any(|pair| { pair == [OsString::from("--type"), OsString::from(encryption)] })
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "do-not-persist")
        );
    }
}

#[cfg(unix)]
#[test]
fn testdisk_analysis_is_logged_hashed_and_bound_to_job_evidence() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDirectory::new("testdisk");
    let program = directory.0.join("testdisk-fake");
    std::fs::write(
        &program,
        "#!/bin/sh\nprintf 'Disk image - partition table found\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let (store, job) = image_complete_job(&directory.0);
    let stale_log = Path::new(&job.job_directory).join("testdisk-1.log");
    std::fs::write(&stale_log, b"incomplete prior invocation").unwrap();

    let analysis = analyze_recovery_image_with_program(&program, &job, &store).unwrap();

    assert!(analysis.successful);
    assert!(analysis.summary.contains("partition table found"));
    assert!(
        !std::fs::read_to_string(&stale_log)
            .unwrap()
            .contains("incomplete prior invocation")
    );
    assert_eq!(analysis.log_sha256.len(), 64);
    assert!(Path::new(&analysis.log_path).is_file());
    let updated = store.get(&job.id).unwrap().unwrap();
    assert_eq!(updated.testdisk_analysis, Some(analysis));
    assert_eq!(
        updated.events.last().unwrap().event_type,
        "testdisk_analyzed"
    );
    assert!(updated.verify_evidence());
}

#[cfg(unix)]
#[test]
fn filesystem_copy_skips_symlinks_and_hashes_every_regular_file() {
    use std::os::unix::fs::symlink;

    let directory = TestDirectory::new("copy");
    let source = directory.0.join("mounted-source");
    std::fs::create_dir(&source).unwrap();
    std::fs::create_dir(source.join("folder")).unwrap();
    std::fs::write(source.join("hello.txt"), b"hello").unwrap();
    std::fs::write(source.join("folder/world.bin"), b"world!").unwrap();
    symlink("/etc/passwd", source.join("unsafe-link")).unwrap();

    let (store, image_job) = image_complete_job(&directory.0);
    let destination = Path::new(&image_job.job_directory).join("filesystem_copy-files-1");
    std::fs::create_dir(&destination).unwrap();
    let active = store
        .begin_recovery(
            &image_job.id,
            RecoveryMethod::FilesystemCopy,
            destination.display().to_string(),
        )
        .unwrap();
    let manifest = Path::new(&active.job_directory).join("filesystem_copy-1.manifest.jsonl");
    let control = register_recovery_control(&active.id).unwrap();
    let (progress_tx, _progress_rx) = tokio::sync::mpsc::unbounded_channel();

    let summary = copy_tree(
        &source,
        &destination,
        &manifest,
        &store,
        &active.id,
        &progress_tx,
        &control,
    )
    .unwrap();

    assert_eq!(summary.file_count, 2);
    assert_eq!(summary.bytes, 11);
    assert_eq!(summary.skipped_entries, 1);
    assert_eq!(summary.sha256.len(), 64);
    assert_eq!(
        std::fs::read(destination.join("hello.txt")).unwrap(),
        b"hello"
    );
    assert_eq!(
        std::fs::read(destination.join("folder/world.bin")).unwrap(),
        b"world!"
    );
    assert!(!destination.join("unsafe-link").exists());
    let records: Vec<serde_json::Value> = std::fs::read_to_string(&manifest)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|record| record["sha256"].as_str().unwrap().len() == 64)
    );

    let completed = store
        .complete_recovery(
            &active.id,
            RecoveryResult {
                method: RecoveryMethod::FilesystemCopy,
                output_directory: destination.display().to_string(),
                manifest_path: manifest.display().to_string(),
                manifest_sha256: summary.sha256,
                recovered_file_count: summary.file_count,
                recovered_bytes: summary.bytes,
                skipped_entries: summary.skipped_entries,
            },
        )
        .unwrap();
    assert_eq!(completed.status, RecoveryJobStatus::Completed);
    assert!(completed.verify_evidence());
}

#[test]
fn filesystem_copy_honors_cancellation_before_writing_files() {
    let directory = TestDirectory::new("cancel");
    let source = directory.0.join("source");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("file"), b"must not be copied").unwrap();
    let (store, image_job) = image_complete_job(&directory.0);
    let destination = Path::new(&image_job.job_directory).join("filesystem_copy-files-1");
    let manifest = Path::new(&image_job.job_directory).join("filesystem_copy-1.manifest.jsonl");
    std::fs::create_dir(&destination).unwrap();
    let active = store
        .begin_recovery(
            &image_job.id,
            RecoveryMethod::FilesystemCopy,
            destination.display().to_string(),
        )
        .unwrap();
    let control = register_recovery_control(&active.id).unwrap();
    request_recovery_cancel(&active.id).unwrap();
    let (progress_tx, _progress_rx) = tokio::sync::mpsc::unbounded_channel();

    let result = copy_tree(
        &source,
        &destination,
        &manifest,
        &store,
        &active.id,
        &progress_tx,
        &control,
    );

    assert!(result.is_err());
    assert!(result.err().unwrap().contains("cancelled"));
    assert!(!destination.join("file").exists());
}
