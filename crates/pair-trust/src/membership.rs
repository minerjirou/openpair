//! Signed cluster membership: endorsements (a trusted member vouches for an
//! introduced peer's certificate) and tombstones (a member asserts a removal).
//! Both fan out across the mesh so trust/removal propagate beyond a single hop.
//!
//! The exact signed byte layouts and JSON field names are confirmed against the
//! upstream implementation (Apache-2.0): Ed25519 over newline-joined,
//! domain-prefixed ASCII, with the signature base64-encoded.

use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const ENDORSE_DOMAIN_V1: &str = "nvpair-endorse:v1";
pub const ENDORSE_DOMAIN_V2: &str = "nvpair-endorse:v2";
pub const TOMBSTONE_DOMAIN_V1: &str = "nvpair-remove:v1";
pub const TOMBSTONE_DOMAIN_V2: &str = "nvpair-remove:v2";

/// A trusted member's signed vouching for an introduced node's certificate.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Endorsement {
    pub by: String,
    pub fingerprint: String,
    #[serde(rename = "clusterId")]
    pub cluster_id: String,
    #[serde(rename = "admissionEpoch", default, skip_serializing_if = "is_zero")]
    pub admission_epoch: u64,
    #[serde(rename = "byAdmissionEpoch", default, skip_serializing_if = "is_zero")]
    pub by_admission_epoch: u64,
    #[serde(rename = "issuedAt")]
    pub issued_at: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sig: String,
    #[serde(rename = "sigV2", default, skip_serializing_if = "String::is_empty")]
    pub sig_v2: String,
}

/// A trusted member's signed assertion that a node was removed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tombstone {
    #[serde(rename = "nodeUuid")]
    pub node_uuid: String,
    #[serde(rename = "clusterId")]
    pub cluster_id: String,
    #[serde(rename = "admissionEpoch", default, skip_serializing_if = "is_zero")]
    pub admission_epoch: u64,
    pub by: String,
    #[serde(rename = "byAdmissionEpoch", default, skip_serializing_if = "is_zero")]
    pub by_admission_epoch: u64,
    #[serde(rename = "removedAt")]
    pub removed_at: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sig: String,
    #[serde(rename = "sigV2", default, skip_serializing_if = "String::is_empty")]
    pub sig_v2: String,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// v1 endorsement signed bytes: domain \n introducedUUID \n fingerprint \n
/// clusterId \n issuedAt. `introduced_uuid` is bound in even though it is not a
/// struct field, tying the signature to a specific peer entry.
pub fn endorse_payload_v1(
    introduced_uuid: &str,
    fingerprint: &str,
    cluster_id: &str,
    issued_at: i64,
) -> Vec<u8> {
    format!("{ENDORSE_DOMAIN_V1}\n{introduced_uuid}\n{fingerprint}\n{cluster_id}\n{issued_at}")
        .into_bytes()
}

/// v2 endorsement signed bytes (admission-bound).
pub fn endorse_payload_v2(
    introduced_uuid: &str,
    fingerprint: &str,
    cluster_id: &str,
    admission_epoch: u64,
    by_admission_epoch: u64,
    issued_at: i64,
) -> Vec<u8> {
    format!("{ENDORSE_DOMAIN_V2}\n{introduced_uuid}\n{fingerprint}\n{cluster_id}\n{admission_epoch}\n{by_admission_epoch}\n{issued_at}").into_bytes()
}

/// v1 tombstone signed bytes: domain \n nodeUUID \n clusterId \n removedAt.
pub fn tombstone_payload_v1(node_uuid: &str, cluster_id: &str, removed_at: i64) -> Vec<u8> {
    format!("{TOMBSTONE_DOMAIN_V1}\n{node_uuid}\n{cluster_id}\n{removed_at}").into_bytes()
}

/// v2 tombstone signed bytes (admission-bound).
pub fn tombstone_payload_v2(
    node_uuid: &str,
    cluster_id: &str,
    admission_epoch: u64,
    by_admission_epoch: u64,
    removed_at: i64,
) -> Vec<u8> {
    format!("{TOMBSTONE_DOMAIN_V2}\n{node_uuid}\n{cluster_id}\n{admission_epoch}\n{by_admission_epoch}\n{removed_at}").into_bytes()
}

/// Sign a payload with an Ed25519 key; returns a base64 (standard) signature.
pub fn sign(signing: &SigningKey, payload: &[u8]) -> String {
    STANDARD.encode(signing.sign(payload).to_bytes())
}

/// Verify a base64 (standard) Ed25519 signature over `payload`.
pub fn verify(pubkey: &VerifyingKey, payload: &[u8], sig_b64: &str) -> bool {
    let raw = match STANDARD.decode(sig_b64) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let sig = match Signature::from_slice(&raw) {
        Ok(s) => s,
        Err(_) => return false,
    };
    pubkey.verify_strict(payload, &sig).is_ok()
}

/// Extract the Ed25519 public key from a peer certificate DER (its SPKI).
pub fn verifying_key_from_cert(der: &[u8]) -> Option<VerifyingKey> {
    use x509_parser::prelude::*;
    let (_, cert) = X509Certificate::from_der(der).ok()?;
    let spk = cert.public_key().subject_public_key.data.as_ref();
    let arr: [u8; 32] = spk.try_into().ok()?;
    VerifyingKey::from_bytes(&arr).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Identity;

    #[test]
    fn payload_layouts_are_exact() {
        assert_eq!(
            endorse_payload_v2("uu", "sha256:ff", "cl", 3, 7, 1788),
            b"nvpair-endorse:v2\nuu\nsha256:ff\ncl\n3\n7\n1788".to_vec()
        );
        assert_eq!(
            tombstone_payload_v2("nn", "cl", 3, 7, 1788),
            b"nvpair-remove:v2\nnn\ncl\n3\n7\n1788".to_vec()
        );
        assert_eq!(
            endorse_payload_v1("uu", "sha256:ff", "cl", 1788),
            b"nvpair-endorse:v1\nuu\nsha256:ff\ncl\n1788".to_vec()
        );
    }

    #[test]
    fn sign_and_verify_endorsement() {
        let signer = Identity::generate().unwrap();
        let payload = endorse_payload_v2("peer-uuid", "sha256:abcd", "cluster-1", 1, 1, 1788);
        let sig = sign(&signer.signing_key(), &payload);
        // Verify with the signer's public key (recovered two ways).
        assert!(verify(&signer.verifying_key(), &payload, &sig));
        let vk = verifying_key_from_cert(&signer.cert_der).unwrap();
        assert!(verify(&vk, &payload, &sig));
        // Tamper -> reject.
        let bad = endorse_payload_v2("peer-uuid", "sha256:0000", "cluster-1", 1, 1, 1788);
        assert!(!verify(&signer.verifying_key(), &bad, &sig));
    }

    #[test]
    fn cert_pubkey_matches_signing_key() {
        let id = Identity::generate().unwrap();
        let from_cert = verifying_key_from_cert(&id.cert_der).unwrap();
        assert_eq!(from_cert.to_bytes(), id.verifying_key().to_bytes());
    }
}
