//! EAP-NOOB integrity: HMAC-SHA256 (MACs/MACp), SHA-256 (Hoob/NoobId), and the
//! 17-element association array they are computed over (RFC 9140 §3.3.2).
//!
//! Confirmed against the upstream implementation (Apache-2.0). To match the peer
//! byte-for-byte, each element is the **verbatim** JSON of the corresponding
//! exchanged field (as it appeared on the wire), concatenated into a
//! whitespace-free JSON array; an absent field is emitted as `""`; element 12 is
//! the literal KeyingMode `0` (Completion Exchange).

use hmac::{Hmac, Mac};
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

/// The exchanged fields that make up the Hoob/MAC input array. Each is the
/// **verbatim** JSON of the field as it appeared on the wire (e.g. `[1]` for
/// Vers, `"abc"` for a base64url nonce, `{"kty":...}` for a JWK). An empty
/// string means the field was absent and is emitted as `""`.
#[derive(Debug, Clone, Default)]
pub struct MacInputs {
    pub vers: String,
    pub verp: String,
    pub peer_id: String,
    pub cryptosuites: String,
    pub dirs: String,
    pub server_info: String,
    pub cryptosuitep: String,
    pub dirp: String,
    pub nai: String,
    pub peer_info: String,
    pub pks: String,
    pub ns: String,
    pub pkp: String,
    pub np: String,
    pub noob: String,
}

fn el(s: &str) -> &str {
    if s.is_empty() {
        "\"\""
    } else {
        s
    }
}

/// Build the 17-element association array (RFC 9140 §3.3.2) as whitespace-free
/// JSON bytes. `lead` is the verbatim leading element (`"2"` for MACs, `"1"` for
/// MACp, `"<dir>"` for Hoob). Element index 11 is the literal KeyingMode `0`.
pub fn noob_array(lead: &str, m: &MacInputs) -> Vec<u8> {
    let parts: [&str; 17] = [
        lead,
        el(&m.vers),
        el(&m.verp),
        el(&m.peer_id),
        el(&m.cryptosuites),
        el(&m.dirs),
        el(&m.server_info),
        el(&m.cryptosuitep),
        el(&m.dirp),
        el(&m.nai),
        el(&m.peer_info),
        "0", // KeyingMode = 0 (Completion Exchange)
        el(&m.pks),
        el(&m.ns),
        el(&m.pkp),
        el(&m.np),
        el(&m.noob),
    ];
    let mut out = Vec::with_capacity(64);
    out.push(b'[');
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(p.as_bytes());
    }
    out.push(b']');
    out
}

/// Completion-Exchange MAC (RFC 9140 §3.3.2), truncated to 32 bytes.
/// `lead` = 2 for MACs (Kms), 1 for MACp (Kmp).
pub fn compute_mac(key: &[u8], lead: i64, m: &MacInputs) -> [u8; 32] {
    hmac_sha256(key, &noob_array(&lead.to_string(), m))
}

/// Out-of-band fingerprint Hoob = SHA-256(array with lead=dir)[:16].
pub fn compute_hoob(dir: i64, m: &MacInputs) -> [u8; 16] {
    let full = sha256(&noob_array(&dir.to_string(), m));
    let mut out = [0u8; 16];
    out.copy_from_slice(&full[..16]);
    out
}

/// NoobId = SHA-256(["NoobId", Noob])[:16], where `noob_json` is the base64url
/// JSON **string** form of Noob (verbatim, including quotes).
pub fn compute_noob_id(noob_json: &str) -> [u8; 16] {
    let mut arr = Vec::new();
    arr.extend_from_slice(b"[\"NoobId\",");
    arr.extend_from_slice(noob_json.as_bytes());
    arr.push(b']');
    let full = sha256(&arr);
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
    fn noob_array_is_verbatim_17_elements() {
        let m = MacInputs {
            vers: "[1]".into(),
            ns: "\"bnM\"".into(),
            np: "\"bnA\"".into(),
            noob: "\"Tm9vYg\"".into(),
            ..MacInputs::default()
        };
        let bytes = noob_array("2", &m);
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("[2,[1],"));
        assert!(!s.contains(' '));
        assert!(s.contains(",0,")); // KeyingMode literal
        assert!(s.ends_with(",\"Tm9vYg\"]"));
        // Absent fields emitted as "".
        assert!(s.contains(",\"\","));
    }

    #[test]
    fn macs_and_macp_differ_by_lead_and_key() {
        let m = MacInputs::default();
        let macs = compute_mac(b"Kms", 2, &m);
        let macp = compute_mac(b"Kmp", 1, &m);
        assert_ne!(macs, macp);
        assert_eq!(macs, compute_mac(b"Kms", 2, &m));
    }

    #[test]
    fn noob_id_shape() {
        let id = compute_noob_id("\"Tm9vYg\"");
        assert_eq!(id.len(), 16);
    }
}
