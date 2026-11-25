//! Linux: `lsblk --json`.

use crate::model::{Bus, Disk, DiskList, as_bool, as_str, as_u64};
use serde_json::Value;

/// The command the live lister runs. `--paths` makes NAME the full device path.
pub const LSBLK_ARGS: &[&str] = &[
    "--json",
    "--bytes",
    "--paths",
    "--output",
    "NAME,TYPE,SIZE,RM,HOTPLUG,TRAN,RO,MOUNTPOINTS,VENDOR,MODEL,LOG-SEC",
];

const SYSTEM_MOUNTS: &[&str] = &[
    "/",
    "/boot",
    "/boot/efi",
    "/efi",
    "/usr",
    "/var",
    "/home",
    "/opt",
    "/srv",
    "[SWAP]",
];

fn collect_mounts(dev: &Value, out: &mut Vec<String>) {
    // Newer lsblk writes "mountpoints": [..], older writes "mountpoint": "..".
    if let Some(Value::Array(a)) = dev.get("mountpoints") {
        out.extend(a.iter().filter_map(|m| m.as_str()).map(String::from));
    }
    if let Some(m) = as_str(dev.get("mountpoint")) {
        out.push(m.to_string());
    }
    if let Some(Value::Array(kids)) = dev.get("children") {
        for k in kids {
            collect_mounts(k, out);
        }
    }
}

/// Device-mapper and RAID children: LUKS, LVM, md. They hold the disk open with no
/// mount point of their own, so each one counts as a volume in use.
fn collect_holders(dev: &Value, out: &mut Vec<String>) {
    if let Some(Value::Array(kids)) = dev.get("children") {
        for k in kids {
            let kind = as_str(k.get("type")).unwrap_or("");
            if kind != "part"
                && let Some(n) = as_str(k.get("name"))
            {
                out.push(format!("in use by {kind} {n}"));
            }
            collect_holders(k, out);
        }
    }
}

/// Parses the JSON `lsblk` printed with [`LSBLK_ARGS`].
pub fn parse(text: &str) -> DiskList {
    let mut list = DiskList::default();
    let root: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            list.problems.push(format!("lsblk output is not JSON: {e}"));
            return list;
        }
    };
    let Some(Value::Array(devs)) = root.get("blockdevices") else {
        list.problems
            .push("lsblk output has no blockdevices".into());
        return list;
    };
    for dev in devs {
        let Some(path) = as_str(dev.get("name")) else {
            list.problems.push("a block device has no name".into());
            continue;
        };
        let kind = as_str(dev.get("type")).unwrap_or("");
        if kind != "disk" && kind != "loop" {
            continue; // rom, and the like
        }
        let mut mounts = Vec::new();
        collect_mounts(dev, &mut mounts);
        collect_holders(dev, &mut mounts);
        mounts.sort();
        mounts.dedup();
        let holds = mounts.iter().any(|m| m == "/");
        let system_volumes: Vec<String> = mounts
            .iter()
            .filter(|m| SYSTEM_MOUNTS.contains(&m.as_str()) || m.starts_with("/boot/"))
            .cloned()
            .collect();
        let tran = as_str(dev.get("tran")).unwrap_or("").to_string();
        let bus = if kind == "loop" {
            Bus::Virtual
        } else {
            match tran.as_str() {
                "usb" => Bus::Usb,
                "mmc" => Bus::Sd,
                "sata" | "ata" | "nvme" | "sas" | "scsi" | "spi" | "virtio" | "fc" | "iscsi" => {
                    Bus::Internal
                }
                _ => Bus::Unknown,
            }
        };
        let rm = as_bool(dev.get("rm"));
        let hotplug = as_bool(dev.get("hotplug"));
        let removable = match (rm, hotplug) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        };
        let vendor = as_str(dev.get("vendor")).unwrap_or("").trim();
        let model = as_str(dev.get("model")).unwrap_or("").trim();
        let size = as_u64(dev.get("size"));
        list.system_located |= holds;
        list.disks.push(Disk {
            path: path.to_string(),
            size,
            removable,
            bus,
            bus_raw: if kind == "loop" { "loop".into() } else { tran },
            holds_system: Some(holds),
            system_volumes,
            mounts,
            synthesized: false,
            label: format!("{vendor} {model}").trim().to_string(),
            block_size: as_u64(dev.get("log-sec")).and_then(|b| u32::try_from(b).ok()),
        });
    }
    list
}

#[cfg(target_os = "linux")]
pub fn live() -> DiskList {
    match crate::run("lsblk", LSBLK_ARGS, None) {
        Ok(out) => parse(&out),
        Err(e) => DiskList {
            problems: vec![e],
            ..DiskList::default()
        },
    }
}
