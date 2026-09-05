//! Cluster pairing transport for openpair (§7.2).
//!
//! This crate carries the EAP-NOOB [`pair_pairing`] state machine onto the wire
//! that a real NVIDIA Personal-AI-Router cluster speaks: the plain-HTTP
//! `/v1/cluster/pairing` channel, the [`PairingInfo`] identity object embedded
//! in ServerInfo/PeerInfo, the six-digit PIN -> 16-byte Noob encoding, and the
//! two-exchange join flow.
//!
//! # Roles
//! * **Joiner** (this node joins a cluster): answers inbound Initial-Exchange
//!   requests, then drives the Completion Exchange after the user enters the PIN
//!   shown by the inviter -- see [`PairingNode::submit_pin`].
//! * **Inviter** (this node grows its cluster): drives the Initial Exchange to a
//!   joiner and serves the joiner-driven Completion -- see
//!   [`PairingNode::create_invite`].
//!
//! On a successful pairing the peer's certificate is authenticated (its embedded
//! [`PairingInfo`] is bound into the Completion MAC, and the cert principal must
//! equal its `nodeUuid`) and handed to a [`TrustSink`] for pinning, establishing
//! the mutual-TLS trust the data plane needs.

pub mod http;
pub mod info;
pub mod node;
pub mod session;
pub mod wire;

pub use http::{post_pairing, serve_pairing};
pub use info::{parse_pairing_info, PairingInfo, PAIRING_INFO_VERSION};
pub use node::{
    default_pairing_addr, NodeProfile, Paired, PairingNode, PendingInvite, Reply, TrustSink,
};
pub use session::{InviterSession, JoinerSession, PeerIdentity, Step};
pub use wire::{
    decode_msg, encode_msg, phase, reason, PairingEnvelope, DEFAULT_PAIRING_PORT, PAIRING_PATH,
};
