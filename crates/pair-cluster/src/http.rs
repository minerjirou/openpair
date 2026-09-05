//! Plain-HTTP transport for the pairing channel: a one-shot client POST and a
//! hyper server that routes `/v1/cluster/pairing` into [`PairingNode`].
//!
//! The pairing channel is unauthenticated by design -- there is no shared trust
//! until the Completion Exchange establishes it -- so this is plain HTTP/1.1, not
//! mTLS. The EAP-NOOB MACs, not the transport, authenticate the exchange.

use crate::node::PairingNode;
use crate::wire::{PairingEnvelope, PAIRING_PATH};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tracing::warn;

/// POST one pairing envelope to `addr` (a `host:port`) and return the response
/// envelope. A `409` carrying a `rejected` envelope is returned as-is (an
/// explicit refusal, not a transport error).
pub async fn post_pairing(addr: &str, env: &PairingEnvelope) -> anyhow::Result<PairingEnvelope> {
    let body = serde_json::to_vec(env)?;
    let stream = TcpStream::connect(addr).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            warn!(error = %e, "pairing client connection error");
        }
    });
    let req = Request::builder()
        .method("POST")
        .uri(PAIRING_PATH)
        .header("host", addr)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))?;
    let resp = sender.send_request(req).await?;
    let status = resp.status();
    let bytes = resp.into_body().collect().await?.to_bytes();

    if status.as_u16() == 409 {
        if let Ok(e) = serde_json::from_slice::<PairingEnvelope>(&bytes) {
            if e.rejected {
                return Ok(e);
            }
        }
        anyhow::bail!("pairing 409: {}", String::from_utf8_lossy(&bytes));
    }
    if !status.is_success() {
        anyhow::bail!("pairing {}: {}", status.as_u16(), String::from_utf8_lossy(&bytes));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

/// Serve the plain-HTTP pairing channel on `bind` until `shutdown` resolves,
/// routing `/v1/cluster/pairing` into `node`.
pub async fn serve_pairing(
    bind: SocketAddr,
    node: Arc<PairingNode>,
    shutdown: impl std::future::Future<Output = ()>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            accepted = listener.accept() => {
                let (tcp, _peer) = accepted?;
                let node = node.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(tcp);
                    let svc = service_fn(move |req| {
                        let node = node.clone();
                        async move { handle(req, node).await }
                    });
                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await
                    {
                        warn!(error = %e, "pairing server connection error");
                    }
                });
            }
        }
    }
}

async fn handle(
    req: Request<Incoming>,
    node: Arc<PairingNode>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if req.uri().path() != PAIRING_PATH {
        return Ok(text(404, "not found"));
    }
    if req.method() != hyper::Method::POST {
        return Ok(text(405, "method not allowed"));
    }
    let body = match req.into_body().collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return Ok(text(400, "cannot read body")),
    };
    let env: PairingEnvelope = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(_) => return Ok(text(400, "bad request")),
    };
    let reply = node.handle_request(env).await;
    let payload = serde_json::to_vec(&reply.envelope).unwrap_or_default();
    Ok(Response::builder()
        .status(reply.status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(payload)))
        .unwrap())
}

fn text(status: u16, msg: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(Full::new(Bytes::from(msg.to_string())))
        .unwrap()
}
