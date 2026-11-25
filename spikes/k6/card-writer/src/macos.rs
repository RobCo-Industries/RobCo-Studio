//! macOS: `diskutil list -plist`, `diskutil info -plist` for each whole disk and for `/`.
//!
//! The live lister turns each plist into JSON with `plutil -convert json`. A plist
//! that plutil cannot convert makes the list incomplete, which refuses every write.

use crate::model::{Bus, Disk, DiskList, as_bool, as_str, as_u64};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The recorded or live input: `{"list": .., "root": .., "info": {"diskN": .. or null}}`.
pub fn parse(text: &str) -> DiskList {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => parse_value(&v),
        Err(e) => DiskList {
            problems: vec![format!("input is not JSON: {e}")],
            ..DiskList::default()
        },
    }
}

/// "disk10s2" -> "disk10". "disk3s1s1" -> "disk3".
fn whole_of(id: &str) -> Option<&str> {
    let rest = id.strip_prefix("disk")?;
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    Some(&id[..4 + digits])
}

fn is_system_mount(m: &str) -> bool {
    m == "/" || m.starts_with("/System/Volumes/") || m == "/private/var/vm"
}

pub fn parse_value(v: &Value) -> DiskList {
    let mut list = DiskList::default();
    let empty = Map::new();
    let info = v.get("info").and_then(|i| i.as_object()).unwrap_or(&empty);
    let Some(whole) = v
        .get("list")
        .and_then(|l| l.get("WholeDisks"))
        .and_then(|w| w.as_array())
    else {
        list.problems.push("diskutil list has no WholeDisks".into());
        return list;
    };
    let entries = v
        .get("list")
        .and_then(|l| l.get("AllDisksAndPartitions"))
        .and_then(|a| a.as_array());
    let Some(entries) = entries else {
        list.problems
            .push("diskutil list has no AllDisksAndPartitions".into());
        return list;
    };

    // Mounts of each whole disk, and the physical stores of each APFS container.
    let mut mounts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stores: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for e in entries {
        let Some(id) = as_str(e.get("DeviceIdentifier")) else {
            list.problems.push("a disk entry has no identifier".into());
            continue;
        };
        let m = mounts.entry(id.to_string()).or_default();
        if let Some(p) = as_str(e.get("MountPoint")) {
            m.push(p.to_string());
        }
        for key in ["Partitions", "APFSVolumes"] {
            if let Some(parts) = e.get(key).and_then(|p| p.as_array()) {
                for p in parts {
                    if let Some(mp) = as_str(p.get("MountPoint")) {
                        m.push(mp.to_string());
                    }
                }
            }
        }
        if let Some(st) = e.get("APFSPhysicalStores").and_then(|s| s.as_array()) {
            let parents = st
                .iter()
                .filter_map(|s| as_str(s.get("DeviceIdentifier")))
                .filter_map(whole_of)
                .map(String::from)
                .collect();
            stores.insert(id.to_string(), parents);
        }
    }
    // A container's mounts also belong to the disks that carry it, through chains.
    for _ in 0..8 {
        let mut changed = false;
        for (container, parents) in &stores {
            let carried = mounts.get(container).cloned().unwrap_or_default();
            for p in parents {
                let pm = mounts.entry(p.clone()).or_default();
                for m in &carried {
                    if !pm.contains(m) {
                        pm.push(m.clone());
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }

    // Where `/` lives, from `diskutil info /`.
    let mut root_disks: Vec<String> = Vec::new();
    match v.get("root") {
        Some(r) if r.is_object() => {
            if let Some(p) = as_str(r.get("ParentWholeDisk")) {
                root_disks.push(p.to_string());
            }
            if let Some(st) = r.get("APFSPhysicalStores").and_then(|s| s.as_array()) {
                for s in st {
                    if let Some(w) = as_str(s.get("APFSPhysicalStore")).and_then(whole_of) {
                        root_disks.push(w.to_string());
                    }
                }
            }
            if root_disks.is_empty() {
                list.problems.push("diskutil info / names no disk".into());
            }
        }
        _ => list.problems.push("diskutil info / is missing".into()),
    }

    for w in whole {
        let Some(id) = w.as_str() else {
            list.problems.push("a whole disk id is not a string".into());
            continue;
        };
        let Some(i) = info.get(id).filter(|i| i.is_object()) else {
            list.problems.push(format!("diskutil info failed for {id}"));
            continue;
        };
        let path = format!("/dev/{id}");
        if as_str(i.get("DeviceNode")) != Some(path.as_str()) {
            list.problems
                .push(format!("{id}: DeviceNode does not match its identifier"));
            continue;
        }
        let mut m = mounts.get(id).cloned().unwrap_or_default();
        m.sort();
        m.dedup();
        let holds = m.iter().any(|x| x == "/") || root_disks.iter().any(|r| r == id);
        let bus_raw = as_str(i.get("BusProtocol")).unwrap_or("").to_string();
        let bus = match bus_raw.as_str() {
            "USB" => Bus::Usb,
            "Secure Digital" => Bus::Sd,
            "Disk Image" => Bus::Virtual,
            "Apple Fabric" | "PCI-Express" | "PCI" | "SATA" | "NVMe" | "SAS" => Bus::Internal,
            _ => Bus::Unknown,
        };
        let removable = match (
            as_bool(i.get("RemovableMedia")),
            as_bool(i.get("Removable")),
        ) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) | (Some(false), None) | (None, Some(false)) => Some(false),
            (None, None) => None,
        };
        let synthesized = stores.contains_key(id)
            || as_str(i.get("IORegistryEntryName")) == Some("AppleAPFSMedia");
        list.system_located |= holds;
        list.disks.push(Disk {
            path,
            size: as_u64(i.get("TotalSize")).or(as_u64(i.get("Size"))),
            removable,
            bus,
            bus_raw,
            holds_system: Some(holds),
            system_volumes: m.iter().filter(|x| is_system_mount(x)).cloned().collect(),
            mounts: m,
            synthesized,
            label: as_str(i.get("MediaName")).unwrap_or("").trim().to_string(),
            block_size: as_u64(i.get("DeviceBlockSize")).map(|b| b as u32),
        });
    }
    if !root_disks.is_empty()
        && !root_disks
            .iter()
            .all(|r| list.disks.iter().any(|d| d.path == format!("/dev/{r}")))
    {
        list.problems
            .push("the disk that holds / is not in the list".into());
    }
    list
}

#[cfg(target_os = "macos")]
fn plist_json(args: &[&str]) -> Result<Value, String> {
    let plist = crate::run("diskutil", args, None)?;
    let json = crate::run(
        "plutil",
        &["-convert", "json", "-o", "-", "-"],
        Some(&plist),
    )?;
    serde_json::from_str(&json).map_err(|e| format!("plutil gave bad JSON: {e}"))
}

/// Read-only: runs `diskutil list -plist` and `diskutil info -plist` only.
#[cfg(target_os = "macos")]
pub fn live() -> DiskList {
    let mut problems = Vec::new();
    let list = plist_json(&["list", "-plist"]).unwrap_or_else(|e| {
        problems.push(e);
        Value::Null
    });
    let root = plist_json(&["info", "-plist", "/"]).unwrap_or_else(|e| {
        problems.push(e);
        Value::Null
    });
    let mut info = Map::new();
    if let Some(w) = list.get("WholeDisks").and_then(|w| w.as_array()) {
        for id in w.iter().filter_map(|x| x.as_str()) {
            // Only plain identifiers reach the command line.
            if whole_of(id) != Some(id) {
                problems.push(format!("odd disk id {id:?}"));
                continue;
            }
            let v = plist_json(&["info", "-plist", id]).unwrap_or(Value::Null);
            info.insert(id.to_string(), v);
        }
    }
    let input = serde_json::json!({"list": list, "root": root, "info": info});
    let mut out = parse_value(&input);
    out.problems.extend(problems);
    out
}
