use server::core::drives::storage_drives_from_lsblk;

#[test]
fn nested_block_topology_marks_active_dependencies_and_mounts() {
    let input = br#"{
        "blockdevices": [{
            "name": "sda",
            "model": "Physical disk",
            "size": 1000000,
            "rota": true,
            "type": "disk",
            "mountpoints": [null],
            "children": [{
                "name": "sda1",
                "size": 1000000,
                "type": "part",
                "mountpoints": [null],
                "children": [{
                    "name": "dm-0",
                    "type": "crypt",
                    "mountpoints": [null],
                    "children": [{
                        "name": "vg-root",
                        "type": "lvm",
                        "mountpoints": ["/"]
                    }]
                }]
            }]
        }]
    }"#;

    let drives = storage_drives_from_lsblk(input, |_| Ok(false)).unwrap();
    let drive = &drives[0];

    assert!(drive.is_mounted);
    assert!(drive.is_os_drive);
    assert_eq!(
        drive
            .active_dependencies
            .iter()
            .map(|dependency| (dependency.name.as_str(), dependency.device_type.as_str()))
            .collect::<Vec<_>>(),
        [("/dev/dm-0", "crypt"), ("/dev/vg-root", "lvm")]
    );
}

#[test]
fn raid_descendant_is_recorded_once_in_repeated_topology() {
    let input = br#"{
        "blockdevices": [{
            "name": "sdb",
            "size": 1000000,
            "rota": true,
            "type": "disk",
            "mountpoints": [null],
            "children": [
                {"name":"sdb1","type":"part","mountpoints":[null],"children":[
                    {"name":"md0","type":"raid1","mountpoints":[null]}
                ]},
                {"name":"md0","type":"raid1","mountpoints":[null]}
            ]
        }]
    }"#;

    let drives = storage_drives_from_lsblk(input, |_| Ok(false)).unwrap();

    assert_eq!(drives[0].active_dependencies.len(), 1);
    assert_eq!(drives[0].active_dependencies[0].name, "/dev/md0");
    assert_eq!(drives[0].active_dependencies[0].device_type, "raid1");
}

#[test]
fn archiso_boot_mount_marks_the_live_usb_as_an_os_drive() {
    let input = br#"{
        "blockdevices": [{
            "name": "sdc",
            "model": "DZap boot USB",
            "size": 16000000000,
            "rota": false,
            "type": "disk",
            "mountpoints": [null],
            "tran": "usb",
            "children": [{
                "name": "sdc1",
                "size": 16000000000,
                "type": "part",
                "mountpoints": ["/run/archiso/bootmnt"],
                "fstype": "iso9660"
            }]
        }]
    }"#;

    let drives = storage_drives_from_lsblk(input, |_| Ok(false)).unwrap();

    assert!(drives[0].is_mounted);
    assert!(drives[0].is_os_drive);
}
