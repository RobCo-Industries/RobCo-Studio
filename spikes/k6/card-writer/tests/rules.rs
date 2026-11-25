//! The refusal rules on recorded disk lists. No test here touches a live device.

use card_writer::model::DiskList;
use card_writer::rules::{Policy, Refusal, decide};
use card_writer::{linux, macos, windows};

const MIB: u64 = 1024 * 1024;

fn fixture(name: &str) -> String {
    let p = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn refusals(list: &DiskList, dev: &str, policy: &Policy) -> Vec<Refusal> {
    match decide(list, dev, 64 * MIB, policy) {
        Ok(a) => panic!("{dev} was approved as {}", a.path()),
        Err(w) => w,
    }
}

fn approves(list: &DiskList, dev: &str, policy: &Policy) {
    if let Err(w) = decide(list, dev, 64 * MIB, policy) {
        panic!("{dev} refused: {w:?}");
    }
}

fn virt() -> Policy {
    Policy {
        allow_virtual: true,
        ..Policy::default()
    }
}

// Linux

#[test]
fn linux_laptop() {
    let l = linux::parse(&fixture("linux-laptop.json"));
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let p = Policy::default();
    // Root sits under LUKS and LVM, three levels down.
    let r = refusals(&l, "/dev/nvme0n1", &p);
    assert!(r.contains(&Refusal::HoldsRunningSystem));
    assert!(r.contains(&Refusal::FixedInternal));
    assert!(
        matches!(&r[..], [_, Refusal::MountedSystemVolume(v), ..] if v.contains(&"[SWAP]".to_string()))
    );
    // A card with its volume mounted: unmount first.
    assert_eq!(
        refusals(&l, "/dev/sda", &p),
        vec![Refusal::VolumesMounted(vec!["/media/USER/CARD".into()])]
    );
    approves(&l, "/dev/sdb", &p);
    // A partition is not a whole disk.
    assert_eq!(refusals(&l, "/dev/sdb1", &p), vec![Refusal::NotInList]);
    // A 1 TB USB SSD: over the cap. RM=0 and HOTPLUG=1 disagree, so removable is unknown.
    assert!(matches!(
        refusals(&l, "/dev/sdc", &p)[..],
        [Refusal::RemovableUnknown, Refusal::TooLarge { .. }]
            | [Refusal::TooLarge { .. }, Refusal::RemovableUnknown]
    ));
    // The built-in reader reports RM=0, so it is refused. A known gap.
    assert_eq!(
        refusals(&l, "/dev/mmcblk0", &p),
        vec![Refusal::NotRemovable]
    );
    // The CD drive is not listed at all.
    assert_eq!(refusals(&l, "/dev/sr0", &p), vec![Refusal::NotInList]);
    // Loop devices only with the CI switch, and never a mounted one.
    assert_eq!(
        refusals(&l, "/dev/loop0", &p),
        vec![Refusal::VirtualNotAllowed]
    );
    approves(&l, "/dev/loop0", &virt());
    let r = refusals(&l, "/dev/loop1", &virt());
    assert!(
        r.contains(&Refusal::VolumesMounted(vec!["/snap/core22/1".into()])),
        "{r:?}"
    );
}

#[test]
fn linux_unmounted_holders_are_in_use() {
    // A card that is an active LVM member, and one that is an md RAID member. No mount point.
    let text = r#"{"blockdevices": [
      {"name": "/dev/nvme0n1", "type": "disk", "size": 512110190592, "rm": false, "hotplug": false, "tran": "nvme", "mountpoints": [null],
       "children": [{"name": "/dev/nvme0n1p1", "type": "part", "mountpoints": ["/"]}]},
      {"name": "/dev/sdb", "type": "disk", "size": 31914983424, "rm": true, "hotplug": true, "tran": "usb", "log-sec": 512, "mountpoints": [null],
       "children": [{"name": "/dev/sdb1", "type": "part", "mountpoints": [null],
         "children": [{"name": "/dev/mapper/vg-data", "type": "lvm", "mountpoints": [null]}]}]},
      {"name": "/dev/sdc", "type": "disk", "size": 31914983424, "rm": true, "hotplug": true, "tran": "usb", "log-sec": 512, "mountpoints": [null],
       "children": [{"name": "/dev/md0", "type": "raid1", "mountpoints": [null]}]}
    ]}"#;
    let l = linux::parse(text);
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let p = Policy::default();
    assert_eq!(
        refusals(&l, "/dev/sdb", &p),
        vec![Refusal::VolumesMounted(vec![
            "in use by lvm /dev/mapper/vg-data".into()
        ])]
    );
    assert_eq!(
        refusals(&l, "/dev/sdc", &p),
        vec![Refusal::VolumesMounted(vec![
            "in use by raid1 /dev/md0".into()
        ])]
    );
}

#[test]
fn sector_size_must_be_known_and_512_or_4096() {
    // The writer pads to the sector size. 8192 is one it does not know; none is unknown.
    let text = r#"{"blockdevices": [
      {"name": "/dev/nvme0n1", "type": "disk", "size": 512110190592, "rm": false, "hotplug": false, "tran": "nvme", "log-sec": 512, "mountpoints": ["/"]},
      {"name": "/dev/sdb", "type": "disk", "size": 31914983424, "rm": true, "hotplug": true, "tran": "usb", "log-sec": 8192, "mountpoints": [null]},
      {"name": "/dev/sdc", "type": "disk", "size": 31914983424, "rm": true, "hotplug": true, "tran": "usb", "mountpoints": [null]},
      {"name": "/dev/sdd", "type": "disk", "size": 67109888, "rm": true, "hotplug": true, "tran": "usb", "log-sec": 4096, "mountpoints": [null]},
      {"name": "/dev/sde", "type": "disk", "size": 67109888, "rm": true, "hotplug": true, "tran": "usb", "log-sec": 512, "mountpoints": [null]}
    ]}"#;
    let l = linux::parse(text);
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let p = Policy::default();
    assert_eq!(
        refusals(&l, "/dev/sdb", &p),
        vec![Refusal::SectorSizeUnsupported(Some(8192))]
    );
    assert_eq!(
        refusals(&l, "/dev/sdc", &p),
        vec![Refusal::SectorSizeUnsupported(None)]
    );
    // 64 MiB plus 512 bytes, on a disk of 64 MiB plus 1024: fits padded to 512, not to 4096.
    let len = 64 * MIB + 512;
    assert!(matches!(
        decide(&l, "/dev/sdd", len, &p).err().as_deref(),
        Some([Refusal::ImageLargerThanDisk { .. }])
    ));
    let a = decide(&l, "/dev/sde", len, &p).unwrap();
    assert_eq!(a.block_size(), 512);
}

#[test]
fn linux_booted_from_usb() {
    let l = linux::parse(&fixture("linux-usb-boot.json"));
    let r = refusals(&l, "/dev/sda", &Policy::default());
    assert!(r.contains(&Refusal::HoldsRunningSystem));
    approves(&l, "/dev/sdb", &Policy::default());
}

#[test]
fn linux_root_not_found_refuses_everything() {
    let l = linux::parse(&fixture("linux-no-root.json"));
    assert!(!l.system_located);
    assert_eq!(
        refusals(&l, "/dev/sda", &Policy::default()),
        vec![Refusal::SystemNotLocated]
    );
}

#[test]
fn linux_old_lsblk_and_unknowns() {
    let l = linux::parse(&fixture("linux-old-lsblk.json"));
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let p = Policy::default();
    assert!(refusals(&l, "/dev/sda", &p).contains(&Refusal::HoldsRunningSystem));
    approves(&l, "/dev/sdb", &p);
    assert_eq!(
        refusals(&l, "/dev/sdc", &p),
        vec![Refusal::BusUnknown(String::new())]
    );
    assert_eq!(
        refusals(&l, "/dev/sdd", &p),
        vec![Refusal::RemovableUnknown]
    );
    assert_eq!(refusals(&l, "/dev/sde", &p), vec![Refusal::SizeUnknown]);
    assert_eq!(refusals(&l, "/dev/sdf", &p), vec![Refusal::Ambiguous]);
    assert!(matches!(
        refusals(&l, "/dev/sdg", &p)[..],
        [Refusal::MountedSystemVolume(_), Refusal::VolumesMounted(_)]
    ));
}

#[test]
fn linux_broken_output_refuses() {
    for text in ["", "{\"blockdevices\": [", "{}", "[1,2]"] {
        let l = linux::parse(text);
        assert!(!l.complete(), "{text:?}");
        let r = refusals(&l, "/dev/sdb", &Policy::default());
        assert!(matches!(r[0], Refusal::ListIncomplete(_)), "{text:?}");
    }
}

// macOS

#[test]
fn macos_card() {
    let l = macos::parse(&fixture("macos-card.json"));
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let p = Policy::default();
    // The physical disk under the system container.
    let r = refusals(&l, "/dev/disk0", &p);
    assert!(r.contains(&Refusal::HoldsRunningSystem));
    assert!(r.contains(&Refusal::FixedInternal));
    // The container itself.
    let r = refusals(&l, "/dev/disk3", &p);
    assert!(r.contains(&Refusal::HoldsRunningSystem) && r.contains(&Refusal::Synthesized));
    // The other containers on the same disk carry system volumes.
    assert!(matches!(
        refusals(&l, "/dev/disk1", &p)[0],
        Refusal::MountedSystemVolume(_)
    ));
    // The card with its FAT volume mounted, then the built-in SD slot.
    assert_eq!(
        refusals(&l, "/dev/disk4", &p),
        vec![Refusal::VolumesMounted(vec!["/Volumes/CARD".into()])]
    );
    approves(&l, "/dev/disk6", &p);
    // A disk image says removable and external: only the bus tells it apart.
    assert_eq!(
        refusals(&l, "/dev/disk5", &p),
        vec![Refusal::VirtualNotAllowed]
    );
    approves(&l, "/dev/disk5", &virt());
    // A reader behind Thunderbolt is a bus the rules do not know.
    assert_eq!(
        refusals(&l, "/dev/disk7", &p),
        vec![Refusal::BusUnknown("Thunderbolt".into())]
    );
    // The raw node is not the name in the list.
    assert_eq!(refusals(&l, "/dev/rdisk6", &p), vec![Refusal::NotInList]);
}

#[test]
fn macos_booted_from_usb() {
    let l = macos::parse(&fixture("macos-external-boot.json"));
    assert!(l.complete(), "{:?}", l.problems);
    let r = refusals(&l, "/dev/disk4", &Policy::default());
    assert!(r.contains(&Refusal::HoldsRunningSystem), "{r:?}");
    let r = refusals(&l, "/dev/disk5", &Policy::default());
    assert!(r.contains(&Refusal::Synthesized), "{r:?}");
}

#[test]
fn macos_partial_list_refuses_everything() {
    for name in ["macos-info-failed.json", "macos-node-mismatch.json"] {
        let l = macos::parse(&fixture(name));
        assert!(!l.complete(), "{name}");
        let r = refusals(&l, "/dev/disk6", &Policy::default());
        assert!(matches!(r[0], Refusal::ListIncomplete(_)), "{name}: {r:?}");
    }
    let l =
        macos::parse(r#"{"list": {"WholeDisks": [], "AllDisksAndPartitions": []}, "info": {}}"#);
    assert!(!l.complete() && !l.system_located);
}

// Windows

#[test]
fn windows_laptop() {
    let l = windows::parse(&fixture("windows-laptop.json"));
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let p = Policy::default();
    let r = refusals(&l, r"\\.\PhysicalDrive0", &p);
    assert!(r.contains(&Refusal::HoldsRunningSystem) && r.contains(&Refusal::FixedInternal));
    assert_eq!(
        refusals(&l, r"\\.\PhysicalDrive1", &p),
        vec![Refusal::VolumesMounted(vec!["E:".into()])]
    );
    // Offline VHD: no mounted volume, virtual only with the CI switch.
    assert_eq!(
        refusals(&l, r"\\.\PhysicalDrive2", &p),
        vec![Refusal::VirtualNotAllowed]
    );
    approves(&l, r"\\.\PhysicalDrive2", &virt());
    // External SSD: fixed media.
    assert!(refusals(&l, r"\\.\PhysicalDrive3", &p).contains(&Refusal::NotRemovable));
    // Case and slashes do not matter on Windows.
    approves(&l, "//./physicaldrive4", &p);
    assert_eq!(
        refusals(&l, r"\\.\PhysicalDrive9", &p),
        vec![Refusal::NotInList]
    );
    assert_eq!(
        refusals(&l, r"\\.\PhysicalDrive4 & del", &p),
        vec![Refusal::BadPath]
    );
}

#[test]
fn windows_volumes_with_only_a_guid_path() {
    let l = windows::parse(&fixture("windows-guid-only.json"));
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let p = Policy::default();
    // An EFI system partition with no letter, on a card reader. Not the active one.
    let r = refusals(&l, r"\\.\PhysicalDrive1", &p);
    assert!(
        r.iter().any(|w| matches!(w, Refusal::MountedSystemVolume(v)
            if v == &vec![r"\\?\Volume{00000000-0000-0000-0000-000000000011}\".to_string()])),
        "{r:?}"
    );
    assert!(
        r.iter().any(|w| matches!(w, Refusal::VolumesMounted(_))),
        "{r:?}"
    );
    // A FAT data volume reachable only through its GUID path is still in use.
    assert_eq!(
        refusals(&l, r"\\.\PhysicalDrive2", &p),
        vec![Refusal::VolumesMounted(vec![
            r"\\?\Volume{00000000-0000-0000-0000-000000000012}\".into()
        ])]
    );
    // A partition with no access path at all has no live volume.
    approves(&l, r"\\.\PhysicalDrive3", &p);
}

#[test]
fn windows_to_go_and_single_objects() {
    // PowerShell wrote objects, not arrays. The system runs from a USB stick.
    let l = windows::parse(&fixture("windows-to-go.json"));
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let r = refusals(&l, r"\\.\PhysicalDrive1", &Policy::default());
    assert!(r.contains(&Refusal::HoldsRunningSystem), "{r:?}");
}

#[test]
fn windows_hostile() {
    let l = windows::parse(&fixture("windows-hostile.json"));
    let p = Policy::default();
    // The incomplete Drives list for disk 4 does not make the list incomplete, but disk 4
    // has no media type, so removable is unknown.
    assert_eq!(
        refusals(&l, r"\\.\PhysicalDrive4", &p),
        vec![Refusal::RemovableUnknown]
    );
    // BusType 7 but text SATA: the two disagree, so the bus is unknown.
    assert!(matches!(
        refusals(&l, r"\\.\PhysicalDrive1", &p)[..],
        [Refusal::BusUnknown(_)]
    ));
    assert_eq!(
        refusals(&l, r"\\.\PhysicalDrive2", &p),
        vec![Refusal::SystemUnknown]
    );
    assert_eq!(
        refusals(&l, r"\\.\PhysicalDrive3", &p),
        vec![Refusal::SizeUnknown]
    );
}

#[test]
fn windows_broken_output_refuses() {
    for text in ["", "null", "{\"Disks\": []}", "{\"SystemDrive\": \"C:\"}"] {
        let l = windows::parse(text);
        assert!(!l.complete(), "{text:?}");
    }
}

#[test]
fn size_cap_and_image_size() {
    let l = linux::parse(&fixture("linux-laptop.json"));
    let small = Policy {
        max_size: 16 * 1024 * MIB,
        ..Policy::default()
    };
    assert!(matches!(
        refusals(&l, "/dev/sdb", &small)[..],
        [Refusal::TooLarge { .. }]
    ));
    let r = decide(&l, "/dev/loop0", 65 * MIB, &virt()).err();
    assert!(matches!(
        r.as_deref(),
        Some([Refusal::ImageLargerThanDisk { .. }])
    ));
}

#[test]
fn second_look_must_see_the_same_disk() {
    let l = windows::parse(&fixture("windows-laptop.json"));
    let p = Policy::default();
    let a = decide(&l, r"\\.\PhysicalDrive4", 64 * MIB, &p).unwrap();
    assert_eq!(a.image_len(), 64 * MIB);
    let b = decide(&l, r"\\.\PhysicalDrive4", 64 * MIB, &p).unwrap();
    assert!(a.same_as(&b));
    // The reader now holds a card of another size under the same path.
    let mut l2 = l.clone();
    l2.disks
        .iter_mut()
        .find(|d| d.path.ends_with('4'))
        .unwrap()
        .size = Some(15_931_539_456);
    let c = decide(&l2, r"\\.\PhysicalDrive4", 64 * MIB, &p).unwrap();
    assert!(!a.same_as(&c));
    // Another image length is another approval.
    let d = decide(&l, r"\\.\PhysicalDrive4", 32 * MIB, &p).unwrap();
    assert!(!a.same_as(&d));
}

#[test]
fn second_look_must_see_the_same_sector_size() {
    let l = windows::parse(&fixture("windows-laptop.json"));
    let p = Policy::default();
    let a = decide(&l, r"\\.\PhysicalDrive4", 64 * MIB, &p).unwrap();
    // Same path, same size, another sector size.
    let mut l2 = l.clone();
    let d = l2.disks.iter_mut().find(|d| d.path.ends_with('4')).unwrap();
    d.block_size = Some(if a.block_size() == 512 { 4096 } else { 512 });
    let c = decide(&l2, r"\\.\PhysicalDrive4", 64 * MIB, &p).unwrap();
    assert!(!a.same_as(&c));
}

/// One recorded Windows disk with the given BusType number and text.
fn windows_bus(num: Option<u64>, text: &str) -> DiskList {
    let n = num.map_or("null".to_string(), |n| n.to_string());
    windows::parse(&format!(
        r#"{{"SystemDrive": "C:",
 "Disks": [{{"Number": 0, "FriendlyName": "System", "Size": 512000000000, "BusType": 17, "BusTypeText": "NVMe", "IsBoot": true, "IsSystem": true, "IsOffline": false, "LogicalSectorSize": 512}},
           {{"Number": 3, "FriendlyName": "Test", "Size": 1073741824, "BusType": {n}, "BusTypeText": "{text}", "IsBoot": false, "IsSystem": false, "IsOffline": false, "LogicalSectorSize": 512}}],
 "Partitions": [{{"DiskNumber": 0, "DriveLetter": "C", "IsBoot": true, "IsSystem": false, "AccessPaths": ["C:\\"]}}],
 "Drives": [{{"Index": 0, "MediaType": "Fixed hard disk media"}}, {{"Index": 3, "MediaType": "Fixed hard disk media"}}]}}"#
    ))
}

#[test]
fn windows_virtual_bus_is_virtual() {
    for (num, text) in [
        (Some(14), "Virtual"),
        (Some(14), ""),
        (None, "Virtual"),
        (Some(15), "File Backed Virtual"),
    ] {
        let l = windows_bus(num, text);
        assert!(l.complete(), "{:?}", l.problems);
        approves(&l, r"\\.\PhysicalDrive3", &virt());
        assert_eq!(
            refusals(&l, r"\\.\PhysicalDrive3", &Policy::default()),
            vec![Refusal::VirtualNotAllowed],
            "{num:?} {text}"
        );
    }
}

#[test]
fn windows_unknown_and_reserved_bus_is_refused_as_unknown() {
    for (num, text) in [
        (Some(0), "Unknown"),
        (Some(18), ""),
        (Some(18), "SCM"),
        (Some(99), "Something New"),
        (None, "Something New"),
        (None, ""),
        // Number and text disagree.
        (Some(7), "Virtual"),
    ] {
        let l = windows_bus(num, text);
        let r = refusals(&l, r"\\.\PhysicalDrive3", &virt());
        assert!(
            r.iter().any(|x| matches!(x, Refusal::BusUnknown(_))),
            "{num:?} {text}: {r:?}"
        );
    }
}

#[test]
fn windows_documented_fixed_buses_are_internal() {
    let fixed = [
        (1, "SCSI"),
        (2, "ATAPI"),
        (3, "ATA"),
        (4, "1394"),
        (5, "SSA"),
        (6, "Fibre Channel"),
        (8, "RAID"),
        (9, "iSCSI"),
        (10, "SAS"),
        (11, "SATA"),
        (16, "Storage Spaces"),
        (17, "NVMe"),
    ];
    for (num, text) in fixed {
        let l = windows_bus(Some(num), text);
        assert!(
            refusals(&l, r"\\.\PhysicalDrive3", &virt()).contains(&Refusal::FixedInternal),
            "{num} {text}"
        );
    }
}

#[test]
fn a_sector_size_past_u32_is_not_truncated() {
    // 2^32 + 512 would read as 512 if cut to 32 bits.
    let mut v: serde_json::Value = serde_json::from_str(&fixture("windows-to-go.json")).unwrap();
    v["Disks"]["LogicalSectorSize"] = 4_294_967_808u64.into();
    let l = windows::parse(&v.to_string());
    assert_eq!(l.disks[0].block_size, None);
}

#[test]
fn windows_bus_number_and_text_must_agree() {
    let l = windows::parse(&fixture("windows-bus-disagree.json"));
    assert!(l.complete() && l.system_located, "{:?}", l.problems);
    let p = Policy::default();
    // Number 0, reserved 42, SATA 11, and an unparsable number, each with text USB.
    for n in [1, 2, 3, 5] {
        let dev = format!(r"\\.\PhysicalDrive{n}");
        assert!(
            matches!(refusals(&l, &dev, &p)[..], [Refusal::BusUnknown(_)]),
            "{dev}"
        );
    }
    // No number at all: the text decides.
    approves(&l, r"\\.\PhysicalDrive4", &p);
}
