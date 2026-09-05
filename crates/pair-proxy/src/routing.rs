//! Shared routing table: what this node and its peers can serve, used by the
//! live proxy to pick a target per request.
//!
//! Populated by discovery + node/model probing (peers) and a local `/api/tags`
//! poll (self). The proxy consults it per request via [`RoutingTable::candidates_for`].

use crate::router::Candidate;
use std::collections::{HashMap, HashSet};

/// A known peer and what it can serve.
#[derive(Debug, Clone)]
pub struct PeerEntry {
    pub node_id: String,
    pub host: String,
    /// Peer's mutual-TLS `/ingress` port.
    pub ingress_port: u16,
    pub models: HashSet<String>,
    /// True once the peer's certificate is pinned (required to route to it).
    pub pinned: bool,
    pub priority_rank: Option<u32>,
    /// Manually selected target (overrides priority).
    pub manually_selected: bool,
}

#[derive(Debug, Default)]
pub struct RoutingTable {
    /// Models the local engine can serve.
    local_models: HashSet<String>,
    peers: HashMap<String, PeerEntry>,
    /// Local node id (for candidate labelling).
    local_id: String,
}

impl RoutingTable {
    pub fn new(local_id: impl Into<String>) -> Self {
        Self { local_id: local_id.into(), ..Default::default() }
    }

    pub fn set_local_models<I: IntoIterator<Item = String>>(&mut self, models: I) {
        self.local_models = models.into_iter().collect();
    }

    pub fn upsert_peer(&mut self, peer: PeerEntry) {
        self.peers.insert(peer.node_id.clone(), peer);
    }

    pub fn remove_peer(&mut self, node_id: &str) {
        self.peers.remove(node_id);
    }

    /// Insert or update a peer's address/pin metadata without disturbing the
    /// model set already learned for it.
    pub fn upsert_peer_meta(&mut self, node_id: &str, host: String, ingress_port: u16, pinned: bool) {
        self.peers
            .entry(node_id.to_string())
            .and_modify(|p| {
                p.host = host.clone();
                p.ingress_port = ingress_port;
                p.pinned = pinned;
            })
            .or_insert_with(|| PeerEntry {
                node_id: node_id.to_string(),
                host,
                ingress_port,
                models: HashSet::new(),
                pinned,
                priority_rank: None,
                manually_selected: false,
            });
    }

    /// Replace the model set known for a peer.
    pub fn set_peer_models<I: IntoIterator<Item = String>>(&mut self, node_id: &str, models: I) {
        if let Some(p) = self.peers.get_mut(node_id) {
            p.models = models.into_iter().collect();
        }
    }

    /// (node_id, host, ingress_port) for every currently-pinned peer.
    pub fn pinned_peers(&self) -> Vec<(String, String, u16)> {
        self.peers
            .values()
            .filter(|p| p.pinned)
            .map(|p| (p.node_id.clone(), p.host.clone(), p.ingress_port))
            .collect()
    }

    pub fn set_peer_priority(&mut self, node_id: &str, rank: Option<u32>) {
        if let Some(p) = self.peers.get_mut(node_id) {
            p.priority_rank = rank;
        }
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Build the candidate set for a request. `model = None` means "no model in
    /// the body" — only the local node is a candidate (we don't blind-route).
    pub fn candidates_for(&self, model: Option<&str>, backend: &str) -> Vec<Candidate> {
        let mut out = Vec::new();

        // Local node: eligible if it can serve the model (or none was specified).
        let self_has = match model {
            Some(m) => self.local_models.contains(m),
            None => true,
        };
        out.push(Candidate {
            node_id: self.local_id.clone(),
            base_url: format!("http://{backend}"),
            advertises_model: self_has,
            is_self: true,
            pinned: false,
            priority_rank: None,
            manually_selected: false,
            host: None,
            ingress_port: None,
        });

        // Peers: eligible if pinned and advertising the model.
        if let Some(m) = model {
            for p in self.peers.values() {
                out.push(Candidate {
                    node_id: p.node_id.clone(),
                    base_url: format!("https://{}:{}", p.host, p.ingress_port),
                    advertises_model: p.models.contains(m),
                    is_self: false,
                    pinned: p.pinned,
                    priority_rank: p.priority_rank,
                    manually_selected: p.manually_selected,
                    host: Some(p.host.clone()),
                    ingress_port: Some(p.ingress_port),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::select;

    fn peer(id: &str, models: &[&str], pinned: bool, rank: Option<u32>) -> PeerEntry {
        PeerEntry {
            node_id: id.into(),
            host: format!("10.0.0.{}", id.len()),
            ingress_port: 7443,
            models: models.iter().map(|s| s.to_string()).collect(),
            pinned,
            priority_rank: rank,
            manually_selected: false,
        }
    }

    #[test]
    fn routes_to_peer_when_self_lacks_model() {
        let mut t = RoutingTable::new("self");
        t.set_local_models(["mistral".to_string()]);
        t.upsert_peer(peer("b", &["llama3"], true, Some(0)));

        let cands = t.candidates_for(Some("llama3"), "127.0.0.1:11434");
        let chosen = select(&cands).unwrap();
        assert_eq!(chosen.node_id, "b");
        assert_eq!(chosen.ingress_port, Some(7443));
        assert!(!chosen.is_self);
    }

    #[test]
    fn prefers_self_when_it_has_the_model() {
        let mut t = RoutingTable::new("self");
        t.set_local_models(["llama3".to_string()]);
        t.upsert_peer(peer("b", &["llama3"], true, None));
        let cands = t.candidates_for(Some("llama3"), "127.0.0.1:11434");
        assert!(select(&cands).unwrap().is_self);
    }

    #[test]
    fn unpinned_peer_ignored() {
        let mut t = RoutingTable::new("self");
        t.set_local_models([]); // self has nothing
        t.upsert_peer(peer("b", &["llama3"], false, Some(0))); // not pinned
        let cands = t.candidates_for(Some("llama3"), "127.0.0.1:11434");
        assert!(select(&cands).is_none());
    }
}
