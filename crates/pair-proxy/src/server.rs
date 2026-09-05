//! Local reverse-proxy HTTP server (hyper 1.x).
//!
//! Serves loopback Ollama/OpenAI traffic and forwards it to a backend. This is
//! the self-routing path (local engine). Peer routing (wrap in
//! [`crate::ingress::IngressEnvelope`], send to a pinned peer's `/ingress` over
//! mTLS) builds on this and on `pair-trust`; it is layered on next.

use crate::model::extract_model;
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
use tracing::{debug, warn};

/// Run the loopback reverse proxy until `shutdown` resolves.
///
/// `backend` is a `host:port` authority for the local engine (e.g.
/// `127.0.0.1:11434`).
pub async fn serve_local(
    bind: SocketAddr,
    backend: String,
    shutdown: impl std::future::Future<Output = ()>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let backend = Arc::new(backend);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                debug!("proxy shutting down");
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, _peer) = accepted?;
                let backend = backend.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let service = service_fn(move |req| {
                        let backend = backend.clone();
                        async move { handle(req, backend).await }
                    });
                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .await
                    {
                        warn!(error = %e, "proxy connection error");
                    }
                });
            }
        }
    }
}

async fn handle(
    req: Request<Incoming>,
    backend: Arc<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    match forward(req, &backend).await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            warn!(error = %e, "proxy forward failed");
            Ok(Response::builder()
                .status(502)
                .body(Full::new(Bytes::from(format!("openpair-proxy: {e}"))))
                .unwrap())
        }
    }
}

async fn forward(req: Request<Incoming>, backend: &str) -> anyhow::Result<Response<Full<Bytes>>> {
    let (parts, body) = req.into_parts();
    let body_bytes = body.collect().await?.to_bytes();

    if let Some(model) = extract_model(&body_bytes) {
        debug!(model = %model, path = %parts.uri.path(), "routing request (self/local)");
    }

    // Open a fresh HTTP/1 connection to the local backend.
    let stream = TcpStream::connect(backend).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        if let Err(e) = conn.await {
            warn!(error = %e, "backend connection closed");
        }
    });

    // Rebuild the request toward the backend, preserving method/path/headers.
    let mut builder = Request::builder().method(parts.method).uri(parts.uri);
    for (k, v) in parts.headers.iter() {
        // Host is set by the backend connection; skip the inbound one.
        if k == hyper::header::HOST {
            continue;
        }
        builder = builder.header(k, v);
    }
    let out_req = builder.body(Full::new(body_bytes))?;

    let resp = sender.send_request(out_req).await?;
    let (rparts, rbody) = resp.into_parts();
    let rbytes = rbody.collect().await?.to_bytes();
    let mut out = Response::builder().status(rparts.status);
    for (k, v) in rparts.headers.iter() {
        out = out.header(k, v);
    }
    Ok(out.body(Full::new(rbytes))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trivial backend that echoes the request body and reports its path.
    async fn spawn_echo_backend() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let io = TokioIo::new(stream);
                tokio::spawn(async move {
                    let svc = service_fn(|req: Request<Incoming>| async move {
                        let path = req.uri().path().to_string();
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        let reply = format!("{path}:{}", String::from_utf8_lossy(&body));
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(reply))))
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn proxies_request_to_backend() {
        let backend = spawn_echo_backend().await;
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = proxy.local_addr().unwrap();
        drop(proxy); // free the port for serve_local to bind

        let (tx, rx) = tokio::sync::oneshot::channel();
        let backend_authority = backend.to_string();
        tokio::spawn(async move {
            serve_local(proxy_addr, backend_authority, async {
                rx.await.ok();
            })
            .await
            .unwrap();
        });
        // Give the server a moment to bind.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Send a request through the proxy.
        let stream = TcpStream::connect(proxy_addr).await.unwrap();
        let io = TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await.unwrap();
        tokio::spawn(async move { conn.await.ok() });
        let req = Request::builder()
            .method("POST")
            .uri("/api/generate")
            .header("host", "localhost")
            .body(Full::new(Bytes::from(r#"{"model":"llama3"}"#)))
            .unwrap();
        let resp = sender.send_request(req).await.unwrap();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            String::from_utf8_lossy(&body),
            r#"/api/generate:{"model":"llama3"}"#
        );

        let _ = tx.send(());
    }
}
