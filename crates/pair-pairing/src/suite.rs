//! EAP-NOOB cryptosuites and ECDH, with JWK public-key encoding.
//!
//! Confirmed suites (from `suiteByID` + rodata):
//! * **1** = X25519, JWK `{"kty":"OKP","crv":"X25519","x":…}`
//! * **2** = NIST P-256, JWK `{"kty":"EC","crv":"P-256","x":…,"y":…}`
//! All coordinates are base64url (no padding).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use p256::elliptic_curve::sec1::FromEncodedPoint;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    X25519,
    P256,
}

impl Suite {
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(Suite::X25519),
            2 => Some(Suite::P256),
            _ => None,
        }
    }
    pub fn id(self) -> u8 {
        match self {
            Suite::X25519 => 1,
            Suite::P256 => 2,
        }
    }
    /// Generate an ephemeral keypair for this suite.
    pub fn generate(self) -> KeyPair {
        match self {
            Suite::X25519 => {
                let secret = x25519_dalek::StaticSecret::random_from_rng(rand::thread_rng());
                let public = x25519_dalek::PublicKey::from(&secret);
                let jwk = Jwk {
                    kty: "OKP".into(),
                    crv: "X25519".into(),
                    x: URL_SAFE_NO_PAD.encode(public.as_bytes()),
                    y: None,
                };
                KeyPair { secret: SecretKey::X25519(secret), public_jwk: jwk }
            }
            Suite::P256 => {
                let secret = p256::ecdh::EphemeralSecret::random(&mut rand::thread_rng());
                let point = p256::EncodedPoint::from(secret.public_key());
                let x = point.x().expect("P-256 x");
                let y = point.y().expect("P-256 y (uncompressed)");
                let jwk = Jwk {
                    kty: "EC".into(),
                    crv: "P-256".into(),
                    x: URL_SAFE_NO_PAD.encode(x),
                    y: Some(URL_SAFE_NO_PAD.encode(y)),
                };
                KeyPair { secret: SecretKey::P256(secret), public_jwk: jwk }
            }
        }
    }
}

/// A JWK public key as it appears on the wire (`PKs`/`PKp`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
}

enum SecretKey {
    X25519(x25519_dalek::StaticSecret),
    P256(p256::ecdh::EphemeralSecret),
}

pub struct KeyPair {
    secret: SecretKey,
    pub public_jwk: Jwk,
}

impl KeyPair {
    /// Compute the ECDH shared secret Z against the peer's JWK public key.
    pub fn compute_z(&self, peer: &Jwk) -> anyhow::Result<Vec<u8>> {
        match &self.secret {
            SecretKey::X25519(sk) => {
                anyhow::ensure!(peer.kty == "OKP" && peer.crv == "X25519", "peer JWK is not X25519");
                let xb = URL_SAFE_NO_PAD.decode(peer.x.as_bytes())?;
                let arr: [u8; 32] = xb.as_slice().try_into().map_err(|_| anyhow::anyhow!("bad X25519 x len"))?;
                let peer_pub = x25519_dalek::PublicKey::from(arr);
                Ok(sk.diffie_hellman(&peer_pub).as_bytes().to_vec())
            }
            SecretKey::P256(sk) => {
                anyhow::ensure!(peer.kty == "EC" && peer.crv == "P-256", "peer JWK is not P-256");
                let x = URL_SAFE_NO_PAD.decode(peer.x.as_bytes())?;
                let y = URL_SAFE_NO_PAD
                    .decode(peer.y.as_ref().ok_or_else(|| anyhow::anyhow!("P-256 JWK missing y"))?.as_bytes())?;
                let mut uncompressed = Vec::with_capacity(1 + x.len() + y.len());
                uncompressed.push(0x04);
                uncompressed.extend_from_slice(&x);
                uncompressed.extend_from_slice(&y);
                let ep = p256::EncodedPoint::from_bytes(&uncompressed)?;
                let peer_pub = p256::PublicKey::from_encoded_point(&ep)
                    .into_option()
                    .ok_or_else(|| anyhow::anyhow!("invalid P-256 point"))?;
                Ok(sk.diffie_hellman(&peer_pub).raw_secret_bytes().to_vec())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_ids() {
        assert_eq!(Suite::from_id(1), Some(Suite::X25519));
        assert_eq!(Suite::from_id(2), Some(Suite::P256));
        assert_eq!(Suite::X25519.id(), 1);
    }

    #[test]
    fn x25519_ecdh_agrees() {
        let a = Suite::X25519.generate();
        let b = Suite::X25519.generate();
        let za = a.compute_z(&b.public_jwk).unwrap();
        let zb = b.compute_z(&a.public_jwk).unwrap();
        assert_eq!(za, zb);
        assert_eq!(za.len(), 32);
        assert_eq!(a.public_jwk.kty, "OKP");
    }

    #[test]
    fn p256_ecdh_agrees() {
        let a = Suite::P256.generate();
        let b = Suite::P256.generate();
        let za = a.compute_z(&b.public_jwk).unwrap();
        let zb = b.compute_z(&a.public_jwk).unwrap();
        assert_eq!(za, zb);
        assert_eq!(za.len(), 32);
        assert_eq!(a.public_jwk.kty, "EC");
        assert!(a.public_jwk.y.is_some());
    }

    #[test]
    fn mismatched_suites_error() {
        let a = Suite::X25519.generate();
        let b = Suite::P256.generate();
        assert!(a.compute_z(&b.public_jwk).is_err());
    }
}
