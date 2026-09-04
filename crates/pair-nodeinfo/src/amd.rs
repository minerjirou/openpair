//! AMD / ROCm GPU backend.
//!
//! This is the ROCm-support extension the reference (NVIDIA-only) node lacks.
//! It fills the exact same telemetry fields (`vram_bytes`, `vram_used_bytes`,
//! `utilization_percent`) the rest of the cluster expects, so an AMD node is
//! indistinguishable to peers/scheduler from an NVIDIA one.
//!
//! Two tool generations are supported, tried in order:
//!   1. `amd-smi` (ROCm 6+): `amd-smi static --json` + `amd-smi metric --json`
//!   2. `rocm-smi` (legacy): `rocm-smi --showproductname --showmeminfo vram
//!      --showuse --json` (reports VRAM already in bytes)
//! Missing tools are not an error: the node reports no AMD GPUs.

use pair_proto::{Gpu, GpuVendor};
use serde_json::Value;
use std::process::Command;

const MB: u64 = 1_000_000; // amd-smi reports MB in decimal megabytes

/// Detect AMD GPUs. Tries amd-smi, then rocm-smi; empty if neither is present.
pub fn detect() -> Vec<Gpu> {
    if let (Some(stat), metric) = (
        run("amd-smi", &["static", "--json"]),
        run("amd-smi", &["metric", "--json"]),
    ) {
        let gpus = parse_amd_smi(&stat, metric.as_deref());
        if !gpus.is_empty() {
            return gpus;
        }
    }
    if let Some(js) = run(
        "rocm-smi",
        &["--showproductname", "--showmeminfo", "vram", "--showuse", "--json"],
    ) {
        return parse_rocm_smi_json(&js);
    }
    Vec::new()
}

fn run(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `amd-smi static --json` (+ optional `amd-smi metric --json`).
/// Both are arrays of per-GPU objects keyed by a `gpu` index.
pub fn parse_amd_smi(static_json: &str, metric_json: Option<&str>) -> Vec<Gpu> {
    let stat: Value = match serde_json::from_str(static_json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let metrics: Vec<Value> = metric_json
        .and_then(|m| serde_json::from_str::<Value>(m).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let entries = match stat.as_array() {
        Some(a) => a.clone(),
        None => return Vec::new(),
    };

    entries
        .iter()
        .map(|e| {
            let idx = e.get("gpu").and_then(Value::as_u64);
            let name = find_str(e, &["market_name", "product_name", "name"])
                .unwrap_or_else(|| "AMD GPU".to_string());
            let uuid = find_str(e, &["uuid", "serial", "bdf", "market_name"])
                .unwrap_or_else(|| format!("amd-gpu-{}", idx.unwrap_or(0)));
            let vram_bytes = find_num(e, &["size", "total", "vram_total", "total_vram"]).map(mb_to_bytes);

            let metric = idx.and_then(|i| {
                metrics.iter().find(|m| m.get("gpu").and_then(Value::as_u64) == Some(i))
            });
            let (util, used) = metric
                .map(|m| {
                    (
                        find_num(m, &["gfx_activity", "gfx", "gpu_activity", "usage"]).map(|v| v as u32),
                        find_num(m, &["used_vram", "vram_used", "used"]).map(mb_to_bytes),
                    )
                })
                .unwrap_or((None, None));

            Gpu {
                uuid,
                name,
                vendor: GpuVendor::Amd,
                vendor_id: Some(0x1002),
                vram_bytes,
                vram_used_bytes: used,
                utilization_percent: util,
            }
        })
        .collect()
}

/// Parse legacy `rocm-smi --json`. Object keyed by "card0", "card1", ...;
/// VRAM keys are already in bytes.
pub fn parse_rocm_smi_json(json: &str) -> Vec<Gpu> {
    let v: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let obj = match v.as_object() {
        Some(o) => o,
        None => return Vec::new(),
    };
    let mut cards: Vec<(&String, &Value)> =
        obj.iter().filter(|(k, _)| k.starts_with("card")).collect();
    cards.sort_by(|a, b| a.0.cmp(b.0));

    cards
        .iter()
        .map(|(key, c)| {
            let name = find_str(c, &["Card Series", "Device Name", "Market Name", "Card Model"])
                .unwrap_or_else(|| "AMD GPU".to_string());
            let uuid = find_str(c, &["Unique ID", "Serial Number", "PCI Bus"])
                .unwrap_or_else(|| (*key).clone());
            let vram_bytes = find_str_num(c, "VRAM Total Memory (B)");
            let used = find_str_num(c, "VRAM Total Used Memory (B)");
            let util = find_str(c, &["GPU use (%)"]).and_then(|s| s.trim().parse::<u32>().ok());
            Gpu {
                uuid,
                name,
                vendor: GpuVendor::Amd,
                vendor_id: Some(0x1002),
                vram_bytes,
                vram_used_bytes: used,
                utilization_percent: util,
            }
        })
        .collect()
}

fn mb_to_bytes(mb: f64) -> u64 {
    (mb * MB as f64) as u64
}

/// Recursively search an object for the first key in `keys` whose value is a
/// string (or `{value: ...}` wrapper). amd-smi nests values as {value, unit}.
fn find_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(found) = deep_find(v, k) {
            if let Some(s) = found.as_str() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Like `find_str` but coerces numeric / {value} wrappers to f64.
fn find_num(v: &Value, keys: &[&str]) -> Option<f64> {
    for k in keys {
        if let Some(found) = deep_find(v, k) {
            if let Some(n) = found.as_f64() {
                return Some(n);
            }
            if let Some(inner) = found.get("value").and_then(Value::as_f64) {
                return Some(inner);
            }
        }
    }
    None
}

/// Parse a stringified integer value (rocm-smi encodes byte counts as strings).
fn find_str_num(v: &Value, key: &str) -> Option<u64> {
    deep_find(v, key).and_then(|f| {
        f.as_str()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .or_else(|| f.as_u64())
    })
}

/// Depth-first search for `key` anywhere in a JSON tree.
fn deep_find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if let Some(hit) = map.get(key) {
                return Some(hit);
            }
            for (_, child) in map {
                if let Some(hit) = deep_find(child, key) {
                    return Some(hit);
                }
            }
            None
        }
        Value::Array(a) => a.iter().find_map(|child| deep_find(child, key)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rocm_smi_bytes() {
        let js = r#"{
          "card0": {
            "Card Series": "Radeon RX 7900 XTX",
            "Unique ID": "0xabc123",
            "VRAM Total Memory (B)": "25753026560",
            "VRAM Total Used Memory (B)": "1288490188",
            "GPU use (%)": "42"
          }
        }"#;
        let g = parse_rocm_smi_json(js);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "Radeon RX 7900 XTX");
        assert_eq!(g[0].uuid, "0xabc123");
        assert_eq!(g[0].vram_bytes, Some(25_753_026_560));
        assert_eq!(g[0].vram_used_bytes, Some(1_288_490_188));
        assert_eq!(g[0].utilization_percent, Some(42));
        assert_eq!(g[0].vendor, GpuVendor::Amd);
    }

    #[test]
    fn amd_smi_nested_value_unit() {
        let stat = r#"[{"gpu":0,"asic":{"market_name":"Instinct MI300X"},
                        "vram":{"size":{"value":196608,"unit":"MB"}},
                        "board":{"serial":"SN-1"}}]"#;
        let metric = r#"[{"gpu":0,"usage":{"gfx_activity":{"value":88,"unit":"%"}},
                          "mem_usage":{"used_vram":{"value":40960,"unit":"MB"}}}]"#;
        let g = parse_amd_smi(stat, Some(metric));
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "Instinct MI300X");
        assert_eq!(g[0].uuid, "SN-1");
        assert_eq!(g[0].vram_bytes, Some(196608 * MB));
        assert_eq!(g[0].vram_used_bytes, Some(40960 * MB));
        assert_eq!(g[0].utilization_percent, Some(88));
    }
}
