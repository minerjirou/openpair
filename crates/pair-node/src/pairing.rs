//! Cluster pairing wiring for the daemon (§7.2): builds the [`PairingNode`],
//! establishes trust on a successful pairing, and provides the operator-driven
//! `invite` / `join` flows.
//!
//! Trust: a freshly-paired peer's certificate is pinned into the live
//! [`SharedPins`] (so mutual-TLS accepts it immediately) and, when a cluster
//! directory is configured, written to `trusted/` so trust survives a restart.
//! Once paired the node considers itself clustered and refuses further inbound
//! joins, matching the reference's single-cluster invariant.

use pair_cluster::{NodeProfile, Paired, PairingNode, TrustSink};
use pair_trust::{Identity, SharedPins};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Establishes trust for freshly-paired peers and tracks cluster membership.
pub struct ClusterTrustSink {
    pins: SharedPins,
    cluster_dir: Option<PathBuf>,
    clustered: AtomicBool,
    cluster_id: Mutex<String>,
}

impl ClusterTrustSink {
    pub fn new(pins: SharedPins, cluster_dir: Option<PathBuf>, already_clustered: bool) -> Arc<Self> {
        Arc::new(Self {
            pins,
            cluster_dir,
            clustered: AtomicBool::new(already_clustered),
            cluster_id: Mutex::new(String::new()),
        })
    }

}

impl TrustSink for ClusterTrustSink {
    fn pin_peer(&self, paired: &Paired) {
        let uuid = &paired.peer.node_uuid;
        // Persist into trusted/ (which also pins) when a cluster dir is set;
        // otherwise pin in-memory only.
        let result = {
            let mut store = self.pins.write().expect("pin store poisoned");
            match &self.cluster_dir {
                Some(dir) => {
                    pair_trust::cluster_dir::add_trusted_peer(dir, &paired.peer.cert, &mut store)
                        .map(|_| ())
                }
                None => store.pin(&paired.peer_cert_der).map(|_| ()),
            }
        };
        match result {
            Ok(()) => {
                if !paired.peer.cluster_id.is_empty() {
                    *self.cluster_id.lock().unwrap() = paired.peer.cluster_id.clone();
                }
                self.clustered.store(true, Ordering::SeqCst);
                info!(
                    peer = %uuid,
                    name = %paired.peer.name,
                    cluster = %paired.peer.cluster_id,
                    "paired: peer certificate pinned; mutual-TLS trust established"
                );
            }
            Err(e) => warn!(peer = %uuid, error = %e, "failed to pin paired peer"),
        }
    }

    fn is_clustered(&self) -> bool {
        self.clustered.load(Ordering::SeqCst)
    }
}

/// Build this node's [`NodeProfile`] for pairing. `advertised_addr` is the
/// reachable `host:port` the node serves the pairing channel on.
pub fn node_profile(
    identity: Arc<Identity>,
    name: String,
    advertised_addr: String,
    cluster_id: String,
    cluster_friendly_name: String,
) -> NodeProfile {
    NodeProfile {
        identity,
        name,
        cluster_id,
        cluster_friendly_name,
        admission_epoch: 1,
        advertised_addr,
    }
}

/// Assemble a [`PairingNode`] from a profile and trust sink.
pub fn build_node(profile: NodeProfile, sink: Arc<ClusterTrustSink>) -> Arc<PairingNode> {
    PairingNode::new(profile, sink)
}
