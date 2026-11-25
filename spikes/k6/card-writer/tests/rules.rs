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
    // A 1 TB USB SSD: over the cap.
    assert!(matches!(
        refusals(&l, "/dev/sdc", &p)[..],
        [Refusal::TooLarge { .. }]
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
      {"name": "/dev/sdb", "type": "disk", "size": 31914983424, "rm": true, "hotplug": true, "tran": "usb", "mountpoints": [null],
       "children": [{"name": "/dev/sdb1", "type": "part", "mountpoints": [null],
         "children": [{"name": "/dev/mapper/vg-data", "type": "lvm", "mountpoints": [null]}]}]},
      {"name": "/dev/sdc", "type": "disk", "size": 31914983424, "rm": true, "hotplug": true, "tran": "usb", "mountpoints": [null],
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
