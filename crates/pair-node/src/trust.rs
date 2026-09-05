//! Dev-only peer-trust bootstrap for local multi-node testing.
//!
//! Real trust is established by pairing (EAP-NOOB) + certificate pinning. Until
//! that handshake is byte-exact against the reference, this helper lets several
//! `openpair-node` instances on one machine trust each other: each publishes its
//! certificate PEM as `<node-uuid>.pem` in a shared directory and pins every
//! other `*.pem` it finds. This is NOT a substitute for pairing and must never
//! be pointed at an untrusted directory.

use pair_trust::{Identity, SharedPins};
use std::path::Path;

/// Publish our cert into `dir` and pin all other node certs found there.
/// Returns the number of peers pinned.
pub fn bootstrap_dev_trust(
    dir: &Path,
    identity: &Identity,
    pins: &SharedPins,
) -> anyhow::Result<usize> {
    std::fs::create_dir_all(dir)?;
    let ours = dir.join(format!("{}.pem", identity.node_uuid));
    std::fs::write(&ours, &identity.cert_pem)?;

    let mut pinned = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("pem") {
            continue;
        }
        if path == ours {
            continue;
        }
        let pem = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Some(der) = pem_cert_to_der(&pem) {
            let mut store = pins.write().expect("pin store poisoned");
            if store.pin(&der).is_ok() {
                pinned += 1;
            }
        }
    }
    Ok(pinned)
}

fn pem_cert_to_der(pem: &str) -> Option<Vec<u8>> {
    let mut b64 = String::new();
    let mut in_block = false;
    for line in pem.lines() {
        if line.starts_with("-----BEGIN CERTIFICATE-----") {
            in_block = true;
            continue;
        }
        if line.starts_with("-----END CERTIFICATE-----") {
            break;
        }
        if in_block {
            b64.push_str(line.trim());
        }
    }
    if b64.is_empty() {
        return None;
    }
    base64_decode(&b64)
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, &c) in T.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &c in s.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = rev[c as usize];
        if v == 255 {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}
