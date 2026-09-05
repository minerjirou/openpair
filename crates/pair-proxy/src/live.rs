//! Live routing proxy: consult the [`RoutingTable`] per request and route to the
//! local engine or a pinned peer's mutual-TLS `/ingress`.

use crate::backend::forward_raw;
use crate::ingress::IngressEnvelope;
use crate::model::extract_model;
use crate::peer::forward_to_peer;
use crate::router::select;
use crate::routing::RoutingTable;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use pair_trust::{Identity, SharedPins};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use tracing::{debug, warn};

/// Everything the live proxy needs to route a request.
#[derive(Clone)]
pub struct ProxyContext {
    /// Local engine authority (host:port).
    pub backend: String,
    /// This node's identity (for mTLS client auth to peers).
    pub identity: Arc<Identity>,
    /// Pinned peer certificates (for mTLS to peers).
    pub pins: SharedPins,
    /// Shared routing table (self + peer models).
    pub routing: Arc<RwLock<RoutingTable>>,
}

/// Serve the loopback proxy with cluster-aware routing until `shutdown`.
pub async fn serve_routing(
    bind: SocketAddr,
    ctx: ProxyContext,
    shutdown: impl std::future::Future<Output = ()>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let ctx = Arc::new(ctx);
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let ctx = ctx.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req| {
                        let ctx = ctx.clone();
                        async move { route(req, ctx).await }
                    });
                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await
                    {
                        warn!(error = %e, "live proxy connection error");
                    }
                });
            }
        }
    }
}

async fn route(
    req: Request<Incoming>,
    ctx: Arc<ProxyContext>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    match route_inner(req, &ctx).await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            warn!(error = %e, "routing failed");
            Ok(Response::builder()
                .status(502)
                .body(Full::new(Bytes::from(format!("openpair-proxy: {e}"))))
                .unwrap())
        }
    }
}

async fn route_inner(
    req: Request<Incoming>,
    ctx: &ProxyContext,
) -> anyhow::Result<Response<Full<Bytes>>> {
    let (parts, body) = req.into_parts();
    let body_bytes = body.collect().await?.to_bytes();
    let path = parts.uri.path().to_string();
    let model = extract_model(&body_bytes);

    // Decide the target under a short read lock (no await while locked).
    let target = {
        let table = ctx.routing.read().expect("routing table poisoned");
        let candidates = table.candidates_for(model.as_deref(), &ctx.backend);
        select(&candidates).cloned()
    };

    match target {
        Some(c) if c.is_self => {
            debug!(model = ?model, "routing to local engine");
            let (code, bytes) =
                forward_raw(&ctx.backend, parts.method.as_str(), &path, body_bytes).await?;
            build_response(code, bytes)
        }
        Some(c) => {
            let host = c.host.clone().unwrap_or_default();
            let port = c.ingress_port.unwrap_or(0);
            debug!(peer = %c.node_id, host = %host, port, model = ?model, "routing to peer");
            let env = IngressEnvelope {
                path: Some(path),
                data: Some(String::from_utf8_lossy(&body_bytes).into_owned()),
                name: model,
                ..Default::default()
            };
            let resp = forward_to_peer(&ctx.identity, ctx.pins.clone(), &host, port, &env).await?;
            let code = resp.code.unwrap_or(502);
            let data = resp.data.unwrap_or_default();
            build_response(code, Bytes::from(data.into_bytes()))
        }
        None => Ok(Response::builder()
            .status(503)
            .body(Full::new(Bytes::from_static(
                b"no node can serve this model",
            )))
            .unwrap()),
    }
}

fn build_response(code: u16, body: Bytes) -> anyhow::Result<Response<Full<Bytes>>> {
    Ok(Response::builder().status(code).body(Full::new(body))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingress_server::serve_ingress;
    use crate::routing::PeerEntry;
    use pair_trust::PeerPinStore;

    /// Full live routing: a request for a model only peer B has is routed from
    /// A's loopback proxy, over mTLS /ingress, to B, to B's engine, and back.
    #[tokio::test]
    async fn live_routes_to_peer_over_mtls() {
        // B's mock engine.
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
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(format!(
                            "B-engine {p} {}",
                            String::from_utf8_lossy(&b)
                        )))))
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await;
                });
            }
        });

        let a = Identity::generate().unwrap();
        let b = Identity::generate().unwrap();
        let a_pins: SharedPins = Arc::new(RwLock::new(PeerPinStore::new()));
        a_pins.write().unwrap().pin(&b.cert_der).unwrap();
        let b_pins: SharedPins = Arc::new(RwLock::new(PeerPinStore::new()));
        b_pins.write().unwrap().pin(&a.cert_der).unwrap();

        // B: mTLS /ingress -> B engine.
        let bl = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let b_ingress = bl.local_addr().unwrap();
        drop(bl);
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

        // A: routing table knows only B serves "llama3"; A serves nothing.
        let mut table = RoutingTable::new(a.node_uuid.clone());
        table.set_local_models(Vec::<String>::new());
        table.upsert_peer(PeerEntry {
            node_id: b.node_uuid.clone(),
            host: "127.0.0.1".into(),
            ingress_port: b_ingress.port(),
            models: ["llama3".to_string()].into_iter().collect(),
            pinned: true,
            priority_rank: Some(0),
            manually_selected: false,
        });
        let ctx = ProxyContext {
            backend: "127.0.0.1:1".into(), // unused; A has no local model
            identity: Arc::new(a),
            pins: a_pins,
            routing: Arc::new(RwLock::new(table)),
        };

        let al = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a_proxy = al.local_addr().unwrap();
        drop(al);
        tokio::spawn(async move {
            serve_routing(a_proxy, ctx, std::future::pending())
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Client hits A's proxy; A must route to B.
        let stream = tokio::net::TcpStream::connect(a_proxy).await.unwrap();
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
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            String::from_utf8_lossy(&body),
            r#"B-engine /api/generate {"model":"llama3"}"#
        );
    }
}
