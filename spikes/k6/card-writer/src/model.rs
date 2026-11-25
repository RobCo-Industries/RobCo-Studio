//! The disk list, the same shape on every system.

use serde_json::{Value, json};

/// The bus a disk sits on, as far as the system tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// USB, which includes most card readers.
    Usb,
    /// A built-in SD or MMC reader.
    Sd,
    /// A loop device, a disk image or a VHD.
    Virtual,
    /// SATA, NVMe, PCIe, Apple Fabric and the like.
    Internal,
    /// Anything else, or nothing reported.
    Unknown,
}

/// One whole disk.
#[derive(Debug, Clone)]
pub struct Disk {
    /// The whole-disk device path as the system names it.
    pub path: String,
    pub size: Option<u64>,
    /// Removable media, as reported. `None` means the system did not say.
    pub removable: Option<bool>,
    pub bus: Bus,
    /// The bus as the system wrote it, for the person and for the log.
    pub bus_raw: String,
    /// Holds the running system. `None` means the lister could not tell.
    pub holds_system: Option<bool>,
    /// Mounted volumes that belong to the system: root, boot, swap, macOS system volumes.
    pub system_volumes: Vec<String>,
    /// Every mounted volume on the disk, system or not.
    pub mounts: Vec<String>,
    /// A synthesized disk such as an APFS container: not a physical disk.
    pub synthesized: bool,
    pub label: String,
    pub block_size: Option<u32>,
}

/// The whole list. A list with problems is incomplete and every write is refused.
#[derive(Debug, Clone, Default)]
pub struct DiskList {
    pub disks: Vec<Disk>,
    pub problems: Vec<String>,
    /// True when one disk was found to hold the running system.
    pub system_located: bool,
}

impl DiskList {
    pub fn complete(&self) -> bool {
        self.problems.is_empty()
    }

    pub fn to_json(&self) -> Value {
        let disks: Vec<Value> = self
            .disks
            .iter()
            .map(|d| {
                json!({
                    "path": d.path,
                    "size": d.size,
                    "removable": d.removable,
                    "bus": format!("{:?}", d.bus),
                    "bus_raw": d.bus_raw,
                    "holds_system": d.holds_system,
                    "system_volumes": d.system_volumes,
                    "mounts": d.mounts,
                    "synthesized": d.synthesized,
                    "label": d.label,
                    "block_size": d.block_size,
                })
            })
            .collect();
        json!({
            "complete": self.complete(),
            "system_located": self.system_located,
            "problems": self.problems,
            "disks": disks,
        })
    }
}

/// Reads a JSON value as a u64, whether it came as a number or a string.
pub fn as_u64(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Reads a JSON value as a bool: true, false, 1, 0, "1", "0", "true", "false".
pub fn as_bool(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_u64()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        },
        Value::String(s) => match s.trim() {
            "1" | "true" | "True" => Some(true),
            "0" | "false" | "False" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

pub fn as_str(v: Option<&Value>) -> Option<&str> {
    v?.as_str()
}

/// Accepts an array, or a single object that PowerShell wrote without the array.
pub fn as_list(v: Option<&Value>) -> Option<Vec<&Value>> {
    match v? {
        Value::Array(a) => Some(a.iter().collect()),
        Value::Object(_) => Some(vec![v?]),
        Value::Null => Some(Vec::new()),
        _ => None,
    }
}
