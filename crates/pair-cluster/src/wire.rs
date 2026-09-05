//! The `/v1/cluster/pairing` wire envelope (§7.2).
//!
//! Each request/response carries exactly one EAP-NOOB message on the pairing
//! channel. `msg` is the base64 (standard alphabet) of the opaque EAP-NOOB blob
//! (the compact JSON produced by the state machine), or empty for the Completion
//! kickoff. The Initial and Completion exchanges use plain HTTP (there is no
//! trust yet); post-commit ack/fail signals reuse the same path over mTLS.

use serde::{Deserialize, Serialize};

/// HTTP path of the pairing channel.
pub const PAIRING_PATH: &str = "/v1/cluster/pairing";

/// Default inter-node pairing port (upstream appends 14321 to the invite host).
pub const DEFAULT_PAIRING_PORT: u16 = 14321;

/// Pairing phases (the `phase` field). `initial` and `completion` carry the
/// EAP-NOOB handshake; the rest are out-of-band terminal/liveness signals.
pub mod phase {
    pub const INITIAL: &str = "initial";
    pub const COMPLETION: &str = "completion";
    pub const CANCEL: &str = "cancel";
    pub const DECLINE: &str = "decline";
    pub const FAIL: &str = "fail";
    pub const ACK: &str = "ack";
    pub const EXPIRED: &str = "expired";
}

/// Recognized `reason` codes carried on a terminal signal.
pub mod reason {
    /// The joiner entered the wrong PIN (EAP-NOOB MAC / NoobId mismatch).
    pub const INCORRECT_PIN: &str = "incorrect-pin";
    /// The joiner is already a member of another cluster.
    pub const ALREADY_CLUSTERED: &str = "already-clustered";
}

/// One message on the pairing channel.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PairingEnvelope {
    #[serde(rename = "inviteId")]
    pub invite_id: String,
    pub phase: String,
    /// base64 (standard) of the EAP-NOOB blob; empty when there is nothing to
    /// send (Completion kickoff, or a terminal ack).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub msg: String,
    /// Set by a joiner that explicitly refuses the pairing (vs. a transport
    /// error), carried with a `reason`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub rejected: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl PairingEnvelope {
    /// A request/response carrying an EAP-NOOB blob for `phase`.
    pub fn with_msg(invite_id: &str, phase: &str, blob: &[u8]) -> Self {
        Self {
            invite_id: invite_id.to_string(),
            phase: phase.to_string(),
            msg: encode_msg(blob),
            ..Default::default()
        }
    }

    /// An empty-`msg` envelope (Completion kickoff, or a terminal signal).
    pub fn signal(invite_id: &str, phase: &str, reason: &str) -> Self {
        Self {
            invite_id: invite_id.to_string(),
            phase: phase.to_string(),
            reason: reason.to_string(),
            ..Default::default()
        }
    }

    /// A `rejected` response carrying a reason.
    pub fn rejected(reason: &str) -> Self {
        Self {
            rejected: true,
            reason: reason.to_string(),
            ..Default::default()
        }
    }

    /// Decode the carried EAP-NOOB blob (empty vec if `msg` is empty/invalid).
    pub fn blob(&self) -> Vec<u8> {
        if self.msg.is_empty() {
            Vec::new()
        } else {
            decode_msg(&self.msg).unwrap_or_default()
        }
    }
}

/// base64-encode (standard alphabet) an EAP-NOOB blob for the `msg` field.
pub fn encode_msg(blob: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.encode(blob)
}

/// Decode a base64 (standard alphabet) `msg` field.
pub fn decode_msg(s: &str) -> Option<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    STANDARD.decode(s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let e = PairingEnvelope::with_msg("inv-1", phase::INITIAL, b"{\"Type\":1}");
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"inviteId\":\"inv-1\""));
        assert!(j.contains("\"phase\":\"initial\""));
        let back: PairingEnvelope = serde_json::from_str(&j).unwrap();
        assert_eq!(back.blob(), b"{\"Type\":1}");
    }

    #[test]
    fn kickoff_has_empty_msg() {
        let e = PairingEnvelope::signal("inv-1", phase::COMPLETION, "");
        let j = serde_json::to_string(&e).unwrap();
        assert!(!j.contains("\"msg\""));
        assert!(e.blob().is_empty());
    }

    #[test]
    fn rejected_shape() {
        let e = PairingEnvelope::rejected(reason::ALREADY_CLUSTERED);
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"rejected\":true"));
        assert!(j.contains("already-clustered"));
    }
}
