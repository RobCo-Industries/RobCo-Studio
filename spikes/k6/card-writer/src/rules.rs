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
    /// The logical sector size is unknown, or is not 512 or 4096. The writer pads the last
    /// chunk to the sector size and only knows these two.
    SectorSizeUnsupported(Option<u32>),
    /// Volumes are mounted. The helper unmounts them, lists again and decides again.
    VolumesMounted(Vec<String>),
}

/// A disk the rules passed. It cannot be built outside this module.
#[derive(Debug)]
pub struct Approved {
    path: String,
    size: u64,
    block_size: u32,
    /// The image length the rules checked. The write writes exactly this many bytes.
    image_len: u64,
}

impl Approved {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn block_size(&self) -> u32 {
        self.block_size
    }
    pub fn image_len(&self) -> u64 {
        self.image_len
    }
    /// True when a second decision approves the same thing as this one. The writer decides
    /// again after it opens the disk and needs this to hold.
    ///
    /// Every field is compared, each for a reason. The destructuring has no `..`, so a new
    /// field does not compile until it is added here with its reason.
    /// Fields the rules read but `Approved` does not carry, such as the bus, removable,
    /// mounts and the system flags, are checked by the second `decide` itself: it must
    /// pass again before `same_as` is asked.
    pub fn same_as(&self, other: &Approved) -> bool {
        let Approved {
            // The disk the writer opened: another path is another disk.
            path,
            // The writer checks the image fits, and on Linux compares it with the open
            // device. A card of another size under the same path is another card.
            size,
            // The writer pads every write and the read-back to this sector size. A disk
            // with another sector size takes partial sectors or writes past the image.
            block_size,
            // The writer writes and compares exactly this many bytes.
            image_len,
        } = self;
        same_path(path, &other.path)
            && *size == other.size
            && *block_size == other.block_size
            && *image_len == other.image_len
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

/// The image length padded to whole sectors: what the writer writes.
pub fn padded_len(len: u64, sector: u32) -> u64 {
    len.div_ceil(sector as u64) * sector as u64
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
    let sector = match disk.block_size {
        Some(b @ (512 | 4096)) => b,
        other => {
            why.push(Refusal::SectorSizeUnsupported(other));
            4096
        }
    };
    let size = match disk.size {
        Some(s) if s > 0 => {
            if s > policy.max_size {
                why.push(Refusal::TooLarge {
                    size: s,
                    max: policy.max_size,
                });
            }
            // The writer pads the last chunk to the sector size, so the padded length must fit.
            if padded_len(image_len, sector) > s {
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
            block_size: sector,
            image_len,
        })
    } else {
        Err(why)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Approved {
        Approved {
            path: r"\\.\PhysicalDrive4".into(),
            size: 31_914_983_424,
            block_size: 512,
            image_len: 64 << 20,
        }
    }

    /// One approval per field, the same as `base` except for that field. The match has no
    /// `..`, so a new field breaks this test until it gets its own changed copy.
    #[test]
    fn same_as_compares_every_field() {
        let a = base();
        assert!(a.same_as(&base()));
        let Approved {
            path: _,
            size: _,
            block_size: _,
            image_len: _,
        } = &a;
        let changed = [
            Approved {
                path: r"\\.\PhysicalDrive5".into(),
                ..base()
            },
            Approved {
                size: a.size + 512,
                ..base()
            },
            Approved {
                block_size: 4096,
                ..base()
            },
            Approved {
                image_len: a.image_len + 512,
                ..base()
            },
        ];
        for c in &changed {
            assert!(!a.same_as(c), "{c:?} passed as the same disk");
        }
    }

    #[test]
    fn same_as_ignores_path_case_on_windows() {
        let b = Approved {
            path: r"\\.\physicaldrive4".into(),
            ..base()
        };
        assert!(base().same_as(&b));
    }
}
