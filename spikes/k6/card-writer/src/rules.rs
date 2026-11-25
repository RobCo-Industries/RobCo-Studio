//! The refusal rules: one pure function over a disk list.
//!
//! The only way to get an [`Approved`] target is [`decide`]. The write path takes an
//! [`Approved`] and nothing else, so it cannot open a disk the rules did not pass.

use crate::model::{Bus, Disk, DiskList};

/// What the person allowed.
#[derive(Debug, Clone)]
pub struct Policy {
    /// The largest disk the writer accepts, in bytes. The person raises it by hand.
    pub max_size: u64,
    /// Accept loop devices, disk images and VHDs. CI turns this on; a person never needs it.
    pub allow_virtual: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            max_size: 64 * 1024 * 1024 * 1024,
            allow_virtual: false,
        }
    }
}

/// Why a write was refused. Every one of them is final for this attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The list could not be read fully.
    ListIncomplete(Vec<String>),
    /// No disk was found to hold the running system, so the lister cannot be trusted.
    SystemNotLocated,
    /// The device path has characters a device path never has.
    BadPath,
    /// The device is not a whole disk in the list.
    NotInList,
    /// Two entries carry the same path.
    Ambiguous,
    HoldsRunningSystem,
    /// The lister could not tell whether this disk holds the running system.
    SystemUnknown,
    MountedSystemVolume(Vec<String>),
    /// An APFS container or another disk that is not a physical disk.
    Synthesized,
    /// SATA, NVMe, PCIe or another fixed bus.
    FixedInternal,
    BusUnknown(String),
    VirtualNotAllowed,
    NotRemovable,
    RemovableUnknown,
    SizeUnknown,
    TooLarge {
        size: u64,
        max: u64,
    },
    ImageLargerThanDisk {
        image: u64,
        disk: u64,
    },
    /// Volumes are mounted. The helper unmounts them, lists again and decides again.
    VolumesMounted(Vec<String>),
}

/// A disk the rules passed. It cannot be built outside this module.
#[derive(Debug)]
pub struct Approved {
    path: String,
    size: u64,
    block_size: Option<u32>,
}

impl Approved {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn block_size(&self) -> Option<u32> {
        self.block_size
    }
}

fn same_path(a: &str, b: &str) -> bool {
    // Windows paths compare without case and with either slash.
    let norm = |s: &str| s.replace('/', "\\").to_ascii_lowercase();
    if a.starts_with("\\\\") || a.starts_with("//") {
        norm(a) == norm(b)
    } else {
        a == b
    }
}

fn path_chars_ok(p: &str) -> bool {
    !p.is_empty()
        && p.len() < 64
        && p.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '\\' | '.' | '_' | '-'))
}

/// Decides whether `device` may be written with an image of `image_len` bytes.
/// "Could not tell" is always a refusal.
pub fn decide(
    list: &DiskList,
    device: &str,
    image_len: u64,
    policy: &Policy,
) -> Result<Approved, Vec<Refusal>> {
    let mut why = Vec::new();
    if !list.complete() {
        why.push(Refusal::ListIncomplete(list.problems.clone()));
    }
    if !list.system_located {
        why.push(Refusal::SystemNotLocated);
    }
    if !path_chars_ok(device) {
        why.push(Refusal::BadPath);
        return Err(why);
    }
    let found: Vec<&Disk> = list
        .disks
        .iter()
        .filter(|d| same_path(&d.path, device))
        .collect();
    let disk = match found.as_slice() {
        [] => {
            why.push(Refusal::NotInList);
            return Err(why);
        }
        [d] => *d,
        _ => {
            why.push(Refusal::Ambiguous);
            return Err(why);
        }
    };
    match disk.holds_system {
        Some(false) => {}
        Some(true) => why.push(Refusal::HoldsRunningSystem),
        None => why.push(Refusal::SystemUnknown),
    }
    if !disk.system_volumes.is_empty() {
        why.push(Refusal::MountedSystemVolume(disk.system_volumes.clone()));
    }
    if disk.synthesized {
        why.push(Refusal::Synthesized);
    }
    match disk.bus {
        Bus::Usb | Bus::Sd => match disk.removable {
            Some(true) => {}
            Some(false) => why.push(Refusal::NotRemovable),
            None => why.push(Refusal::RemovableUnknown),
        },
        Bus::Virtual => {
            if !policy.allow_virtual {
                why.push(Refusal::VirtualNotAllowed);
            }
        }
        Bus::Internal => why.push(Refusal::FixedInternal),
        Bus::Unknown => why.push(Refusal::BusUnknown(disk.bus_raw.clone())),
    }
    let size = match disk.size {
        Some(s) if s > 0 => {
            if s > policy.max_size {
                why.push(Refusal::TooLarge {
                    size: s,
                    max: policy.max_size,
                });
            }
            if image_len > s {
                why.push(Refusal::ImageLargerThanDisk {
                    image: image_len,
                    disk: s,
                });
            }
            s
        }
        _ => {
            why.push(Refusal::SizeUnknown);
            0
        }
    };
    if !disk.mounts.is_empty() {
        why.push(Refusal::VolumesMounted(disk.mounts.clone()));
    }
    if why.is_empty() {
        Ok(Approved {
            path: disk.path.clone(),
            size,
            block_size: disk.block_size,
        })
    } else {
        Err(why)
    }
}
