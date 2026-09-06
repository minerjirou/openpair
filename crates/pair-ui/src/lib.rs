//! openpair node web dashboard + control API.
//!
//! Serves a small single-page dashboard (node status, hardware, discovered
//! peers, routing) and a control API that can **pair two openpair nodes** by
//! exchanging certificates over plain HTTP (establishing mutual-TLS trust).
//! Bind it to loopback; it is an operator console, not a public surface.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use pair_proxy::RoutingTable;
use pair_trust::{Identity, SharedPins};
use serde_json::json;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tokio::net::TcpStream;
use tracing::warn;

const INDEX_HTML: &str = include_str!("index.html");

/// Everything the dashboard needs to render + act.
pub struct UiContext {
    pub node_id: String,
    pub identity: Arc<Identity>,
    pub pins: SharedPins,
    pub routing: Arc<RwLock<RoutingTable>>,
    /// This node's mutual-TLS `/ingress` port (advertised to peers on pairing).
    pub ingress_port: u16,
    /// The EAP-NOOB cluster-pairing endpoint (see [`pair_cluster`]).
    pub pairing: Arc<pair_cluster::PairingNode>,
}

/// Serve the dashboard + control API until `shutdown` resolves.
pub async fn serve(
    bind: SocketAddr,
    ctx: Arc<UiContext>,
    shutdown: impl std::future::Future<Output = ()>,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(bind).await?;
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
                        warn!(error = %e, "ui connection error");
                    }
                });
            }
        }
    }
}

async fn route(
    req: Request<Incoming>,
    ctx: Arc<UiContext>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let resp = match (method.as_str(), path.as_str()) {
        ("GET", "/") => html(INDEX_HTML),
        ("GET", "/api/status") => json_ok(status(&ctx)),
        ("GET", "/api/identity/cert") => text(ctx.identity.cert_pem.clone()),
        ("POST", "/api/trust/add") => {
            let body = read_body(req).await;
            add_cert(&ctx, &String::from_utf8_lossy(&body))
        }
        ("POST", "/api/trust/pair") => {
            let body = read_body(req).await;
            match pair(&ctx, &body).await {
                Ok(v) => json_ok(v),
                Err(e) => json_ok(json!({"ok": false, "error": e.to_string()})),
            }
        }
        // --- EAP-NOOB cluster pairing (real cluster join) ---
        ("GET", "/api/pairing/pending") => json_ok(pending_invites(&ctx).await),
        ("POST", "/api/pairing/invite") => {
            let body = read_body(req).await;
            match invite(&ctx, &body).await {
                Ok(v) => json_ok(v),
                Err(e) => json_ok(json!({"ok": false, "error": e.to_string()})),
            }
        }
        ("POST", "/api/pairing/respond") => {
            let body = read_body(req).await;
            match respond(&ctx, &body).await {
                Ok(v) => json_ok(v),
                Err(e) => json_ok(json!({"ok": false, "error": e.to_string()})),
            }
        }
        _ => not_found(),
    };
    Ok(resp)
}

fn status(ctx: &UiContext) -> serde_json::Value {
    let ni = pair_nodeinfo::collect(Some(ctx.node_id.clone()), None);
    let trusted: Vec<_> = ctx
        .pins
        .read()
        .map(|p| {
            p.iter()
                .map(|pp| json!({"node_uuid": pp.node_uuid, "fingerprint": pp.fingerprint}))
                .collect()
        })
        .unwrap_or_default();
    let snap = ctx.routing.read().map(|r| r.snapshot()).ok();
    json!({
        "node_uuid": ctx.node_id,
        "fingerprint": ctx.identity.fingerprint(),
        "ingress_port": ctx.ingress_port,
        "cpu": ni.cpu,
        "memory": ni.memory,
        "gpus": ni.gpus,
        "trusted": trusted,
        "local_models": snap.as_ref().map(|s| s.local_models.clone()).unwrap_or_default(),
        "peers": snap.map(|s| s.peers).unwrap_or_default(),
    })
}

fn add_cert(ctx: &UiContext, pem: &str) -> Response<Full<Bytes>> {
    match pair_trust::identity::pem_cert_to_der(pem) {
        Some(der) => match ctx.pins.write().unwrap().pin(&der) {
            Ok(uuid) => json_ok(json!({"ok": true, "node_uuid": uuid})),
            Err(e) => json_ok(json!({"ok": false, "error": e.to_string()})),
        },
        None => json_ok(json!({"ok": false, "error": "no certificate in PEM"})),
    }
}

/// Pair with another openpair node: pin its cert, push ours to it, and add it to
/// routing. `body` = `{"peer":"http://host:port"}`.
async fn pair(ctx: &UiContext, body: &[u8]) -> anyhow::Result<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(body)?;
    let peer = v
        .get("peer")
        .and_then(|p| p.as_str())
        .ok_or_else(|| anyhow::anyhow!("peer required"))?;
    let (host, port, _) = parse_url(peer)?;

    // 1. Fetch the peer's status (node uuid + ingress port).
    let (_, sbytes) = http(&host, port, "GET", "/api/status", &[]).await?;
    let st: serde_json::Value = serde_json::from_slice(&sbytes)?;
    let peer_uuid = st
        .get("node_uuid")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let peer_ingress = st.get("ingress_port").and_then(|x| x.as_u64()).unwrap_or(0) as u16;

    // 2. Fetch + pin the peer's certificate.
    let (_, cbytes) = http(&host, port, "GET", "/api/identity/cert", &[]).await?;
    let der = pair_trust::identity::pem_cert_to_der(&String::from_utf8_lossy(&cbytes))
        .ok_or_else(|| anyhow::anyhow!("peer returned no certificate"))?;
    ctx.pins.write().unwrap().pin(&der)?;

    // 3. Push our certificate to the peer so trust is mutual.
    let (code, _) = http(
        &host,
        port,
        "POST",
        "/api/trust/add",
        ctx.identity.cert_pem.as_bytes(),
    )
    .await?;
    anyhow::ensure!(code < 400, "peer rejected our certificate ({code})");

    // 4. Add the peer to the routing table (reachable at its ingress port).
    if peer_ingress != 0 {
        ctx.routing
            .write()
            .unwrap()
            .upsert_peer_meta(&peer_uuid, host.clone(), peer_ingress, true);
    }
    Ok(json!({"ok": true, "node_uuid": peer_uuid, "ingress_port": peer_ingress}))
}

// --- EAP-NOOB cluster pairing ---------------------------------------------

/// Invites awaiting a local PIN response (this node as joiner).
async fn pending_invites(ctx: &UiContext) -> serde_json::Value {
    let invites: Vec<_> = ctx
        .pairing
        .pending_invites()
        .await
        .into_iter()
        .map(|p| {
            json!({
                "invite_id": p.invite_id,
                "from_node_uuid": p.from_node_uuid,
                "from_name": p.from_name,
                "cluster_id": p.cluster_id,
                "cluster_friendly_name": p.cluster_friendly_name,
                "inviter_addr": p.inviter_addr,
            })
        })
        .collect();
    json!({ "invites": invites })
}

/// Inviter: drive an Initial Exchange to a joiner and return the PIN to display.
/// `body` = `{"joiner":"host[:port]"}`.
async fn invite(ctx: &UiContext, body: &[u8]) -> anyhow::Result<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(body)?;
    let joiner = v
        .get("joiner")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("joiner address required"))?;
    let addr = normalize_pairing_addr(joiner);
    let (invite_id, pin) = ctx.pairing.create_invite(&addr).await?;
    Ok(json!({ "ok": true, "invite_id": invite_id, "pin": pin, "joiner": addr }))
}

/// Joiner: submit the PIN for a pending invite, driving the Completion Exchange.
/// `body` = `{"invite_id":"…","pin":"123456"}`.
async fn respond(ctx: &UiContext, body: &[u8]) -> anyhow::Result<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_slice(body)?;
    let invite_id = v
        .get("invite_id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow::anyhow!("invite_id required"))?;
    let pin = v
        .get("pin")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .ok_or_else(|| anyhow::anyhow!("pin required"))?;
    let paired = ctx.pairing.submit_pin(invite_id, pin).await?;
    Ok(json!({
        "ok": true,
        "node_uuid": paired.peer.node_uuid,
        "name": paired.peer.name,
        "cluster_id": paired.peer.cluster_id,
        "cluster_friendly_name": paired.peer.cluster_friendly_name,
    }))
}

/// Normalize a bare `host` to `host:14321`, leaving an explicit port intact.
fn normalize_pairing_addr(input: &str) -> String {
    if input
        .rsplit_once(':')
        .map(|(_, p)| p.parse::<u16>().is_ok())
        == Some(true)
    {
        input.to_string()
    } else {
        format!("{input}:{}", pair_cluster::DEFAULT_PAIRING_PORT)
    }
}

// --- tiny plain-HTTP client + helpers -------------------------------------

fn parse_url(url: &str) -> anyhow::Result<(String, u16, String)> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow::anyhow!("only http:// URLs"))?;
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a.to_string(), format!("/{p}")),
        None => (rest.to_string(), "/".to_string()),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(80)),
        None => (authority, 80),
    };
    Ok((host, port, path))
}

async fn http(
    host: &str,
    port: u16,
    method: &str,
    path: &str,
    body: &[u8],
) -> anyhow::Result<(u16, Bytes)> {
    let stream = TcpStream::connect((host, port)).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", format!("{host}:{port}"))
        .body(Full::new(Bytes::copy_from_slice(body)))?;
    let resp = sender.send_request(req).await?;
    let code = resp.status().as_u16();
    let bytes = resp.into_body().collect().await?.to_bytes();
    Ok((code, bytes))
}

async fn read_body(req: Request<Incoming>) -> Bytes {
    req.into_body()
        .collect()
        .await
        .map(|b| b.to_bytes())
        .unwrap_or_default()
}

fn html(s: &str) -> Response<Full<Bytes>> {
    Response::builder()
        .header("content-type", "text/html; charset=utf-8")
        .body(Full::new(Bytes::from(s.to_string())))
        .unwrap()
}
fn text(s: String) -> Response<Full<Bytes>> {
    Response::builder()
        .header("content-type", "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from(s)))
        .unwrap()
}
fn json_ok(v: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(
            serde_json::to_vec(&v).unwrap_or_default(),
        )))
        .unwrap()
}
fn not_found() -> Response<Full<Bytes>> {
    Response::builder()
        .status(404)
        .body(Full::new(Bytes::from_static(b"not found")))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parsing() {
        assert_eq!(
            parse_url("http://1.2.3.4:7070").unwrap(),
            ("1.2.3.4".into(), 7070, "/".into())
        );
        assert_eq!(
            parse_url("http://host:80/api/status").unwrap(),
            ("host".into(), 80, "/api/status".into())
        );
        assert!(parse_url("https://x").is_err());
    }
}
