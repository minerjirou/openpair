//! Cluster ingress envelope.
//!
//! Confirmed: `/ingress` accepts a wrapped upstream request and proxies it to a
//! peer's local engine. Body fields: {host, port, path, name, data, txt, code}.
//! `data` carries the original request bytes; `code`/`txt` carry status on the
//! response path.

use serde::{Deserialize, Serialize};

/// The `/ingress` request/response envelope.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IngressEnvelope {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Upstream path to hit on the peer's local engine (e.g. `/api/generate`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// HTTP method for the wrapped request (openpair extension; defaults to POST
    /// on the receive side). The reference envelope omits this — for reference
    /// interop the method is a [live] item; this field only affects
    /// openpair<->openpair routing (e.g. probing a peer's `GET /api/tags`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Model / target name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Original request (or response) body bytes, base64-agnostic string form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Status text on the response path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub txt: Option<String>,
    /// HTTP status code on the response path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingress_roundtrip_named_fields() {
        let e = IngressEnvelope {
            host: Some("10.0.0.5".into()),
            port: Some(11434),
            path: Some("/api/generate".into()),
            name: Some("llama3".into()),
            data: Some("{\"model\":\"llama3\"}".into()),
            ..Default::default()
        };
        let s = serde_json::to_string(&e).unwrap();
        for k in ["host", "port", "path", "name", "data"] {
            assert!(s.contains(k), "missing {k}");
        }
        let back: IngressEnvelope = serde_json::from_str(&s).unwrap();
        assert_eq!(back.port, Some(11434));
        assert_eq!(back.path.as_deref(), Some("/api/generate"));
    }
}
