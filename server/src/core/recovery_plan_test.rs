use super::drives::{BlockDependency, Drive, DriveType};
use super::evidence_export::ExportDestination;
use super::recovery_plan::*;
use std::path::Path;

fn drive(path: &str, major_minor: &str, size: u64) -> Drive {
    Drive {
        name: path.to_string(),
        model: format!("Model {major_minor}"),
        serial: format!("SERIAL-{major_minor}"),
        wwn: format!("WWN-{major_minor}"),
        size: size.to_string(),
        transport: "usb".to_string(),
        major_minor: major_minor.to_string(),
        drive_type: DriveType::Usb,
        is_mounted: false,
        is_frozen: false,
        is_os_drive: false,
        active_dependencies: Vec::new(),
        partitions: Vec::new(),
    }
}

fn export_destination(drive: &Drive, device_path: &str) -> ExportDestination {
    ExportDestination {
        drive_path: drive.name.clone(),
        drive_major_minor: drive.major_minor.clone(),
        device_path: device_path.to_string(),
        device_major_minor: format!("{}1", drive.major_minor),
        mount_path: Some(format!("/mnt/{}", drive.major_minor.replace(':', "-"))),
        model: drive.model.clone(),
        serial: drive.serial.clone(),
        transport: drive.transport.clone(),
        filesystem: "ext4".to_string(),
        size_bytes: drive.size.clone(),
    }
}

fn destination(drive: &Drive, available_bytes: u64) -> RecoveryDestination {
    RecoveryDestination {
        drive_path: drive.name.clone(),
        device_path: format!("{}1", drive.name),
        device_major_minor: format!("{}1", drive.major_minor),
        mount_path: Some(format!("/mnt/{}", drive.major_minor.replace(':', "-"))),
        filesystem: "ext4".to_string(),
        size_bytes: drive.size.clone(),
        drive_identity: drive.identity(),
        filesystem_size_bytes: Some(drive.size.clone()),
        available_bytes: Some(available_bytes.to_string()),
        read_only: Some(false),
        capacity_error: None,
    }
}

fn request(source: &Drive, destination: RecoveryDestination) -> RecoveryPlanRequest {
    RecoveryPlanRequest {
        source_device_path: source.name.clone(),
        expected_source_identity: source.identity(),
        destination,
    }
}

fn check_status(plan: &RecoveryImagePlan, code: &str) -> RecoveryPlanCheckStatus {
    plan.checks
        .iter()
        .find(|check| check.code == code)
        .unwrap()
        .status
}

#[test]
fn destination_discovery_excludes_source_and_adds_capacity() {
    let source = drive("/dev/sda", "8:0", 1024);
    let target = drive("/dev/sdb", "8:16", 4096);
    let candidates = vec![
        export_destination(&source, "/dev/sda1"),
        export_destination(&target, "/dev/sdb1"),
    ];

    let destinations = recovery_destinations_from_candidates(
        &source.name,
        &[source.clone(), target.clone()],
        candidates,
        |_path: &Path| {
            Ok(FilesystemCapacity {
                size_bytes: 4096,
                available_bytes: 3072,
                read_only: false,
            })
        },
    )
    .unwrap();

    assert_eq!(destinations.len(), 1);
    assert_eq!(destinations[0].drive_identity, target.identity());
    assert_eq!(destinations[0].available_bytes.as_deref(), Some("3072"));
    assert_eq!(destinations[0].read_only, Some(false));
}

#[test]
fn ready_plan_binds_separate_identities_and_capacity() {
    let source = drive("/dev/sda", "8:0", 128 * 1024 * 1024);
    let target = drive("/dev/sdb", "8:16", 512 * 1024 * 1024);
    let destination = destination(&target, 400 * 1024 * 1024);
    let request = request(&source, destination.clone());

    let plan = build_image_plan(&request, Some(&source), Some(&destination), None, None);

    assert_eq!(plan.decision, RecoveryPlanDecision::Ready);
    assert_eq!(plan.source_identity, Some(source.identity()));
    assert_eq!(plan.destination, Some(destination));
    assert!(plan.output_directory.unwrap().ends_with("/DZap-Recovery"));
}

#[test]
fn plan_blocks_same_physical_drive_and_changed_identities() {
    let source = drive("/dev/sda", "8:0", 128 * 1024 * 1024);
    let mut destination = destination(&source, 400 * 1024 * 1024);
    destination.device_path = "/dev/sda2".to_string();
    let mut request = request(&source, destination.clone());
    request.expected_source_identity.serial = "REPLACED-SOURCE".to_string();
    request.destination.drive_identity.serial = "REPLACED-TARGET".to_string();
    request.destination.filesystem = "exfat".to_string();

    let plan = build_image_plan(&request, Some(&source), Some(&destination), None, None);

    assert_eq!(plan.decision, RecoveryPlanDecision::Blocked);
    assert_eq!(
        check_status(&plan, "source_identity"),
        RecoveryPlanCheckStatus::Blocked
    );
    assert_eq!(
        check_status(&plan, "destination_identity"),
        RecoveryPlanCheckStatus::Blocked
    );
    assert_eq!(
        check_status(&plan, "destination_volume"),
        RecoveryPlanCheckStatus::Blocked
    );
    assert_eq!(
        check_status(&plan, "separate_destination"),
        RecoveryPlanCheckStatus::Blocked
    );
}

#[test]
fn plan_blocks_mounted_or_logically_active_source() {
    let mut source = drive("/dev/sda", "8:0", 128 * 1024 * 1024);
    source.is_mounted = true;
    source.active_dependencies.push(BlockDependency {
        name: "/dev/mapper/live".to_string(),
        device_type: "crypt".to_string(),
    });
    let target = drive("/dev/sdb", "8:16", 512 * 1024 * 1024);
    let destination = destination(&target, 400 * 1024 * 1024);
    let request = request(&source, destination.clone());

    let plan = build_image_plan(&request, Some(&source), Some(&destination), None, None);

    assert_eq!(plan.decision, RecoveryPlanDecision::Blocked);
    assert_eq!(
        check_status(&plan, "source_quiescent"),
        RecoveryPlanCheckStatus::Blocked
    );
}

#[test]
fn plan_blocks_insufficient_space_and_fat32_large_files() {
    let source = drive("/dev/sda", "8:0", 8 * 1024 * 1024 * 1024);
    let target = drive("/dev/sdb", "8:16", 16 * 1024 * 1024 * 1024);
    let mut destination = destination(&target, 1024 * 1024);
    destination.filesystem = "vfat".to_string();
    let request = request(&source, destination.clone());

    let plan = build_image_plan(&request, Some(&source), Some(&destination), None, None);

    assert_eq!(plan.decision, RecoveryPlanDecision::Blocked);
    assert_eq!(
        check_status(&plan, "image_file_size"),
        RecoveryPlanCheckStatus::Blocked
    );
    assert_eq!(
        check_status(&plan, "destination_capacity"),
        RecoveryPlanCheckStatus::Blocked
    );
}

#[test]
fn plan_blocks_unmounted_or_read_only_destination() {
    let source = drive("/dev/sda", "8:0", 128 * 1024 * 1024);
    let target = drive("/dev/sdb", "8:16", 512 * 1024 * 1024);
    let mut destination = destination(&target, 400 * 1024 * 1024);
    destination.mount_path = None;
    destination.available_bytes = None;
    destination.read_only = None;
    let request = request(&source, destination.clone());

    let plan = build_image_plan(&request, Some(&source), Some(&destination), None, None);

    assert_eq!(plan.decision, RecoveryPlanDecision::Blocked);
    assert_eq!(
        check_status(&plan, "destination_mounted"),
        RecoveryPlanCheckStatus::Blocked
    );
    assert_eq!(
        check_status(&plan, "destination_writable"),
        RecoveryPlanCheckStatus::Blocked
    );
}
