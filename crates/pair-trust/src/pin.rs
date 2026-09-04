//! Peer certificate pinning: cluster membership is "trust by pinned raw DER".
//!
//! Confirmed: peers are trusted by pinning their exact certificate DER. Before
//! pinning, the reference validates the certificate carries a `urn:nvpair:node`
//! SAN UUID (so only well-formed node certs enter the trust set).

use crate::identity::{cert_fingerprint, node_uuid_from_cert};
use std::collections::HashMap;

/// A set of pinned peer certificates (raw DER), indexed by node UUID.
#[derive(Debug, Default, Clone)]
pub struct PeerPinStore {
    /// node_uuid -> (cert_der, fingerprint)
    by_uuid: HashMap<String, PinnedPeer>,
}

#[derive(Debug, Clone)]
pub struct PinnedPeer {
    pub node_uuid: String,
    pub cert_der: Vec<u8>,
    pub fingerprint: String,
}

impl PeerPinStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pin a peer certificate. Rejects certs lacking the node-UUID SAN.
    pub fn pin(&mut self, cert_der: &[u8]) -> anyhow::Result<String> {
        let node_uuid = node_uuid_from_cert(cert_der)
            .ok_or_else(|| anyhow::anyhow!("refusing to pin: cert has no urn:nvpair:node SAN"))?;
        let fingerprint = cert_fingerprint(cert_der);
        self.by_uuid.insert(
            node_uuid.clone(),
            PinnedPeer { node_uuid: node_uuid.clone(), cert_der: cert_der.to_vec(), fingerprint },
        );
        Ok(node_uuid)
    }

    /// True if this exact certificate DER is pinned.
    pub fn is_pinned(&self, cert_der: &[u8]) -> bool {
        self.by_uuid.values().any(|p| p.cert_der == cert_der)
    }

    pub fn get(&self, node_uuid: &str) -> Option<&PinnedPeer> {
        self.by_uuid.get(node_uuid)
    }

    pub fn remove(&mut self, node_uuid: &str) -> Option<PinnedPeer> {
        self.by_uuid.remove(node_uuid)
    }

    pub fn len(&self) -> usize {
        self.by_uuid.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_uuid.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &PinnedPeer> {
        self.by_uuid.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    #[test]
    fn pin_and_match() {
        let peer = Identity::generate().unwrap();
        let mut store = PeerPinStore::new();
        let uuid = store.pin(&peer.cert_der).unwrap();
        assert_eq!(uuid, peer.node_uuid);
        assert!(store.is_pinned(&peer.cert_der));
        assert_eq!(store.len(), 1);

        let other = Identity::generate().unwrap();
        assert!(!store.is_pinned(&other.cert_der));
    }
}
