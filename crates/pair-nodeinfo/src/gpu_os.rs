//! OS-level GPU inventory (no vendor compute tools required).
//!
//! Mirrors the reference's approach of enumerating GPUs from the operating
//! system so AMD (and Intel) GPUs appear even without ROCm tools installed:
//! * Windows: `Win32_VideoController` via CIM/WMI
//! * macOS:   `system_profiler SPDisplaysDataType -json`
//! * Linux:   handled by the amdgpu sysfs path in [`crate::amd`]
//!
//! Vendor telemetry tools (nvidia-smi / amd-smi / rocm-smi) remain the source of
//! live utilization + VRAM-used; this only provides static inventory.

use pair_proto::{Gpu, GpuVendor};

/// Enumerate GPUs from the OS. Empty on Linux (use the amdgpu sysfs path).
pub fn os_gpus() -> Vec<Gpu> {
    #[cfg(windows)]
    {
        windows_gpus()
    }
    #[cfg(target_os = "macos")]
    {
        macos_gpus()
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn windows_gpus() -> Vec<Gpu> {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,PNPDeviceID | ConvertTo-Json -Compress",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_win32_video(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Parse `Win32_VideoController` CIM JSON (single object or array).
pub fn parse_win32_video(json: &str) -> Vec<Gpu> {
    let v: serde_json::Value = match serde_json::from_str(json.trim()) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let items = match &v {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(_) => vec![v],
        _ => return Vec::new(),
    };
    items
        .iter()
        .filter_map(|it| {
            let name = it.get("Name")?.as_str()?.trim().to_string();
            let vram_bytes = it.get("AdapterRAM").and_then(|r| r.as_u64()).filter(|&n| n > 0);
            let pnp = it.get("PNPDeviceID").and_then(|p| p.as_str()).unwrap_or("");
            let vendor = vendor_from_pnp(pnp);
            let vendor_id = vendor_id_from_pnp(pnp);
            Some(Gpu {
                uuid: pnp.to_string(),
                name,
                vendor,
                vendor_id,
                vram_bytes,
                vram_used_bytes: None,
                utilization_percent: None,
            })
        })
        .collect()
}

/// Extract a `VEN_XXXX` PCI vendor id (hex) from a Windows PNPDeviceID.
fn vendor_id_from_pnp(pnp: &str) -> Option<u16> {
    let up = pnp.to_uppercase();
    let idx = up.find("VEN_")? + 4;
    let hex: String = up[idx..].chars().take(4).collect();
    u16::from_str_radix(&hex, 16).ok()
}

fn vendor_from_pnp(pnp: &str) -> GpuVendor {
    vendor_id_from_pnp(pnp).map(GpuVendor::from_pci_id).unwrap_or(GpuVendor::Other)
}

#[cfg(target_os = "macos")]
fn macos_gpus() -> Vec<Gpu> {
    let out = std::process::Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_macos_displays(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

/// Parse `system_profiler SPDisplaysDataType -json`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn parse_macos_displays(json: &str) -> Vec<Gpu> {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v.get("SPDisplaysDataType")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| {
                    let name = g
                        .get("sppci_model")
                        .or_else(|| g.get("_name"))?
                        .as_str()?
                        .to_string();
                    let vendor = if name.to_lowercase().contains("amd") || name.to_lowercase().contains("radeon") {
                        GpuVendor::Amd
                    } else if name.to_lowercase().contains("nvidia") {
                        GpuVendor::Nvidia
                    } else if name.to_lowercase().contains("apple") || name.to_lowercase().contains("intel") {
                        GpuVendor::Intel
                    } else {
                        GpuVendor::Other
                    };
                    Some(Gpu {
                        uuid: name.clone(),
                        name,
                        vendor,
                        vendor_id: None,
                        vram_bytes: None,
                        vram_used_bytes: None,
                        utilization_percent: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_win32_array_and_vendor() {
        let json = r#"[
          {"Name":"NVIDIA GeForce RTX 5070","AdapterRAM":4293918720,"PNPDeviceID":"PCI\\VEN_10DE&DEV_2F04&SUBSYS_00000000"},
          {"Name":"AMD Radeon(TM) Graphics","AdapterRAM":536870912,"PNPDeviceID":"PCI\\VEN_1002&DEV_164E"}
        ]"#;
        let g = parse_win32_video(json);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].vendor, GpuVendor::Nvidia);
        assert_eq!(g[0].vendor_id, Some(0x10de));
        assert_eq!(g[1].vendor, GpuVendor::Amd);
        assert_eq!(g[1].vendor_id, Some(0x1002));
        assert_eq!(g[1].vram_bytes, Some(536870912));
    }

    #[test]
    fn parse_win32_single_object() {
        let json = r#"{"Name":"AMD Radeon(TM) Graphics","AdapterRAM":536870912,"PNPDeviceID":"PCI\\VEN_1002&DEV_164E"}"#;
        let g = parse_win32_video(json);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].vendor, GpuVendor::Amd);
    }

    #[test]
    fn macos_parse() {
        let json = r#"{"SPDisplaysDataType":[{"_name":"AMD Radeon Pro 5500M","sppci_model":"AMD Radeon Pro 5500M"}]}"#;
        let g = parse_macos_displays(json);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].vendor, GpuVendor::Amd);
    }
}
