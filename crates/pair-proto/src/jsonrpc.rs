//! JSON-RPC 2.0 envelope used for supervisor <-> service IPC (over stdio) and
//! for several HTTP control surfaces.
//!
//! Confirmed from embedded struct tags: `jsonrpc`, `method`, `params`, `result`,
//! `id`, `error` { `code`, `message` }. The stdio framing (newline-delimited vs.
//! length-prefixed) is being confirmed by the protocol-analysis pass; the JSON
//! bodies themselves are exactly these shapes regardless of framing.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC id: a string or an integer (never fractional in practice).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    Num(i64),
    Str(String),
}

/// A request expecting a response (`id` present).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: JsonRpcVersion,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    pub id: RpcId,
}

/// A notification (no `id`, no response expected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: JsonRpcVersion,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A response to a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: JsonRpcVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: RpcId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Any frame that can appear on the wire, decoded by shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcMessage {
    Request(RpcRequest),
    Response(RpcResponse),
    Notification(RpcNotification),
}

/// Serializes/deserializes only the literal string `"2.0"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JsonRpcVersion;

impl Serialize for JsonRpcVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("2.0")
    }
}

impl<'de> Deserialize<'de> for JsonRpcVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = String::deserialize(d)?;
        if v == "2.0" {
            Ok(JsonRpcVersion)
        } else {
            Err(serde::de::Error::custom(format!("unsupported jsonrpc version: {v}")))
        }
    }
}

impl RpcRequest {
    pub fn new(method: impl Into<String>, params: Option<Value>, id: RpcId) -> Self {
        Self { jsonrpc: JsonRpcVersion, method: method.into(), params, id }
    }
}

impl RpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self { jsonrpc: JsonRpcVersion, method: method.into(), params }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip() {
        let req = RpcRequest::new("nodes.upsert", Some(serde_json::json!({"uuid":"x"})), RpcId::Num(7));
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("\"jsonrpc\":\"2.0\""));
        assert!(s.contains("\"method\":\"nodes.upsert\""));
        let back: RpcMessage = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, RpcMessage::Request(_)));
    }

    #[test]
    fn notification_has_no_id() {
        let n = RpcNotification::new("telemetry.update", None);
        let s = serde_json::to_string(&n).unwrap();
        assert!(!s.contains("\"id\""));
    }

    #[test]
    fn response_decodes_as_message() {
        let s = r#"{"jsonrpc":"2.0","result":{"ok":true},"id":1}"#;
        let m: RpcMessage = serde_json::from_str(s).unwrap();
        assert!(matches!(m, RpcMessage::Response(_)));
    }
}
