//! Candidate selection: given the models a request needs and the known nodes,
//! pick where to run it.
//!
//! Confirmed behaviour: the proxy forwards to a node that (a) is reachable and
//! (b) advertises the requested model, choosing by manual selection, then
//! scheduler priority, then a deterministic default. Requests only ever leave
//! over mTLS to a *pinned* peer.

/// A routing candidate (self or a peer).
#[derive(Debug, Clone)]
pub struct Candidate {
    pub node_id: String,
    /// Base URL of the target's local engine (loopback for self; peer address
    /// reached via `/ingress` for peers).
    pub base_url: String,
    /// Whether this node advertises the requested model.
    pub advertises_model: bool,
    /// Whether this candidate is the local node.
    pub is_self: bool,
    /// Whether this peer's certificate is pinned (required to route to a peer).
    pub pinned: bool,
    /// Scheduler priority rank (lower = preferred); `None` if unranked.
    pub priority_rank: Option<u32>,
    /// Explicit manual selection wins over priority.
    pub manually_selected: bool,
    /// Peer address for cluster routing (host + mTLS `/ingress` port); `None`
    /// for the local node.
    pub host: Option<String>,
    pub ingress_port: Option<u16>,
}

/// Pick the best candidate for a model from the given set, or `None` if nothing
/// eligible. Selection order: manual > lowest priority rank > self > stable by id.
pub fn select<'a>(candidates: &'a [Candidate]) -> Option<&'a Candidate> {
    let eligible: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.advertises_model && (c.is_self || c.pinned))
        .collect();
    if eligible.is_empty() {
        return None;
    }
    // 1. Manual selection.
    if let Some(c) = eligible.iter().find(|c| c.manually_selected) {
        return Some(c);
    }
    // 2. Lowest scheduler priority rank.
    if let Some(c) = eligible
        .iter()
        .filter(|c| c.priority_rank.is_some())
        .min_by_key(|c| c.priority_rank.unwrap())
    {
        return Some(c);
    }
    // 3. Prefer self.
    if let Some(c) = eligible.iter().find(|c| c.is_self) {
        return Some(c);
    }
    // 4. Deterministic default: stable by node id.
    eligible.into_iter().min_by(|a, b| a.node_id.cmp(&b.node_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, model: bool, is_self: bool, pinned: bool, rank: Option<u32>, manual: bool) -> Candidate {
        Candidate {
            node_id: id.into(),
            base_url: format!("http://{id}"),
            advertises_model: model,
            is_self,
            pinned,
            priority_rank: rank,
            manually_selected: manual,
            host: None,
            ingress_port: None,
        }
    }

    #[test]
    fn manual_wins() {
        let cands = vec![
            cand("a", true, false, true, Some(0), false),
            cand("b", true, false, true, Some(9), true),
        ];
        assert_eq!(select(&cands).unwrap().node_id, "b");
    }

    #[test]
    fn priority_then_self() {
        let cands = vec![
            cand("self", true, true, false, None, false),
            cand("peer", true, false, true, Some(1), false),
        ];
        // peer has a rank, self does not -> ranked peer wins.
        assert_eq!(select(&cands).unwrap().node_id, "peer");

        let cands2 = vec![
            cand("self", true, true, false, None, false),
            cand("peer", true, false, true, None, false),
        ];
        // no ranks -> prefer self.
        assert_eq!(select(&cands2).unwrap().node_id, "self");
    }

    #[test]
    fn unpinned_peer_is_ineligible() {
        let cands = vec![cand("peer", true, false, false, Some(0), false)];
        assert!(select(&cands).is_none());
    }

    #[test]
    fn model_must_be_advertised() {
        let cands = vec![cand("self", false, true, false, None, false)];
        assert!(select(&cands).is_none());
    }
}
