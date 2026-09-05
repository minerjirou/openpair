//! EAP-NOOB integrity: HMAC-SHA256 (MACs/MACp), SHA-256 (Hoob/NoobId), and the
//! 17-element association array they are computed over (RFC 9140 §3.3.2).
//!
//! The array layout and the leading-element convention are confirmed against the
//! upstream implementation (Apache-2.0): elements are the verbatim JSON values
//! of the exchanged fields, concatenated into a whitespace-free JSON array;
//! absent fields serialize as `""`; element 12 is the literal KeyingMode `0`
//! (Completion Exchange).

use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256(key, data) -> 32 bytes.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(data);
    m.finalize().into_bytes().into()
}

/// SHA-256(data) -> 32 bytes.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}

/// Constant-time comparison of two MACs.
pub fn mac_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// The exchanged fields that make up the Hoob/MAC input array. Each is the JSON
/// value as it appears on the wire (base64url nonces are JSON strings). Absent
/// fields default to the empty JSON string `""`.
#[derive(Debug, Clone)]
pub struct MacInputs {
    pub vers: Value,
    pub verp: Value,
    pub peer_id: Value,
    pub cryptosuites: Value,
    pub dirs: Value,
    pub server_info: Value,
    pub cryptosuitep: Value,
    pub dirp: Value,
    pub nai: Value,
    pub peer_info: Value,
    pub pks: Value,
    pub ns: Value,
    pub pkp: Value,
    pub np: Value,
    pub noob: Value,
}

impl Default for MacInputs {
    fn default() -> Self {
        let e = || Value::String(String::new());
        MacInputs {
            vers: e(),
            verp: e(),
            peer_id: e(),
            cryptosuites: e(),
            dirs: e(),
            server_info: e(),
            cryptosuitep: e(),
            dirp: e(),
            nai: e(),
            peer_info: e(),
            pks: e(),
            ns: e(),
            pkp: e(),
            np: e(),
            noob: e(),
        }
    }
}

fn empty_to_str(v: &Value) -> Value {
    if v.is_null() {
        Value::String(String::new())
    } else {
        v.clone()
    }
}

/// Build the 17-element association array (RFC 9140 §3.3.2), serialized as a
/// whitespace-free JSON array. `lead` is the leading element (dir for Hoob,
/// 2 for MACs, 1 for MACp). Element index 11 is the literal KeyingMode `0`.
pub fn noob_array(lead: Value, m: &MacInputs) -> Vec<u8> {
    let arr = Value::Array(vec![
        lead,
        empty_to_str(&m.vers),
        empty_to_str(&m.verp),
        empty_to_str(&m.peer_id),
        empty_to_str(&m.cryptosuites),
        empty_to_str(&m.dirs),
        empty_to_str(&m.server_info),
        empty_to_str(&m.cryptosuitep),
        empty_to_str(&m.dirp),
        empty_to_str(&m.nai),
        empty_to_str(&m.peer_info),
        json!(0), // KeyingMode = 0 (Completion Exchange)
        empty_to_str(&m.pks),
        empty_to_str(&m.ns),
        empty_to_str(&m.pkp),
        empty_to_str(&m.np),
        empty_to_str(&m.noob),
    ]);
    serde_json::to_vec(&arr).expect("json array serializes")
}

/// Completion-Exchange MAC (RFC 9140 §3.3.2), truncated to 32 bytes.
/// `lead` = 2 for MACs (Kms), 1 for MACp (Kmp).
pub fn compute_mac(key: &[u8], lead: i64, m: &MacInputs) -> [u8; 32] {
    hmac_sha256(key, &noob_array(json!(lead), m))
}

/// Out-of-band fingerprint Hoob = SHA-256(array with lead=dir)[:16].
pub fn compute_hoob(dir: i64, m: &MacInputs) -> [u8; 16] {
    let full = sha256(&noob_array(json!(dir), m));
    let mut out = [0u8; 16];
    out.copy_from_slice(&full[..16]);
    out
}

/// NoobId = SHA-256(["NoobId", Noob])[:16], where `noob` is its base64url JSON
/// string form.
pub fn compute_noob_id(noob: &Value) -> [u8; 16] {
    let arr = Value::Array(vec![Value::String("NoobId".into()), noob.clone()]);
    let full = sha256(&serde_json::to_vec(&arr).expect("json"));
    let mut out = [0u8; 16];
    out.copy_from_slice(&full[..16]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_rfc_shape() {
        let m = hmac_sha256(b"key", b"data");
        assert_eq!(m.len(), 32);
        assert_eq!(m, hmac_sha256(b"key", b"data"));
        assert_ne!(m, hmac_sha256(b"key2", b"data"));
    }

    #[test]
    fn mac_equal_is_length_safe() {
        assert!(mac_equal(&[1, 2, 3], &[1, 2, 3]));
        assert!(!mac_equal(&[1, 2, 3], &[1, 2, 4]));
        assert!(!mac_equal(&[1, 2], &[1, 2, 3]));
    }

    #[test]
    fn noob_array_is_compact_17_elements() {
        let m = MacInputs {
            ns: json!("bnM"), // base64url-ish
            np: json!("bnA"),
            noob: json!("Tm9vYg"),
            ..MacInputs::default()
        };
        let bytes = noob_array(json!(2), &m);
        let s = String::from_utf8(bytes).unwrap();
        // whitespace-free, KeyingMode literal 0 present, leading element 2
        assert!(s.starts_with("[2,"));
        assert!(!s.contains(' '));
        assert!(s.contains(",0,")); // KeyingMode
        let v: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 17);
    }

    #[test]
    fn macs_and_macp_differ_by_lead_and_key() {
        let m = MacInputs::default();
        let macs = compute_mac(b"Kms", 2, &m);
        let macp = compute_mac(b"Kmp", 1, &m);
        assert_ne!(macs, macp);
        // Same key+lead+inputs is deterministic.
        assert_eq!(macs, compute_mac(b"Kms", 2, &m));
    }

    #[test]
    fn noob_id_shape() {
        let id = compute_noob_id(&json!("Tm9vYg"));
        assert_eq!(id.len(), 16);
    }
}
