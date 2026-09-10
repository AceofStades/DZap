use crate::core::drives::{
    DriveType, android_devices_from_adb, hdparm_output_is_frozen, storage_drives_from_lsblk,
};

#[test]
fn drive_type_serializes_to_frontend_strings() {
    assert_eq!(
        serde_json::to_value(DriveType::Hdd).unwrap(),
        serde_json::json!("HDD")
    );
    assert_eq!(
        serde_json::to_value(DriveType::Ssd).unwrap(),
        serde_json::json!("SATA SSD")
    );
    assert_eq!(
        serde_json::to_value(DriveType::Nvme).unwrap(),
        serde_json::json!("NVMe SSD")
    );
    assert_eq!(
        serde_json::to_value(DriveType::Usb).unwrap(),
        serde_json::json!("USB Drive")
    );
    assert_eq!(
        serde_json::to_value(DriveType::Unknown).unwrap(),
        serde_json::json!("Unknown")
    );
}

#[test]
fn lsblk_json_maps_devices_partitions_mounts_and_types() {
    let input = br#"{
        "blockdevices": [
            {"name":"loop0","type":"loop","size":4096,"mountpoints":[null]},
            {"name":"nbd0","model":" Network disk ","size":100,"rota":false,"type":"disk","mountpoints":[null],"tran":"usb"},
            {"name":"sda","model":" Spinning Disk  ","serial":" S123 ","wwn":" 0xabc ","size":200,"rota":true,"type":"disk","mountpoints":[null],"tran":"sata","maj:min":"8:0","children":[
                {"name":"sda1","size":150,"type":"part","mountpoints":["/"],"fstype":"ext4"},
                {"name":"sda2","size":50,"type":"part","mountpoints":[null]}
            ]},
            {"name":"nvme0n1","size":300,"rota":false,"type":"disk","mountpoints":[null]},
            {"name":"sdb","size":400,"rota":false,"type":"disk","mountpoints":["/media/data"]},
            {"name":"sdc","size":500,"rota":true,"type":"disk","mountpoints":[null],"tran":"usb"}
        ]
    }"#;
    let mut frozen_checks = Vec::new();

    let drives = storage_drives_from_lsblk(input, |path| {
        frozen_checks.push(path.to_string());
        Ok(path == "/dev/sdb")
    })
    .unwrap();

    assert_eq!(drives.len(), 5, "non-disk devices must be filtered out");
    assert_eq!(drives[0].name, "/dev/nbd0");
    assert_eq!(drives[0].model, "Network disk");
    assert_eq!(drives[0].drive_type, DriveType::Hdd, "nbd takes precedence");

    assert_eq!(drives[1].drive_type, DriveType::Hdd);
    assert_eq!(drives[1].serial, "S123");
    assert_eq!(drives[1].wwn, "0xabc");
    assert_eq!(drives[1].transport, "sata");
    assert_eq!(drives[1].major_minor, "8:0");
    assert_eq!(drives[1].identity().size_bytes, "200");
    assert!(drives[1].is_mounted);
    assert!(drives[1].is_os_drive);
    assert_eq!(drives[1].partitions.len(), 2);
    assert_eq!(drives[1].partitions[0].name, "/dev/sda1");
    assert_eq!(drives[1].partitions[0].size, "150");
    assert_eq!(drives[1].partitions[0].fs_type, "ext4");
    assert_eq!(drives[1].partitions[1].fs_type, "");

    assert_eq!(drives[2].drive_type, DriveType::Nvme);
    assert_eq!(drives[3].drive_type, DriveType::Ssd);
    assert!(drives[3].is_mounted);
    assert!(drives[3].is_frozen);
    assert_eq!(drives[4].drive_type, DriveType::Usb);
    assert_eq!(frozen_checks, ["/dev/sdb"]);
}

#[test]
fn malformed_lsblk_json_returns_contextual_error() {
    let err = storage_drives_from_lsblk(b"not json", |_| Ok(false)).unwrap_err();
    assert!(err.starts_with("failed to parse lsblk JSON:"), "got: {err}");
}

#[test]
fn adb_output_keeps_only_ready_devices_with_readable_models() {
    let output = "List of devices attached\nready-1\tdevice\noffline-1\toffline\nunauth-1\tunauthorized\nready-2 device\nextra device product:x\n";
    let mut requested = Vec::new();

    let devices = android_devices_from_adb(output, |serial| {
        requested.push(serial.to_string());
        match serial {
            "ready-1" => Some("  Pixel 9 Pro\n".to_string()),
            "ready-2" => None,
            _ => panic!("model lookup requested for unavailable device {serial}"),
        }
    });

    assert_eq!(requested, ["ready-1", "ready-2"]);
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "Pixel 9 Pro");
    assert_eq!(devices[0].model, "Pixel 9 Pro");
    assert_eq!(devices[0].serial, "ready-1");
    assert_eq!(devices[0].device_type, "Android");
}

#[test]
fn hdparm_security_section_distinguishes_frozen_from_not_frozen() {
    let frozen = "ATA device\nSecurity:\n\tnot enabled\n\tnot locked\n\tfrozen\n\tnot expired: security count\n";
    let thawed = "ATA device\nSecurity:\n\tnot enabled\n\tnot locked\n\tnot frozen\n";

    assert!(hdparm_output_is_frozen(frozen));
    assert!(!hdparm_output_is_frozen(thawed));
}
