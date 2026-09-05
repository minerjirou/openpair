//! Minimal HTTP server exposing `GET /v1/node-info` with live GPU/CPU/memory
//! telemetry (NVIDIA + AMD/ROCm), matching the confirmed endpoint path.

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use pair_proto::contract::endpoints;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::warn;

/// Serve `/v1/node-info` until `shutdown` resolves.
pub async fn serve(
    bind: SocketAddr,
    node_id: String,
    shutdown: impl std::future::Future<Output = ()>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let node_id = Arc::new(node_id);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let node_id = node_id.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req| {
                        let node_id = node_id.clone();
                        async move { handle(req, node_id).await }
                    });
                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await
                    {
                        warn!(error = %e, "node-info connection error");
                    }
                });
            }
        }
    }
}

async fn handle(
    req: Request<Incoming>,
    node_id: Arc<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if req.uri().path() != endpoints::NODE_INFO {
        return Ok(Response::builder()
            .status(404)
            .body(Full::new(Bytes::from_static(b"not found")))
            .unwrap());
    }
    // TODO(perf): cache telemetry with a periodic refresh instead of per-request.
    let ni = pair_nodeinfo::collect(Some((*node_id).clone()), None);
    let body = serde_json::to_vec(&ni).unwrap_or_else(|_| b"{}".to_vec());
    Ok(Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)))
        .unwrap())
}
