//! Node hardware inventory + telemetry.
//!
//! GPU field names are confirmed from embedded JSON struct tags:
//! `vram_bytes`, `vram_used_bytes`, `utilization_percent`, plus the hardware
//! inventory keys (`vendor`, `vendor_id`, `product`, `name`, `index`). The
//! reference implementation fills VRAM/utilization from a vendor CLI
//! (`nvidia-smi` on NVIDIA) and static inventory from sysfs/PCI, degrading
//! gracefully (leaving used/utilization absent) when the vendor CLI is missing.
//!
//! This crate defines the vendor-neutral shape; `pair-nodeinfo` fills it from
//! either NVIDIA (`nvidia-smi`) or AMD (`rocm-smi` / `amd-smi`) backends.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl GpuVendor {
    /// PCI vendor id -> vendor. 0x10de = NVIDIA, 0x1002 = AMD/ATI, 0x8086 = Intel.
    pub fn from_pci_id(id: u16) -> Self {
        match id {
            0x10de => GpuVendor::Nvidia,
            0x1002 => GpuVendor::Amd,
            0x8086 => GpuVendor::Intel,
            _ => GpuVendor::Other,
        }
    }
}

/// A single GPU, static inventory + (optional) live telemetry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    /// Stable per-GPU identifier (NVIDIA GPU UUID, or AMD unique id / PCI BDF).
    pub uuid: String,
    /// Product name, e.g. "NVIDIA GeForce RTX 4090" / "AMD Radeon RX 7900 XTX".
    pub name: String,
    pub vendor: GpuVendor,
    /// PCI vendor id (0x10de / 0x1002 / ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor_id: Option<u16>,
    /// Total VRAM in bytes (static).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_bytes: Option<u64>,
    /// Currently used VRAM in bytes (live; absent if vendor CLI unavailable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_used_bytes: Option<u64>,
    /// GPU utilization 0-100 (live; absent if vendor CLI unavailable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization_percent: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cpu {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_cores: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_threads: Option<u32>,
    /// Live utilization 0-100 if sampled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization_percent: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_bytes: Option<u64>,
}

/// Aggregate node info/telemetry payload.
///
/// TODO(interop): the exact outer envelope key casing (`clusterUuid` vs
/// `cluster_uuid`) is being reconciled with the RPC-contract analysis pass; the
/// GPU sub-schema above is confirmed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeInfo {
    #[serde(rename = "hostUuid", default, skip_serializing_if = "Option::is_none")]
    pub host_uuid: Option<String>,
    #[serde(rename = "nodeUuid", default, skip_serializing_if = "Option::is_none")]
    pub node_uuid: Option<String>,
    #[serde(rename = "clusterUuid", default, skip_serializing_if = "Option::is_none")]
    pub cluster_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Cpu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryInfo>,
    #[serde(rename = "GPUs", default)]
    pub gpus: Vec<Gpu>,
    /// True once at least one telemetry sample has succeeded.
    #[serde(rename = "telemetryValid", default)]
    pub telemetry_valid: bool,
}

impl NodeInfo {
    /// Bytes of VRAM currently free across all GPUs (best-effort).
    pub fn free_vram_bytes(&self) -> u64 {
        self.gpus
            .iter()
            .filter_map(|g| match (g.vram_bytes, g.vram_used_bytes) {
                (Some(total), Some(used)) => Some(total.saturating_sub(used)),
                _ => None,
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_serializes_confirmed_tags() {
        let g = Gpu {
            uuid: "GPU-abc".into(),
            name: "AMD Radeon RX 7900 XTX".into(),
            vendor: GpuVendor::Amd,
            vendor_id: Some(0x1002),
            vram_bytes: Some(24 << 30),
            vram_used_bytes: Some(2 << 30),
            utilization_percent: Some(37),
        };
        let s = serde_json::to_string(&g).unwrap();
        for tag in ["vram_bytes", "vram_used_bytes", "utilization_percent", "vendor"] {
            assert!(s.contains(tag), "missing tag {tag} in {s}");
        }
    }

    #[test]
    fn vendor_from_pci() {
        assert_eq!(GpuVendor::from_pci_id(0x1002), GpuVendor::Amd);
        assert_eq!(GpuVendor::from_pci_id(0x10de), GpuVendor::Nvidia);
    }

    #[test]
    fn free_vram() {
        let ni = NodeInfo {
            host_uuid: None, node_uuid: None, cluster_uuid: None, cpu: None, memory: None,
            telemetry_valid: true,
            gpus: vec![Gpu {
                uuid: "g".into(), name: "n".into(), vendor: GpuVendor::Amd, vendor_id: None,
                vram_bytes: Some(100), vram_used_bytes: Some(40), utilization_percent: None,
            }],
        };
        assert_eq!(ni.free_vram_bytes(), 60);
    }
}
