//! Extract the requested model name from an Ollama/OpenAI request body.
//!
//! Confirmed: the proxy buffers the request body and reads the `model` field to
//! pick a routing target (`bufferBodyAndModel`). Both Ollama (`/api/*`) and
//! OpenAI (`/v1/*`) request shapes carry a top-level JSON `"model"` string.

/// Return the `model` field from a JSON request body, if present.
pub fn extract_model(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v.get("model")?.as_str().map(|s| s.to_string())
}

/// Normalize an Ollama model key: strip a trailing `:latest` tag so
/// `llama3` and `llama3:latest` route to the same nodes.
pub fn normalize_model_key(model: &str) -> String {
    model.strip_suffix(":latest").unwrap_or(model).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_from_ollama_and_openai() {
        assert_eq!(
            extract_model(br#"{"model":"llama3","prompt":"hi"}"#).as_deref(),
            Some("llama3")
        );
        assert_eq!(
            extract_model(br#"{"model":"gpt-oss:20b","messages":[]}"#).as_deref(),
            Some("gpt-oss:20b")
        );
        assert_eq!(extract_model(b"not json"), None);
        assert_eq!(extract_model(br#"{"prompt":"no model"}"#), None);
    }

    #[test]
    fn normalize_strips_latest() {
        assert_eq!(normalize_model_key("llama3:latest"), "llama3");
        assert_eq!(normalize_model_key("llama3:8b"), "llama3:8b");
    }
}
