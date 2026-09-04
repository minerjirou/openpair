//! Node hardware inventory + GPU telemetry.
//!
//! Vendor-neutral collector that fills the cluster's telemetry schema from
//! whichever GPU vendor is present. Unlike the reference implementation (which
//! only understands `nvidia-smi`), this collector also supports AMD/ROCm, so an
//! AMD box participates in the cluster on equal footing.

pub mod amd;
pub mod nvidia;

use pair_proto::{Cpu, Gpu, MemoryInfo, NodeInfo};

/// Detect all GPUs across supported vendors (NVIDIA + AMD/ROCm).
pub fn detect_gpus() -> Vec<Gpu> {
    let mut gpus = nvidia::detect();
    gpus.extend(amd::detect());
    gpus
}

/// Collect a full node telemetry snapshot.
pub fn collect(node_uuid: Option<String>, cluster_uuid: Option<String>) -> NodeInfo {
    let gpus = detect_gpus();
    let telemetry_valid = gpus.iter().any(|g| g.vram_used_bytes.is_some() || g.utilization_percent.is_some());
    NodeInfo {
        host_uuid: host::host_uuid(),
        node_uuid,
        cluster_uuid,
        cpu: host::cpu(),
        memory: host::memory(),
        gpus,
        telemetry_valid,
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

    pub fn cpu() -> Option<Cpu> {
        let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        let mut model = None;
        let mut logical = 0u32;
        let mut physical_ids = std::collections::BTreeSet::new();
        let mut cores_per_socket = None;
        for line in text.lines() {
            if let Some((k, v)) = line.split_once(':') {
                let (k, v) = (k.trim(), v.trim());
                match k {
                    "model name" if model.is_none() => model = Some(v.to_string()),
                    "processor" => logical += 1,
                    "physical id" => {
                        physical_ids.insert(v.to_string());
                    }
                    "cpu cores" if cores_per_socket.is_none() => {
                        cores_per_socket = v.parse::<u32>().ok()
                    }
                    _ => {}
                }
            }
        }
        let sockets = physical_ids.len().max(1) as u32;
        let total_cores = cores_per_socket.map(|c| c * sockets);
        Some(Cpu {
            model: model.unwrap_or_else(|| "unknown".into()),
            total_cores,
            total_threads: if logical > 0 { Some(logical) } else { None },
            utilization_percent: None,
        })
    }

    pub fn memory() -> Option<MemoryInfo> {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        let mut total_kb = None;
        let mut avail_kb = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                total_kb = rest.split_whitespace().next().and_then(|n| n.parse::<u64>().ok());
            } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                avail_kb = rest.split_whitespace().next().and_then(|n| n.parse::<u64>().ok());
            }
        }
        let total = total_kb? * 1024;
        let used = avail_kb.map(|a| total.saturating_sub(a * 1024));
        Some(MemoryInfo { total_bytes: total, used_bytes: used })
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
