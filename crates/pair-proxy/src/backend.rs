//! Forward a raw HTTP/1 request to the local engine backend and buffer the
//! response. Shared by the local proxy and the peer `/ingress` receiver.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

/// Send `method path` with `body` to `backend` (host:port); return (status, body).
pub async fn forward_raw(
    backend: &str,
    method: &str,
    path: &str,
    body: Bytes,
) -> anyhow::Result<(u16, Bytes)> {
    let stream = TcpStream::connect(backend).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", backend)
        .header("content-type", "application/json")
        .body(Full::new(body))?;
    let resp = sender.send_request(req).await?;
    let status = resp.status().as_u16();
    let bytes = resp.into_body().collect().await?.to_bytes();
    Ok((status, bytes))
}
