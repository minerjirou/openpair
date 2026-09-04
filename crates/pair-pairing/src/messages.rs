//! EAP-NOOB wire messages (RFC 9140) as exchanged over `/pairing` + `/invite*`.
//!
//! Field names are byte-confirmed from rodata tags. `Type` selects the phase:
//! 1 Discovery, 2 Negotiation, 3 KeyExchange, 4 Waiting, 5 NoobID, 6 Completion
//! (7-9 = Kz reconnect, not yet modelled). All binary values are base64url.

use crate::suite::Jwk;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

/// EAP-NOOB message phase (the `Type` field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsgType {
    Discovery = 1,
    Negotiation = 2,
    KeyExchange = 3,
    Waiting = 4,
    NoobId = 5,
    Completion = 6,
}

impl MsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Discovery,
            2 => Self::Negotiation,
            3 => Self::KeyExchange,
            4 => Self::Waiting,
            5 => Self::NoobId,
            6 => Self::Completion,
            _ => return None,
        })
    }
}

/// A single EAP-NOOB exchange message. Fields are optional because each `Type`
/// carries a different subset (RFC 9140 section 3.3). Names match observed tags.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WireMessage {
    #[serde(rename = "Type")]
    pub type_: u8,
    #[serde(rename = "PeerId", default, skip_serializing_if = "Option::is_none")]
    pub peer_id: Option<String>,
    #[serde(rename = "Vers", default, skip_serializing_if = "Option::is_none")]
    pub vers: Option<Vec<u32>>,
    #[serde(rename = "Verp", default, skip_serializing_if = "Option::is_none")]
    pub verp: Option<u32>,
    #[serde(rename = "Cryptosuites", default, skip_serializing_if = "Option::is_none")]
    pub cryptosuites: Option<Vec<u8>>,
    #[serde(rename = "Cryptosuitep", default, skip_serializing_if = "Option::is_none")]
    pub cryptosuitep: Option<u8>,
    #[serde(rename = "Dirs", default, skip_serializing_if = "Option::is_none")]
    pub dirs: Option<u8>,
    #[serde(rename = "Dirp", default, skip_serializing_if = "Option::is_none")]
    pub dirp: Option<u8>,
    #[serde(rename = "NAI", default, skip_serializing_if = "Option::is_none")]
    pub nai: Option<String>,
    #[serde(rename = "PKs", default, skip_serializing_if = "Option::is_none")]
    pub pks: Option<Jwk>,
    #[serde(rename = "PKp", default, skip_serializing_if = "Option::is_none")]
    pub pkp: Option<Jwk>,
    /// Server nonce (base64url).
    #[serde(rename = "Ns", default, skip_serializing_if = "Option::is_none")]
    pub ns: Option<String>,
    /// Peer nonce (base64url).
    #[serde(rename = "Np", default, skip_serializing_if = "Option::is_none")]
    pub np: Option<String>,
    #[serde(rename = "NoobId", default, skip_serializing_if = "Option::is_none")]
    pub noob_id: Option<String>,
    #[serde(rename = "MACs", default, skip_serializing_if = "Option::is_none")]
    pub macs: Option<String>,
    #[serde(rename = "MACp", default, skip_serializing_if = "Option::is_none")]
    pub macp: Option<String>,
}

/// base64url (no padding) encode.
pub fn b64u(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// base64url (no padding) decode.
pub fn unb64u(s: &str) -> anyhow::Result<Vec<u8>> {
    Ok(URL_SAFE_NO_PAD.decode(s.as_bytes())?)
}

/// A 16-byte random nonce, base64url-encoded (RFC 9140 uses 16-byte Ns/Np).
pub fn fresh_nonce() -> (String, Vec<u8>) {
    use rand::RngCore;
    let mut n = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut n);
    (b64u(&n), n.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_roundtrip() {
        assert_eq!(MsgType::from_u8(3), Some(MsgType::KeyExchange));
        assert_eq!(MsgType::Completion as u8, 6);
    }

    #[test]
    fn wire_serializes_named_fields() {
        let (ns_b64, _) = fresh_nonce();
        let m = WireMessage {
            type_: 2,
            verp: Some(1),
            cryptosuitep: Some(1),
            ns: Some(ns_b64),
            ..Default::default()
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains("\"Type\":2"));
        assert!(s.contains("\"Cryptosuitep\":1"));
        assert!(s.contains("\"Ns\""));
        let back: WireMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(back.type_, 2);
    }

    #[test]
    fn b64url_roundtrip() {
        let data = [0u8, 1, 2, 250, 251, 255];
        assert_eq!(unb64u(&b64u(&data)).unwrap(), data);
    }
}
