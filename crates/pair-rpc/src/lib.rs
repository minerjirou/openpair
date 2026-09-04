//! Async JSON-RPC 2.0 transport with pluggable stdio framing.
//!
//! The supervisor talks to each service over the child's stdio using JSON-RPC
//! 2.0. The exact framing (how one JSON object is delimited on the byte stream)
//! is being confirmed by protocol analysis, so it is abstracted behind
//! [`Framing`]: switching the confirmed value is a one-line change and every
//! consumer keeps working.

pub mod framing;

pub use framing::Framing;

use pair_proto::RpcMessage;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

/// Encode and write one JSON-RPC message using the given framing.
pub async fn write_message<W: AsyncWrite + Unpin>(
    w: &mut W,
    framing: Framing,
    msg: &RpcMessage,
) -> anyhow::Result<()> {
    let body = serde_json::to_vec(msg)?;
    let framed = framing.encode(&body);
    w.write_all(&framed).await?;
    w.flush().await?;
    Ok(())
}

/// Read and decode the next JSON-RPC message, or `Ok(None)` at clean EOF.
pub async fn read_message<R: AsyncRead + Unpin>(
    r: &mut R,
    framing: Framing,
) -> anyhow::Result<Option<RpcMessage>> {
    match framing.read_frame(r).await? {
        Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pair_proto::{RpcId, RpcNotification, RpcRequest};

    async fn roundtrip(framing: Framing) {
        let (mut a, mut b) = tokio::io::duplex(4096);
        let req = RpcMessage::Request(RpcRequest::new(
            "engines.start",
            Some(serde_json::json!({"engine":"ollama"})),
            RpcId::Num(1),
        ));
        let note = RpcMessage::Notification(RpcNotification::new("telemetry.update", None));

        write_message(&mut a, framing, &req).await.unwrap();
        write_message(&mut a, framing, &note).await.unwrap();
        drop(a);

        let got1 = read_message(&mut b, framing).await.unwrap().unwrap();
        let got2 = read_message(&mut b, framing).await.unwrap().unwrap();
        let eof = read_message(&mut b, framing).await.unwrap();

        assert!(matches!(got1, RpcMessage::Request(_)));
        assert!(matches!(got2, RpcMessage::Notification(_)));
        assert!(eof.is_none());
    }

    #[tokio::test]
    async fn jsonlines_roundtrip() {
        roundtrip(Framing::JsonLines).await;
    }

    #[tokio::test]
    async fn length_prefixed_roundtrip() {
        roundtrip(Framing::LengthPrefixedBE).await;
    }

    #[tokio::test]
    async fn content_length_roundtrip() {
        roundtrip(Framing::ContentLength).await;
    }
}
