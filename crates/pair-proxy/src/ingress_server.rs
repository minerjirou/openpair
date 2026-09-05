//! Cluster `/ingress` receiver: accept peer requests over mutual TLS and
//! forward them to the local engine.
//!
//! This is the receive side of cross-node routing. A peer that selected this
//! node (via [`crate::router::select`]) POSTs an [`IngressEnvelope`] here over a
//! pinned TLS 1.3 connection; we unwrap `path`/`data`, hit the local engine, and
//! return the response wrapped back into an envelope.

use crate::backend::forward_raw;
use crate::ingress::IngressEnvelope;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use pair_proto::contract::endpoints;
use pair_trust::{Identity, SharedPins};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::warn;

/// Serve mutual-TLS `/ingress` on `bind`, forwarding unwrapped requests to
/// `backend` (host:port), until `shutdown` resolves.
pub async fn serve_ingress(
    bind: SocketAddr,
    identity: &Identity,
    pins: SharedPins,
    backend: String,
    shutdown: impl std::future::Future<Output = ()>,
) -> anyhow::Result<()> {
    let config = pair_trust::server_config(identity, pins)?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(bind).await?;
    let backend = Arc::new(backend);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            accepted = listener.accept() => {
                let (tcp, _) = accepted?;
                let acceptor = acceptor.clone();
                let backend = backend.clone();
                tokio::spawn(async move {
                    let tls = match acceptor.accept(tcp).await {
                        Ok(t) => t,
                        Err(e) => { warn!(error = %e, "ingress TLS handshake failed (unpinned?)"); return; }
                    };
                    let io = TokioIo::new(tls);
                    let svc = service_fn(move |req| {
                        let backend = backend.clone();
                        async move { handle(req, backend).await }
                    });
                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await
                    {
                        warn!(error = %e, "ingress connection error");
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
    if req.uri().path() != endpoints::INGRESS {
        return Ok(Response::builder()
            .status(404)
            .body(Full::new(Bytes::new()))
            .unwrap());
    }
    let body = match req.into_body().collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return Ok(bad_request()),
    };
    let env: IngressEnvelope = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(_) => return Ok(bad_request()),
    };
    let path = env.path.unwrap_or_else(|| "/".to_string());
    let data = env.data.unwrap_or_default();
    let method = env.method.as_deref().unwrap_or("POST");
    let reply = match forward_raw(&backend, method, &path, Bytes::from(data.into_bytes())).await {
        Ok((code, bytes)) => IngressEnvelope {
            code: Some(code),
            data: Some(String::from_utf8_lossy(&bytes).into_owned()),
            txt: Some("ok".into()),
            ..Default::default()
        },
        Err(e) => IngressEnvelope {
            code: Some(502),
            txt: Some(format!("{e}")),
            ..Default::default()
        },
    };
    let out = serde_json::to_vec(&reply).unwrap_or_default();
    Ok(Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(out)))
        .unwrap())
}

fn bad_request() -> Response<Full<Bytes>> {
    Response::builder()
        .status(400)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::peer::forward_to_peer;
    use pair_trust::PeerPinStore;
    use std::sync::RwLock;

    /// Full cross-node inference path: node A -> (mTLS /ingress) node B ->
    /// B's local engine -> response back to A.
    #[tokio::test]
    async fn a_routes_to_b_backend() {
        // Mock local engine for node B: echoes "engine:<path>:<body>".
        let engine = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let engine_addr = engine.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (s, _) = engine.accept().await.unwrap();
                let io = TokioIo::new(s);
                tokio::spawn(async move {
                    let svc = service_fn(|req: Request<Incoming>| async move {
                        let p = req.uri().path().to_string();
                        let b = req.into_body().collect().await.unwrap().to_bytes();
                        let r = format!("engine:{p}:{}", String::from_utf8_lossy(&b));
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(r))))
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await;
                });
            }
        });

        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        let a_pins = Arc::new(RwLock::new(PeerPinStore::new()));
        a_pins.write().unwrap().pin(&b.cert_der).unwrap();
        let b_pins = Arc::new(RwLock::new(PeerPinStore::new()));
        b_pins.write().unwrap().pin(&a.cert_der).unwrap();

        // Node B: ingress receiver -> its local engine.
        let bind = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let b_ingress = bind.local_addr().unwrap();
        drop(bind);
        let b_id = b.clone();
        tokio::spawn(async move {
            serve_ingress(
                b_ingress,
                &b_id,
                b_pins,
                engine_addr.to_string(),
                std::future::pending(),
            )
            .await
            .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // Node A forwards a request to node B.
        let env = IngressEnvelope {
            path: Some("/api/generate".into()),
            data: Some(r#"{"model":"llama3"}"#.into()),
            name: Some("llama3".into()),
            ..Default::default()
        };
        let resp = forward_to_peer(&a, a_pins, "127.0.0.1", b_ingress.port(), &env)
            .await
            .unwrap();
        assert_eq!(resp.code, Some(200));
        assert_eq!(
            resp.data.as_deref(),
            Some("engine:/api/generate:{\"model\":\"llama3\"}")
        );
    }
}
