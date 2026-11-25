//! Windows: PowerShell `Get-Disk`, `Get-Partition` and `Win32_DiskDrive`, as one JSON object.

use crate::model::{Bus, Disk, DiskList, as_bool, as_list, as_str, as_u64};
use serde_json::Value;

/// The script the live lister runs. `@(..)` keeps arrays even with one item.
pub const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$d = @(Get-Disk | ForEach-Object { [pscustomobject]@{
  Number = $_.Number; FriendlyName = $_.FriendlyName; Size = $_.Size
  BusType = [int]$_.BusType; BusTypeText = "$($_.BusType)"
  IsBoot = $_.IsBoot; IsSystem = $_.IsSystem; IsOffline = $_.IsOffline
  LogicalSectorSize = $_.LogicalSectorSize } })
$p = @(Get-Partition | ForEach-Object { [pscustomobject]@{
  DiskNumber = $_.DiskNumber; DriveLetter = "$($_.DriveLetter)"
  IsBoot = $_.IsBoot; IsSystem = $_.IsSystem; AccessPaths = @($_.AccessPaths) } })
$w = @(Get-CimInstance Win32_DiskDrive | ForEach-Object { [pscustomobject]@{
  Index = $_.Index; MediaType = $_.MediaType } })
ConvertTo-Json -Depth 4 -Compress -InputObject ([pscustomobject]@{
  SystemDrive = $env:SystemDrive; Disks = $d; Partitions = $p; Drives = $w })
"#;

/// MSFT_Disk BusType values.
fn bus_of(n: Option<u64>, text: &str) -> (Bus, String) {
    let by_num = match n {
        Some(7) => Some(Bus::Usb),
        Some(12) | Some(13) => Some(Bus::Sd),
        Some(15) => Some(Bus::Virtual),
        Some(1..=6) | Some(8..=11) | Some(14) | Some(16..=18) => Some(Bus::Internal),
        _ => None,
    };
    let by_text = match text {
        "USB" => Some(Bus::Usb),
        "SD" | "MMC" => Some(Bus::Sd),
        "File Backed Virtual" => Some(Bus::Virtual),
        "" | "Unknown" => None,
        _ => Some(Bus::Internal),
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
            let is_sys = as_bool(p.get("IsBoot")) == Some(true)
                || as_bool(p.get("IsSystem")) == Some(true)
                || (l.is_some() && l == sys_letter);
            if is_sys {
                holds = Some(true);
                system_volumes.push(match l {
                    Some(c) => format!("{c}:"),
                    None => "system partition".into(),
                });
            }
            if offline {
                continue;
            }
            if let Some(c) = l {
                mounts.push(format!("{c}:"));
            }
            for a in as_list(p.get("AccessPaths")).unwrap_or_default() {
                if let Some(a) = a.as_str()
                    && !a.starts_with("\\\\?\\")
                    && !mounts.iter().any(|m| a.starts_with(m.as_str()))
                {
                    mounts.push(a.to_string());
                }
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
            block_size: as_u64(d.get("LogicalSectorSize")).map(|b| b as u32),
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
