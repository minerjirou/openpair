//! EAP-NOOB (RFC 9140) PIN pairing -- clean-room.
//!
//! Confirmed primitives (implemented + tested here):
//! * cryptosuites 1 (X25519/OKP) and 2 (P-256/EC), ECDH -- [`suite`]
//! * NIST SP 800-56C one-step KDF, SHA-256, 320-byte output -- [`kdf`]
//! * HMAC-SHA256 MACs and SHA-256 Hoob/NoobId -- [`mac`]
//! * wire messages with byte-confirmed field names, base64url -- [`messages`]
//!
//! Message sequence (byte-confirmed `Type` dispatch 1..=6):
//! Discovery -> Negotiation -> KeyExchange -> Waiting -> NoobID -> Completion.
//! The out-of-band value (`Noob`, delivered via the PIN channel) plus the ECDH
//! `Z` feed the KDF; `Kms`/`Kmp` key the confirmation MACs.
//!
//! Byte-exact serializations confirmed against the upstream implementation
//! (Apache-2.0): the KDF FixedInfo (`"EAP-NOOB" || Np || Ns || len(Noob) ||
//! Noob`), the 320-byte output split, and the 17-element MACs/MACp/Hoob
//! association array. The reconnect exchange (KeyingMode 3, Kz) is not yet
//! modelled.

pub mod kdf;
pub mod mac;
pub mod messages;
pub mod suite;

pub use kdf::{one_step_kdf_sha256, DerivedKeys, EAPNOOB_OUTPUT_LEN};
pub use messages::{MsgType, WireMessage};
pub use suite::{Jwk, KeyPair, Suite};

/// Role in the EAP-NOOB exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The inviting side (EAP server).
    Server,
    /// The joining side (EAP peer).
    Peer,
}

/// High-level phase tracker for the handshake state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Discovery,
    Negotiation,
    KeyExchange,
    WaitingForOob,
    NoobId,
    Completion,
    Done,
}

impl Phase {
    /// The next phase on success, per the RFC 9140 sequence.
    pub fn next(self) -> Phase {
        match self {
            Phase::Discovery => Phase::Negotiation,
            Phase::Negotiation => Phase::KeyExchange,
            Phase::KeyExchange => Phase::WaitingForOob,
            Phase::WaitingForOob => Phase::NoobId,
            Phase::NoobId => Phase::Completion,
            Phase::Completion => Phase::Done,
            Phase::Done => Phase::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_sequence() {
        let mut p = Phase::Discovery;
        for expected in [
            Phase::Negotiation,
            Phase::KeyExchange,
            Phase::WaitingForOob,
            Phase::NoobId,
            Phase::Completion,
            Phase::Done,
        ] {
            p = p.next();
            assert_eq!(p, expected);
        }
    }

    /// End-to-end of the *confirmed* crypto: both sides derive identical keys
    /// from ECDH Z + shared Noob via the one-step KDF, and agree on a MAC.
    #[test]
    fn both_sides_derive_same_keys() {
        let server = Suite::X25519.generate();
        let peer = Suite::X25519.generate();
        let z_s = server.compute_z(&peer.public_jwk).unwrap();
        let z_p = peer.compute_z(&server.public_jwk).unwrap();
        assert_eq!(z_s, z_p);

        let (np, ns, noob) = (b"peer-nonce", b"server-nonce", b"oob-noob-value");
        let fi = kdf::eapnoob_fixed_info(np, ns, noob);
        let out_s = one_step_kdf_sha256(&z_s, &fi, EAPNOOB_OUTPUT_LEN);
        let out_p = one_step_kdf_sha256(&z_p, &fi, EAPNOOB_OUTPUT_LEN);
        assert_eq!(out_s, out_p);

        let ks = DerivedKeys::from_output(&out_s).unwrap();
        let kp = DerivedKeys::from_output(&out_p).unwrap();
        assert_eq!(ks.kms, kp.kms);
        assert_eq!(ks.kz, kp.kz);

        // Both sides compute the confirmation MAC (MACs, lead=2) over the same
        // association inputs with the derived Kms.
        let inputs = mac::MacInputs::default();
        assert!(mac::mac_equal(
            &mac::compute_mac(&ks.kms, 2, &inputs),
            &mac::compute_mac(&kp.kms, 2, &inputs)
        ));
    }
}
