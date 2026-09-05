//! Node hardware inventory + GPU telemetry.
//!
//! Vendor-neutral collector that fills the cluster's telemetry schema from
//! whichever GPU vendor is present. Unlike the reference implementation (which
//! only understands `nvidia-smi`), this collector also supports AMD/ROCm, so an
//! AMD box participates in the cluster on equal footing.

pub mod amd;
pub mod gpu_os;
pub mod nvidia;

use pair_proto::{Cpu, Gpu, MemoryInfo, NodeInfo};

/// Detect all GPUs across supported vendors (NVIDIA + AMD/ROCm).
pub fn detect_gpus() -> Vec<Gpu> {
    let mut gpus = nvidia::detect();
    gpus.extend(amd::detect());
    // OS-level inventory (Windows WMI / macOS system_profiler) fills GPUs that no
    // vendor tool reported -- e.g. an AMD iGPU with no ROCm tools installed. Skip
    // vendors a tool already covered (their VRAM/util are more accurate).
    let covered: std::collections::HashSet<_> = gpus.iter().map(|g| g.vendor).collect();
    for g in gpu_os::os_gpus() {
        if !covered.contains(&g.vendor) {
            gpus.push(g);
        }
    }
    gpus
}

/// Collect a full node telemetry snapshot.
pub fn collect(node_uuid: Option<String>, cluster_uuid: Option<String>) -> NodeInfo {
    let gpus = detect_gpus();
    let telemetry_valid = gpus.iter().any(|g| g.vram_used_bytes.is_some() || g.utilization_percent.is_some());
    NodeInfo {
        gpus,
        cpu: host::cpu(),
        memory: host::memory(),
        telemetry_valid,
        ms_since: 0,
        host_uuid: host::host_uuid(),
        node_uuid,
        cluster_uuid,
    }
}

/// Host CPU/memory/uuid collection. Linux uses /proc; other OSes return best
/// effort (to be extended per platform).
mod host {
    use super::*;

    pub fn host_uuid() -> Option<String> {
        // Linux: /etc/machine-id or /var/lib/dbus/machine-id.
        for p in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
            if let Ok(s) = std::fs::read_to_string(p) {
                let s = s.trim();
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
        None
    }

    /// Portable CPU detection (Windows / macOS / Linux) via `sysinfo`.
    pub fn cpu() -> Option<Cpu> {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_all();
        let cpus = sys.cpus();
        if cpus.is_empty() {
            return None;
        }
        let name = cpus[0].brand().trim().to_string();
        let name = if name.is_empty() {
            cpus[0].vendor_id().trim().to_string()
        } else {
            name
        };
        let cores = sys.physical_core_count().map(|c| c as u32);
        Some(Cpu {
            name: if name.is_empty() { "unknown".into() } else { name },
            cores,
            total_threads: Some(cpus.len() as u32),
            utilization_percent: None,
        })
    }

    /// Portable memory detection via `sysinfo` (values are bytes in 0.30+).
    pub fn memory() -> Option<MemoryInfo> {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        let total = sys.total_memory();
        if total == 0 {
            return None;
        }
        Some(MemoryInfo { total_bytes: total, used_bytes: Some(sys.used_memory()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_gpus_never_panics() {
        // On a machine with no GPU tools this must simply return empty.
        let _ = detect_gpus();
    }

    #[test]
    fn collect_builds_nodeinfo() {
        let ni = collect(Some("node-1".into()), None);
        assert_eq!(ni.node_uuid.as_deref(), Some("node-1"));
    }
}
