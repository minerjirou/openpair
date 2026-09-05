//! Model discovery via the Ollama `GET /api/tags` endpoint, for both the local
//! engine and (through mutual-TLS `/ingress`) a peer's engine.

use crate::backend::forward_raw;
use crate::ingress::IngressEnvelope;
use crate::peer::forward_to_peer;
use bytes::Bytes;
use pair_trust::{Identity, SharedPins};

/// Parse an Ollama `/api/tags` response into model names.
/// Shape: `{"models":[{"name":"llama3:latest",...}, ...]}`.
pub fn parse_ollama_tags(body: &[u8]) -> Vec<String> {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v.get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Fetch the local engine's model list.
pub async fn fetch_local_models(backend: &str) -> anyhow::Result<Vec<String>> {
    let (code, body) = forward_raw(backend, "GET", "/api/tags", Bytes::new()).await?;
    if code != 200 {
        anyhow::bail!("local /api/tags returned {code}");
    }
    Ok(parse_ollama_tags(&body))
}

/// Fetch a peer's model list by probing `GET /api/tags` through its mutual-TLS
/// `/ingress`.
pub async fn fetch_peer_models(
    identity: &Identity,
    pins: SharedPins,
    host: &str,
    port: u16,
) -> anyhow::Result<Vec<String>> {
    let env = IngressEnvelope {
        method: Some("GET".into()),
        path: Some("/api/tags".into()),
        ..Default::default()
    };
    let resp = forward_to_peer(identity, pins, host, port, &env).await?;
    let data = resp.data.unwrap_or_default();
    Ok(parse_ollama_tags(data.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags() {
        let body = br#"{"models":[{"name":"llama3:latest","size":1},{"name":"mistral:7b"}]}"#;
        let m = parse_ollama_tags(body);
        assert_eq!(m, vec!["llama3:latest", "mistral:7b"]);
    }

    #[test]
    fn parse_tags_empty_on_garbage() {
        assert!(parse_ollama_tags(b"nope").is_empty());
        assert!(parse_ollama_tags(br#"{"models":[]}"#).is_empty());
    }
}
