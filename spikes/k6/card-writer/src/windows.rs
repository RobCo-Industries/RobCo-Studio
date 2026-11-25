//! Windows: PowerShell `Get-Disk`, `Get-Partition` and `Win32_DiskDrive`, as one JSON object.

use crate::model::{Bus, Disk, DiskList, as_bool, as_list, as_str, as_u64};
use serde_json::Value;

/// The script the live lister runs. `@(..)` keeps arrays even with one item.
pub const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$d = @(Get-Disk | ForEach-Object { [pscustomobject]@{
  Number = $_.Number; FriendlyName = $_.FriendlyName; Size = $_.Size
  BusType = $_.CimInstanceProperties["BusType"].Value; BusTypeText = "$($_.BusType)"
  IsBoot = $_.IsBoot; IsSystem = $_.IsSystem; IsOffline = $_.IsOffline
  LogicalSectorSize = $_.LogicalSectorSize } })
$p = @(Get-Partition | ForEach-Object { [pscustomobject]@{
  DiskNumber = $_.DiskNumber; DriveLetter = "$($_.DriveLetter)"
  IsBoot = $_.IsBoot; IsSystem = $_.IsSystem; GptType = "$($_.GptType)"; MbrType = $_.MbrType
  AccessPaths = @($_.AccessPaths) } })
$w = @(Get-CimInstance Win32_DiskDrive | ForEach-Object { [pscustomobject]@{
  Index = $_.Index; MediaType = $_.MediaType } })
ConvertTo-Json -Depth 4 -Compress -InputObject ([pscustomobject]@{
  SystemDrive = $env:SystemDrive; Disks = $d; Partitions = $p; Drives = $w })
"#;

/// MSFT_Disk BusType values, from Microsoft's MSFT_Disk page: 0 Unknown, 1 SCSI, 2 ATAPI,
/// 3 ATA, 4 1394, 5 SSA, 6 Fibre Channel, 7 USB, 8 RAID, 9 iSCSI, 10 SAS, 11 SATA, 12 SD,
/// 13 MMC, 14 Virtual (reserved for system use), 15 File Backed Virtual, 16 Storage Spaces,
/// 17 NVMe. 18 and up are reserved. Every documented value has a class on purpose. Value 0,
/// a reserved value and any text not in this list land in `Unknown`, which is refused.
fn bus_of(n: Option<u64>, text: &str) -> (Bus, String) {
    let by_num = match n {
        Some(7) => Some(Bus::Usb),
        Some(12) | Some(13) => Some(Bus::Sd),
        // 14 is the Virtual bus some virtual disks report; 15 is a VHD.
        Some(14) | Some(15) => Some(Bus::Virtual),
        Some(1..=6) | Some(8..=11) | Some(16) | Some(17) => Some(Bus::Internal),
        _ => None,
    };
    let by_text = match text {
        "USB" => Some(Bus::Usb),
        "SD" | "MMC" => Some(Bus::Sd),
        "Virtual" | "File Backed Virtual" => Some(Bus::Virtual),
        "SCSI" | "ATAPI" | "ATA" | "1394" | "SSA" | "Fibre Channel" | "RAID" | "iSCSI" | "SAS"
        | "SATA" | "Storage Spaces" | "Spaces" | "NVMe" => Some(Bus::Internal),
        _ => None,
    };
    let raw = match n {
        Some(n) => format!("{text} ({n})"),
        None => text.to_string(),
    };
    // Both must agree when both are there.
    match (by_num, by_text) {
        (Some(a), Some(b)) if a == b => (a, raw),
        (Some(a), None) => (a, raw),
        (None, Some(b)) => (b, raw),
        _ => (Bus::Unknown, raw),
    }
}

/// The EFI system partition: GPT type c12a7328-f81f-11d2-ba4b-00a0c93ec93b or MBR type 0xEF.
/// Only the active one carries IsSystem, so the type is checked too.
fn is_esp(p: &Value) -> bool {
    let gpt = as_str(p.get("GptType")).unwrap_or("").to_ascii_lowercase();
    gpt.contains("c12a7328-f81f-11d2-ba4b-00a0c93ec93b") || as_u64(p.get("MbrType")) == Some(0xEF)
}

fn letter(v: Option<&Value>) -> Option<char> {
    let s = as_str(v)?;
    let c = s.chars().next()?;
    c.is_ascii_alphabetic().then(|| c.to_ascii_uppercase())
}

pub fn parse(text: &str) -> DiskList {
    let mut list = DiskList::default();
    let text = text.trim_start_matches('\u{feff}');
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            list.problems
                .push(format!("PowerShell output is not JSON: {e}"));
            return list;
        }
    };
    let sys_letter = letter(v.get("SystemDrive"));
    if sys_letter.is_none() {
        list.problems.push("SystemDrive is missing".into());
    }
    let (Some(disks), Some(parts)) = (as_list(v.get("Disks")), as_list(v.get("Partitions"))) else {
        list.problems.push("Disks or Partitions is missing".into());
        return list;
    };
    let drives = as_list(v.get("Drives")).unwrap_or_default();
    if drives.is_empty() {
        list.problems.push("Win32_DiskDrive gave nothing".into());
    }
    for d in disks {
        let Some(n) = as_u64(d.get("Number")) else {
            list.problems.push("a disk has no number".into());
            continue;
        };
        let offline = as_bool(d.get("IsOffline")).unwrap_or(false);
        let mut holds = match (as_bool(d.get("IsBoot")), as_bool(d.get("IsSystem"))) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        };
        let mut system_volumes = Vec::new();
        let mut mounts = Vec::new();
        for p in parts
            .iter()
            .filter(|p| as_u64(p.get("DiskNumber")) == Some(n))
        {
            let l = letter(p.get("DriveLetter"));
            // Every access path the system reports: a letter, a folder, or the volume GUID
            // path. A volume with only a GUID path is still a live volume, so it is kept.
            let paths: Vec<String> = as_list(p.get("AccessPaths"))
                .unwrap_or_default()
                .iter()
                .filter_map(|a| a.as_str().map(String::from))
                .collect();
            let is_sys = as_bool(p.get("IsBoot")) == Some(true)
                || as_bool(p.get("IsSystem")) == Some(true)
                || (l.is_some() && l == sys_letter)
                || is_esp(p);
            // The name shown for this volume: its letter, else a folder, else its GUID path.
            let name = match l {
                Some(c) => Some(format!("{c}:")),
                None => paths
                    .iter()
                    .find(|a| !a.starts_with("\\\\?\\"))
                    .or(paths.first())
                    .cloned(),
            };
            if is_sys {
                if as_bool(p.get("IsBoot")) == Some(true)
                    || as_bool(p.get("IsSystem")) == Some(true)
                    || (l.is_some() && l == sys_letter)
                {
                    holds = Some(true);
                }
                system_volumes.push(name.clone().unwrap_or_else(|| "system partition".into()));
            }
            if offline {
                continue;
            }
            if let Some(c) = l {
                mounts.push(format!("{c}:"));
            }
            let mut kept = false;
            for a in &paths {
                if !a.starts_with("\\\\?\\") && !mounts.iter().any(|m| a.starts_with(m.as_str())) {
                    mounts.push(a.clone());
                }
                kept |= !a.starts_with("\\\\?\\");
            }
            // No letter and no folder: the GUID path is the volume's only access path.
            if l.is_none()
                && !kept
                && let Some(g) = paths.first()
            {
                mounts.push(g.clone());
            }
        }
        let (bus, bus_raw) = bus_of(
            as_u64(d.get("BusType")),
            as_str(d.get("BusTypeText")).unwrap_or(""),
        );
        let media = drives
            .iter()
            .find(|w| as_u64(w.get("Index")) == Some(n))
            .and_then(|w| as_str(w.get("MediaType")));
        let removable = match media {
            Some(m) if m.contains("Removable") => Some(true),
            Some(m) if m.contains("Fixed") || m.contains("External") => Some(false),
            _ => None,
        };
        if holds == Some(true) {
            list.system_located = true;
        }
        list.disks.push(Disk {
            path: format!("\\\\.\\PhysicalDrive{n}"),
            size: as_u64(d.get("Size")),
            removable,
            bus,
            bus_raw,
            holds_system: holds,
            system_volumes,
            mounts,
            synthesized: false,
            label: as_str(d.get("FriendlyName"))
                .unwrap_or("")
                .trim()
                .to_string(),
            block_size: as_u64(d.get("LogicalSectorSize")).and_then(|b| u32::try_from(b).ok()),
        });
    }
    list
}

#[cfg(windows)]
pub fn live() -> DiskList {
    let args = ["-NoProfile", "-NonInteractive", "-Command", SCRIPT];
    match crate::run("powershell", &args, None) {
        Ok(out) => parse(&out),
        Err(e) => DiskList {
            problems: vec![e],
            ..DiskList::default()
        },
    }
}
