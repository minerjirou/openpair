//! Ollama/OpenAI reverse proxy with cluster routing -- clean-room.
//!
//! Data-plane roles (confirmed):
//! * Listen on **loopback** for local apps speaking the Ollama (`/api/*`) and
//!   OpenAI (`/v1/*`) protocols. Local ingress is loopback-only.
//! * Buffer the request, read its `model`, and pick a target node
//!   ([`router::select`]) that advertises the model.
//! * If the target is local, reverse-proxy to the local engine. If it is a
//!   pinned peer, wrap the request in an [`ingress::IngressEnvelope`] and send it
//!   to the peer's `/ingress` over mutual TLS (via `pair-trust`).
//! * Serve `/ingress` for inbound peer requests, forwarding to the local engine.
//!
//! This module currently provides the confirmed, pure routing/contract pieces
//! (model extraction, ingress envelope, candidate selection) with full tests.
//! The hyper listener + mTLS peer client are wired in [`server`] (in progress).

pub mod backend;
pub mod ingress;
pub mod ingress_server;
pub mod live;
pub mod model;
pub mod peer;
pub mod router;
pub mod routing;
pub mod server;
pub mod tags;

pub use ingress::IngressEnvelope;
pub use live::{serve_routing, ProxyContext};
pub use model::{extract_model, normalize_model_key};
pub use router::{select, Candidate};
pub use routing::{PeerEntry, RoutingTable};

/// Where a local application connects and which protocols are accepted.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Loopback bind address for local apps (e.g. 127.0.0.1:11434 for Ollama).
    pub local_bind: std::net::SocketAddr,
    /// The local engine to reverse-proxy to when self is selected.
    pub local_backend: String,
    /// This node's id.
    pub node_id: String,
}
