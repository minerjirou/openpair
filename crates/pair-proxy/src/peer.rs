//! Peer routing: forward a request to a pinned peer's `/ingress` over mutual TLS.
//!
//! When [`crate::router::select`] picks a peer, the proxy wraps the upstream
//! request in an [`IngressEnvelope`] and POSTs it to the peer's `/ingress`
//! endpoint over a mutually-authenticated, certificate-pinned TLS 1.3
//! connection (built by `pair-trust`). The peer unwraps and hits its local
//! engine, returning the response in the envelope.

use crate::ingress::IngressEnvelope;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_util::rt::TokioIo;
use pair_proto::contract::endpoints;
use pair_trust::{Identity, SharedPins};
use rustls_pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// POST an ingress envelope to `host:port`/ingress over mutual TLS, returning
/// the peer's ingress response envelope.
pub async fn forward_to_peer(
    identity: &Identity,
    pins: SharedPins,
    host: &str,
    port: u16,
    envelope: &IngressEnvelope,
) -> anyhow::Result<IngressEnvelope> {
    let config = pair_trust::client_config(identity, pins)?;
    let connector = TlsConnector::from(Arc::new(config));

    let tcp = TcpStream::connect((host, port)).await?;
    // The pinned verifier ignores the SNI name (trust is by pinned DER), but a
    // syntactically valid ServerName is still required by the TLS stack.
    let server_name = ServerName::try_from(host.to_string())
        .unwrap_or_else(|_| ServerName::try_from("peer.local").unwrap());
    let tls = connector.connect(server_name, tcp).await?;

    let io = TokioIo::new(tls);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let body = serde_json::to_vec(envelope)?;
    let req = Request::builder()
        .method("POST")
        .uri(endpoints::INGRESS)
        .header("content-type", "application/json")
        .header("host", host)
        .body(Full::new(Bytes::from(body)))?;

    let resp = sender.send_request(req).await?;
    let status = resp.status();
    let bytes = resp.into_body().collect().await?.to_bytes();
    if !status.is_success() {
        anyhow::bail!("peer /ingress returned {status}");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::body::Incoming;
    use hyper::service::service_fn;
    use hyper::Response;
    use pair_trust::PeerPinStore;
    use std::convert::Infallible;
    use std::sync::RwLock;
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    /// A pinned-mTLS `/ingress` server that echoes the envelope's `name` back in
    /// `txt` with code 200 — exercises the full mutual-TLS + ingress round-trip.
    #[tokio::test]
    async fn mtls_ingress_roundtrip() {
        let server_id = Identity::generate().unwrap();
        let client_id = Identity::generate().unwrap();

        // Mutual pinning.
        let server_pins = Arc::new(RwLock::new(PeerPinStore::new()));
        server_pins.write().unwrap().pin(&client_id.cert_der).unwrap();
        let client_pins = Arc::new(RwLock::new(PeerPinStore::new()));
        client_pins.write().unwrap().pin(&server_id.cert_der).unwrap();

        // Start the TLS ingress server on an ephemeral port.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_config = pair_trust::server_config(&server_id, server_pins).unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let tls = acceptor.accept(tcp).await.unwrap();
            let io = TokioIo::new(tls);
            let svc = service_fn(|req: Request<Incoming>| async move {
                let body = req.into_body().collect().await.unwrap().to_bytes();
                let env: IngressEnvelope = serde_json::from_slice(&body).unwrap();
                let reply = IngressEnvelope {
                    txt: env.name.clone(),
                    code: Some(200),
                    ..Default::default()
                };
                let out = serde_json::to_vec(&reply).unwrap();
                Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(out))))
            });
            hyper::server::conn::http1::Builder::new()
                .serve_connection(io, svc)
                .await
                .unwrap();
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let env = IngressEnvelope {
            name: Some("llama3".into()),
            path: Some("/api/generate".into()),
            data: Some("{\"model\":\"llama3\"}".into()),
            ..Default::default()
        };
        let resp = forward_to_peer(&client_id, client_pins, "127.0.0.1", addr.port(), &env)
            .await
            .unwrap();
        assert_eq!(resp.code, Some(200));
        assert_eq!(resp.txt.as_deref(), Some("llama3"));
    }

    #[tokio::test]
    async fn unpinned_peer_is_rejected() {
        let server_id = Identity::generate().unwrap();
        let client_id = Identity::generate().unwrap();
        // Server pins the client, but the client does NOT pin the server.
        let server_pins = Arc::new(RwLock::new(PeerPinStore::new()));
        server_pins.write().unwrap().pin(&client_id.cert_der).unwrap();
        let empty_pins = Arc::new(RwLock::new(PeerPinStore::new()));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_config = pair_trust::server_config(&server_id, server_pins).unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(server_config));
        tokio::spawn(async move {
            if let Ok((tcp, _)) = listener.accept().await {
                let _ = acceptor.accept(tcp).await; // handshake will fail
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let env = IngressEnvelope::default();
        // Client cannot verify the (unpinned) server -> handshake fails.
        let res = forward_to_peer(&client_id, empty_pins, "127.0.0.1", addr.port(), &env).await;
        assert!(res.is_err());
    }
}
