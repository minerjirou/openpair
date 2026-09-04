//! EAP-NOOB integrity primitives: HMAC-SHA256 (for MACs/MACp) and SHA-256
//! (for Hoob/NoobId).
//!
//! Confirmed: HMAC-SHA256 keyed by Kms (server) / Kmp (peer) over the ordered
//! association-data array; Hoob/NoobId are SHA-256 based, base64url encoded.

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

/// Compute the message-authentication code over the EAP-NOOB association data.
///
/// TODO(interop, needs dynamic confirmation): the exact 17-element association
/// array (RFC 9140 section 3.3.2 MACs/MACp) -- element order, which fields are
/// raw vs. JSON-quoted, and the leading `Dir` value -- is confirmed only in
/// outline. The caller passes already-serialized association bytes here;
/// assembling them to byte-match the reference is the remaining dynamic step.
pub fn compute_mac(km: &[u8], association_data: &[u8]) -> [u8; 32] {
    hmac_sha256(km, association_data)
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
}
