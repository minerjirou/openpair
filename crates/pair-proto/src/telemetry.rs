//! Node hardware inventory + telemetry.
//!
//! The `/v1/node-info` wire shape is **confirmed against the running reference**
//! (dynamic capture of `nvpair-node-info`):
//! ```json
//! {"GPUs":[{"name":"...","vram_bytes":N,"vram_used_bytes":N,"utilization_percent":P}],
//!  "cpu":{"name":"...","cores":N},
//!  "memory":{"total_bytes":N},
//!  "telemetryValid":bool,"msSince":N,"hostUuid":"..."}
//! ```
//! `vram_used_bytes`/`utilization_percent` appear only once live telemetry has
//! been sampled (`telemetryValid` true). This crate serializes exactly those
//! keys; `pair-nodeinfo` fills them from NVIDIA (`nvidia-smi`) or AMD
//! (`amd-smi`/`rocm-smi`) uniformly.

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

/// A single GPU on the `/v1/node-info` wire. Only `name` + `vram_bytes` are
/// always present; used/utilization appear when telemetry is valid.
///
/// `uuid`/`vendor`/`vendor_id` are kept for internal routing but are NOT part of
/// this endpoint's wire shape (the reference omits them here), so they are
/// `#[serde(skip)]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gpu {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vram_used_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization_percent: Option<u32>,

    // Internal-only (not on the /v1/node-info wire).
    #[serde(skip)]
    pub uuid: String,
    #[serde(skip)]
    pub vendor: GpuVendor,
    #[serde(skip)]
    pub vendor_id: Option<u16>,
}

impl Default for GpuVendor {
    fn default() -> Self {
        GpuVendor::Other
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cpu {
    /// Confirmed wire key: `name`.
    pub name: String,
    /// Confirmed wire key: `cores`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cores: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_threads: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utilization_percent: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_bytes: Option<u64>,
}

/// The `/v1/node-info` response. Key names + shape confirmed against the
/// reference. `hostUuid` carries the node id the server was told to report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeInfo {
    #[serde(rename = "GPUs", default)]
    pub gpus: Vec<Gpu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<Cpu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryInfo>,
    #[serde(rename = "telemetryValid", default)]
    pub telemetry_valid: bool,
    #[serde(rename = "msSince", default)]
    pub ms_since: u64,
    #[serde(rename = "hostUuid", default, skip_serializing_if = "Option::is_none")]
    pub host_uuid: Option<String>,
    #[serde(rename = "nodeUuid", default, skip_serializing_if = "Option::is_none")]
    pub node_uuid: Option<String>,
    #[serde(rename = "clusterUuid", default, skip_serializing_if = "Option::is_none")]
    pub cluster_uuid: Option<String>,
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
    fn gpu_wire_matches_reference() {
        let g = Gpu {
            name: "AMD Radeon RX 7900 XTX".into(),
            vram_bytes: Some(24 << 30),
            vram_used_bytes: Some(2 << 30),
            utilization_percent: Some(37),
            uuid: "GPU-abc".into(),
            vendor: GpuVendor::Amd,
            vendor_id: Some(0x1002),
        };
        let s = serde_json::to_string(&g).unwrap();
        assert!(s.contains("\"name\""));
        assert!(s.contains("\"vram_bytes\""));
        assert!(s.contains("\"vram_used_bytes\""));
        assert!(s.contains("\"utilization_percent\""));
        // Internal fields must NOT be on the wire (reference omits them here).
        assert!(!s.contains("uuid"));
        assert!(!s.contains("vendor"));
    }

    #[test]
    fn parses_reference_capture() {
        // Exact bytes captured from the running reference node-info.
        let raw = r#"{"GPUs":[{"name":"NVIDIA GeForce RTX 5070","vram_bytes":12523143168},
            {"name":"AMD Radeon(TM) Graphics","vram_bytes":509353984}],
            "cpu":{"name":"AMD Ryzen 7 9700X 8-Core Processor","cores":8},
            "memory":{"total_bytes":34359738368},
            "telemetryValid":false,"msSince":0,"hostUuid":"demo-1234"}"#;
        let ni: NodeInfo = serde_json::from_str(raw).unwrap();
        assert_eq!(ni.gpus.len(), 2);
        assert_eq!(ni.gpus[0].name, "NVIDIA GeForce RTX 5070");
        assert_eq!(ni.gpus[0].vram_bytes, Some(12523143168));
        assert_eq!(ni.cpu.as_ref().unwrap().name, "AMD Ryzen 7 9700X 8-Core Processor");
        assert_eq!(ni.cpu.as_ref().unwrap().cores, Some(8));
        assert_eq!(ni.memory.unwrap().total_bytes, 34359738368);
        assert!(!ni.telemetry_valid);
        assert_eq!(ni.host_uuid.as_deref(), Some("demo-1234"));
    }

    #[test]
    fn vendor_from_pci() {
        assert_eq!(GpuVendor::from_pci_id(0x1002), GpuVendor::Amd);
        assert_eq!(GpuVendor::from_pci_id(0x10de), GpuVendor::Nvidia);
    }
}
