//! Reference-compatible on-disk cluster trust directory.
//!
//! Confirmed layout (from the reference workers' `-cluster-dir` flag):
//! ```text
//! <cluster-dir>/
//!   node.crt      # this node's certificate (PEM)
//!   node.key      # this node's private key (PEM)
//!   trusted/      # one PEM per pinned peer certificate
//! ```
//! Using the same layout means an `openpair` node and a reference node can share
//! (or hand off) a cluster directory.

use crate::identity::{pem_cert_to_der, Identity};
use crate::pin::PeerPinStore;
use std::path::Path;

/// Load an identity + pinned peers from a cluster dir, initializing it (minting
/// a fresh identity + empty `trusted/`) if `node.crt`/`node.key` are absent.
pub fn load_or_init(dir: &Path) -> anyhow::Result<(Identity, PeerPinStore)> {
    let crt = dir.join("node.crt");
    let key = dir.join("node.key");
    let trusted = dir.join("trusted");
    std::fs::create_dir_all(&trusted)?;

    let identity = if crt.exists() && key.exists() {
        Identity::from_pem(&std::fs::read_to_string(&crt)?, &std::fs::read_to_string(&key)?)?
    } else {
        let id = Identity::generate()?;
        std::fs::write(&crt, &id.cert_pem)?;
        std::fs::write(&key, &id.key_pem)?;
        id
    };

    let mut pins = PeerPinStore::new();
    load_trusted_into(&trusted, &mut pins)?;
    Ok((identity, pins))
}

/// Pin every PEM certificate found in a `trusted/` directory.
pub fn load_trusted_into(trusted_dir: &Path, pins: &mut PeerPinStore) -> anyhow::Result<usize> {
    let mut n = 0;
    if !trusted_dir.exists() {
        return Ok(0);
    }
    for entry in std::fs::read_dir(trusted_dir)? {
        let path = entry?.path();
        match path.extension().and_then(|e| e.to_str()) {
            Some("crt") | Some("pem") => {}
            _ => continue,
        }
        if let Ok(pem) = std::fs::read_to_string(&path) {
            if let Some(der) = pem_cert_to_der(&pem) {
                if pins.pin(&der).is_ok() {
                    n += 1;
                }
            }
        }
    }
    Ok(n)
}

/// Write a peer certificate (PEM) into `trusted/` under its node UUID, pinning
/// it. Returns the pinned node UUID.
pub fn add_trusted_peer(dir: &Path, peer_cert_pem: &str, pins: &mut PeerPinStore) -> anyhow::Result<String> {
    let der = pem_cert_to_der(peer_cert_pem)
        .ok_or_else(|| anyhow::anyhow!("no CERTIFICATE block in peer PEM"))?;
    let uuid = pins.pin(&der)?;
    let trusted = dir.join("trusted");
    std::fs::create_dir_all(&trusted)?;
    std::fs::write(trusted.join(format!("{uuid}.crt")), peer_cert_pem)?;
    Ok(uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_then_reload_is_stable() {
        let dir = std::env::temp_dir().join(format!("openpair-cd-{}", uuid::Uuid::new_v4()));
        let (id1, pins1) = load_or_init(&dir).unwrap();
        assert!(pins1.is_empty());
        assert!(dir.join("node.crt").exists());
        assert!(dir.join("trusted").exists());
        let (id2, _) = load_or_init(&dir).unwrap();
        assert_eq!(id1.node_uuid, id2.node_uuid);
        assert_eq!(id1.fingerprint(), id2.fingerprint());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trusted_peer_roundtrip() {
        let dir = std::env::temp_dir().join(format!("openpair-cd-{}", uuid::Uuid::new_v4()));
        let (_id, mut pins) = load_or_init(&dir).unwrap();
        let peer = Identity::generate().unwrap();
        let uuid = add_trusted_peer(&dir, &peer.cert_pem, &mut pins).unwrap();
        assert_eq!(uuid, peer.node_uuid);
        assert!(pins.is_pinned(&peer.cert_der));
        // Reload picks the pinned peer back up from trusted/.
        let (_id2, pins2) = load_or_init(&dir).unwrap();
        assert!(pins2.is_pinned(&peer.cert_der));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
