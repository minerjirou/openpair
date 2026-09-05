//! Cluster node identity: a self-signed **Ed25519** X.509 leaf.
//!
//! Confirmed profile (from static analysis of the reference `generateLeaf`):
//! * key: Ed25519 (PureEd25519 signature)
//! * serial: 128-bit random
//! * extended key usage: serverAuth + clientAuth
//! * SAN: URI `urn:nvpair:node:<uuid>` — the node UUID lives here
//! * fingerprint format: `sha256:<hex-of-cert-DER>`
//!
//! TODO(interop, needs dynamic confirmation): exact Subject (O/OU/CN) fields and
//! the validity window. A wide validity is used until confirmed; the SAN URI,
//! key type, EKU and serial width are byte-confirmed.

use crate::uuid_scheme::{node_urn, uuid_from_urn};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::path::Path;

/// URN scheme prefix carrying the node UUID in the certificate SAN.
pub use crate::uuid_scheme::NODE_URN_PREFIX;

#[derive(Clone)]
pub struct Identity {
    pub node_uuid: String,
    pub cert_der: Vec<u8>,
    pub key_pkcs8_der: Vec<u8>,
    pub cert_pem: String,
    pub key_pem: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("node_uuid", &self.node_uuid)
            .field("fingerprint", &self.fingerprint())
            .finish()
    }
}

impl Identity {
    /// Mint a fresh identity with a random node UUID.
    pub fn generate() -> anyhow::Result<Self> {
        Self::generate_with_uuid(&uuid::Uuid::new_v4().to_string())
    }

    /// Mint an identity for a specific node UUID.
    pub fn generate_with_uuid(node_uuid: &str) -> anyhow::Result<Self> {
        let key_pair = rcgen::KeyPair::generate_for(&rcgen::PKCS_ED25519)?;

        let mut params = rcgen::CertificateParams::new(Vec::<String>::new())?;
        // SAN: URI urn:nvpair:node:<uuid>
        params
            .subject_alt_names
            .push(rcgen::SanType::URI(rcgen::Ia5String::try_from(node_urn(node_uuid))?));
        // EKU: server + client auth (mutual TLS both directions).
        params.extended_key_usages = vec![
            rcgen::ExtendedKeyUsagePurpose::ServerAuth,
            rcgen::ExtendedKeyUsagePurpose::ClientAuth,
        ];
        // 128-bit random serial.
        let mut serial = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut serial);
        params.serial_number = Some(rcgen::SerialNumber::from_slice(&serial));
        // Minimal subject; a CN is set to the node UUID (Subject fields pending
        // dynamic confirmation).
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, node_uuid);
        // Wide validity window (exact window pending confirmation).
        params.not_before = rcgen::date_time_ymd(2025, 1, 1);
        params.not_after = rcgen::date_time_ymd(2035, 1, 1);

        let cert = params.self_signed(&key_pair)?;
        let cert_der = cert.der().as_ref().to_vec();
        let cert_pem = cert.pem();
        let key_pkcs8_der = key_pair.serialize_der();
        let key_pem = key_pair.serialize_pem();

        Ok(Self { node_uuid: node_uuid.to_string(), cert_der, key_pkcs8_der, cert_pem, key_pem })
    }

    /// `sha256:<hex>` fingerprint over the certificate DER.
    pub fn fingerprint(&self) -> String {
        cert_fingerprint(&self.cert_der)
    }

    /// Persist identity to `<dir>/node-key.pem` and `<dir>/node-cert.pem`.
    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("node-key.pem"), &self.key_pem)?;
        std::fs::write(dir.join("node-cert.pem"), &self.cert_pem)?;
        Ok(())
    }

    /// Load a previously saved identity, or mint + save a new one if absent.
    pub fn load_or_generate(dir: &Path) -> anyhow::Result<Self> {
        let key_path = dir.join("node-key.pem");
        let cert_path = dir.join("node-cert.pem");
        if key_path.exists() && cert_path.exists() {
            let key_pem = std::fs::read_to_string(&key_path)?;
            let cert_pem = std::fs::read_to_string(&cert_path)?;
            let key_pair = rcgen::KeyPair::from_pem(&key_pem)?;
            let cert_der = pem_to_der(&cert_pem)
                .ok_or_else(|| anyhow::anyhow!("no CERTIFICATE block in {cert_path:?}"))?;
            let node_uuid = node_uuid_from_cert(&cert_der)
                .ok_or_else(|| anyhow::anyhow!("cert missing urn:nvpair:node SAN"))?;
            Ok(Self {
                node_uuid,
                cert_der,
                key_pkcs8_der: key_pair.serialize_der(),
                cert_pem,
                key_pem,
            })
        } else {
            let id = Self::generate()?;
            id.save(dir)?;
            Ok(id)
        }
    }
}

/// `sha256:<hex>` over arbitrary certificate DER bytes.
pub fn cert_fingerprint(der: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(der);
    format!("sha256:{}", hex::encode(h.finalize()))
}

/// Extract the node UUID from a certificate's `urn:nvpair:node:<uuid>` SAN URI.
pub fn node_uuid_from_cert(der: &[u8]) -> Option<String> {
    use x509_parser::prelude::*;
    let (_, cert) = X509Certificate::from_der(der).ok()?;
    let san = cert.subject_alternative_name().ok()??;
    for name in &san.value.general_names {
        if let GeneralName::URI(uri) = name {
            if let Some(uuid) = uuid_from_urn(uri) {
                return Some(uuid);
            }
        }
    }
    None
}

impl Identity {
    /// Reconstruct an identity from cert + key PEM (e.g. a cluster dir's
    /// `node.crt` / `node.key`).
    pub fn from_pem(cert_pem: &str, key_pem: &str) -> anyhow::Result<Self> {
        let key_pair = rcgen::KeyPair::from_pem(key_pem)?;
        let cert_der =
            pem_cert_to_der(cert_pem).ok_or_else(|| anyhow::anyhow!("no CERTIFICATE block"))?;
        let node_uuid = node_uuid_from_cert(&cert_der)
            .ok_or_else(|| anyhow::anyhow!("cert missing urn:nvpair:node SAN"))?;
        Ok(Self {
            node_uuid,
            cert_der,
            key_pkcs8_der: key_pair.serialize_der(),
            cert_pem: cert_pem.to_string(),
            key_pem: key_pem.to_string(),
        })
    }
}

/// Extract DER from the first CERTIFICATE PEM block.
pub fn pem_cert_to_der(pem: &str) -> Option<Vec<u8>> {
    pem_to_der(pem)
}

fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
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

/// Minimal standard base64 decoder (avoids an extra dependency for one call).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, &c) in TABLE.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_has_uuid_san_and_fingerprint() {
        let id = Identity::generate_with_uuid("11112222-3333-4444-5555-666677778888").unwrap();
        assert_eq!(id.node_uuid, "11112222-3333-4444-5555-666677778888");
        assert!(id.fingerprint().starts_with("sha256:"));
        // The UUID must be recoverable from the cert SAN.
        let recovered = node_uuid_from_cert(&id.cert_der).unwrap();
        assert_eq!(recovered, id.node_uuid);
    }

    #[test]
    fn fingerprint_is_stable() {
        let id = Identity::generate().unwrap();
        assert_eq!(id.fingerprint(), cert_fingerprint(&id.cert_der));
    }

    #[test]
    fn save_and_reload_roundtrips_uuid() {
        let dir = std::env::temp_dir().join(format!("openpair-id-{}", uuid::Uuid::new_v4()));
        let id = Identity::load_or_generate(&dir).unwrap();
        let again = Identity::load_or_generate(&dir).unwrap();
        assert_eq!(id.node_uuid, again.node_uuid);
        assert_eq!(id.fingerprint(), again.fingerprint());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
