//! Cluster identity, certificate pinning, and (next) mutual-TLS trust.
//!
//! Clean-room implementation of the cluster trust model:
//! * Ed25519 self-signed node certificate carrying `urn:nvpair:node:<uuid>`
//! * trust by pinned raw DER (validated to carry the node-UUID SAN)
//! * cluster UUID is a random-minted identifier (not derived from the cert)
//! * transport: TLS 1.3, mutual auth, peers accepted only if their DER is pinned
//!   (mTLS config builders land in a follow-up once the rustls verifier is wired)

pub mod cluster_dir;
pub mod identity;
pub mod mtls;
pub mod pin;
pub mod uuid_scheme;

pub use identity::{cert_fingerprint, node_uuid_from_cert, Identity};
pub use mtls::{client_config, server_config, SharedPins};
pub use cluster_dir::load_or_init as load_cluster_dir;
pub use pin::{PeerPinStore, PinnedPeer};
pub use uuid_scheme::{node_urn, uuid_from_urn, NODE_URN_PREFIX};

/// Mint a fresh random cluster UUID. Confirmed: the cluster identifier is a
/// freshly minted random UUID, independent of any certificate.
pub fn mint_cluster_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
