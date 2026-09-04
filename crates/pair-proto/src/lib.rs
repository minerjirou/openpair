//! Clean-room wire types for the PAIR (Personal AI Router) cluster protocol.
//!
//! These types are an independent re-implementation of the on-the-wire contract
//! observed from a distributed AI-inference router, derived from static analysis
//! of embedded JSON struct tags and protocol constants. No third-party source
//! code is reused; only the interoperability contract (field names, framing,
//! service identifiers) is reproduced so that an independent node can speak to
//! existing peers.
//!
//! Field-name fidelity matters for interoperability: every `#[serde(rename)]`
//! below reproduces a tag recovered from the reference implementation. Where the
//! exact envelope shape is still being confirmed it is marked `TODO(interop)`.

pub mod jsonrpc;
pub mod telemetry;
pub mod discovery;

pub use jsonrpc::{RpcError, RpcId, RpcMessage, RpcNotification, RpcRequest, RpcResponse};
pub use telemetry::{Cpu, Gpu, GpuVendor, MemoryInfo, NodeInfo};
pub use discovery::{NodeRecord, MDNS_SERVICE_TYPE};
