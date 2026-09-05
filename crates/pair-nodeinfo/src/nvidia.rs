//! NVIDIA GPU backend via `nvidia-smi`.
//!
//! Reproduces the exact telemetry contract observed in the reference node:
//!   static : nvidia-smi --query-gpu=uuid,name,memory.total  --format=csv,noheader,nounits
//!   dynamic: nvidia-smi --query-gpu=uuid,utilization.gpu,memory.used --format=csv,noheader,nounits
//! `nounits` => memory values are bare integers in MiB; convert to bytes via <<20.
//! Missing `nvidia-smi` is not an error: the node simply reports no NVIDIA GPUs.

use pair_proto::{Gpu, GpuVendor};
use std::collections::BTreeMap;
use std::process::Command;

const MIB: u64 = 1 << 20;

/// Detect NVIDIA GPUs and their live telemetry. Returns empty if nvidia-smi is
/// absent or reports nothing.
pub fn detect() -> Vec<Gpu> {
    let statik = match run(&[
        "--query-gpu=uuid,name,memory.total",
        "--format=csv,noheader,nounits",
    ]) {
        Some(out) => parse_static(&out),
        None => return Vec::new(),
    };
    let dynamic = run(&[
        "--query-gpu=uuid,utilization.gpu,memory.used",
        "--format=csv,noheader,nounits",
    ])
    .map(|o| parse_dynamic(&o))
    .unwrap_or_default();
    merge(statik, &dynamic)
}

fn run(args: &[&str]) -> Option<String> {
    let out = Command::new("nvidia-smi").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `uuid, name, memory.total(MiB)` CSV rows into static GPU records.
pub fn parse_static(csv: &str) -> Vec<Gpu> {
    csv.lines()
        .filter_map(|line| {
            let f = split_csv(line);
            if f.len() < 3 {
                return None;
            }
            let vram = f[2].parse::<u64>().ok().map(|mib| mib * MIB);
            Some(Gpu {
                uuid: f[0].clone(),
                name: f[1].clone(),
                vendor: GpuVendor::Nvidia,
                vendor_id: Some(0x10de),
                vram_bytes: vram,
                vram_used_bytes: None,
                utilization_percent: None,
            })
        })
        .collect()
}

/// Parse `uuid, utilization.gpu(%), memory.used(MiB)` CSV rows keyed by uuid.
pub fn parse_dynamic(csv: &str) -> BTreeMap<String, (Option<u32>, Option<u64>)> {
    let mut m = BTreeMap::new();
    for line in csv.lines() {
        let f = split_csv(line);
        if f.len() < 3 {
            continue;
        }
        let util = f[1].parse::<u32>().ok();
        let used = f[2].parse::<u64>().ok().map(|mib| mib * MIB);
        m.insert(f[0].clone(), (util, used));
    }
    m
}

fn merge(mut statik: Vec<Gpu>, dynamic: &BTreeMap<String, (Option<u32>, Option<u64>)>) -> Vec<Gpu> {
    for g in &mut statik {
        if let Some((util, used)) = dynamic.get(&g.uuid) {
            g.utilization_percent = *util;
            g.vram_used_bytes = *used;
        }
    }
    statik
}

/// Split one `nvidia-smi` CSV row (comma+space separated, values may be "[N/A]").
fn split_csv(line: &str) -> Vec<String> {
    line.split(',')
        .map(|s| s.trim().to_string())
        .filter(|_| !line.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_static_mib_to_bytes() {
        let out = "GPU-1111, NVIDIA GeForce RTX 4090, 24564\nGPU-2222, NVIDIA RTX A6000, 49140";
        let g = parse_static(out);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].uuid, "GPU-1111");
        assert_eq!(g[0].name, "NVIDIA GeForce RTX 4090");
        assert_eq!(g[0].vram_bytes, Some(24564 * MIB));
        assert_eq!(g[0].vendor, GpuVendor::Nvidia);
    }

    #[test]
    fn parse_dynamic_and_merge() {
        let statik = parse_static("GPU-1111, RTX 4090, 24564");
        let dyn_ = parse_dynamic("GPU-1111, 55, 8000");
        let merged = merge(statik, &dyn_);
        assert_eq!(merged[0].utilization_percent, Some(55));
        assert_eq!(merged[0].vram_used_bytes, Some(8000 * MIB));
    }
}
