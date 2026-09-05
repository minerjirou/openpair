//! openpair node daemon (clean-room, PAIR-interoperable).
//!
//! Wires the clean-room crates into one runnable node:
//! * identity: Ed25519 node certificate ([`pair_trust`])
//! * telemetry: NVIDIA + AMD/ROCm GPU inventory ([`pair_nodeinfo`]) served at
//!   `GET /v1/node-info`
//! * discovery: advertise `_nvpair-node._tcp` and browse peers ([`pair_discovery`])
//! * data plane: loopback Ollama/OpenAI reverse proxy ([`pair_proxy`])
//!
//! Configuration (env):
//!   OPENPAIR_DATA_DIR     identity/store dir      (default: ./openpair-data)
//!   OPENPAIR_BACKEND      local engine authority  (default: 127.0.0.1:11434)
//!   OPENPAIR_PROXY_BIND   loopback proxy bind     (default: 127.0.0.1:11435)
//!   OPENPAIR_NODEINFO_BIND node-info http bind    (default: 127.0.0.1:7071)
//!   OPENPAIR_ADVERTISE_PORT mDNS advertised port  (default: 7443)

mod nodeinfo_server;

use pair_discovery::Discovery;
use pair_proto::NodeRecord;
use pair_trust::Identity;
use std::path::PathBuf;
use tracing::{info, warn};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let data_dir = PathBuf::from(env_or("OPENPAIR_DATA_DIR", "./openpair-data"));
    let backend = env_or("OPENPAIR_BACKEND", "127.0.0.1:11434");
    let proxy_bind: std::net::SocketAddr = env_or("OPENPAIR_PROXY_BIND", "127.0.0.1:11435").parse()?;
    let nodeinfo_bind: std::net::SocketAddr =
        env_or("OPENPAIR_NODEINFO_BIND", "127.0.0.1:7071").parse()?;
    let advertise_port: u16 = env_or("OPENPAIR_ADVERTISE_PORT", "7443").parse()?;

    // 1. Identity (Ed25519 node cert; UUID lives in its SAN).
    let identity = Identity::load_or_generate(&data_dir)?;
    let node_id = identity.node_uuid.clone();
    info!(node = %node_id, fingerprint = %identity.fingerprint(), "node identity ready");

    // 2. First telemetry snapshot (NVIDIA + AMD/ROCm).
    let ni = pair_nodeinfo::collect(Some(node_id.clone()), None);
    info!(
        gpus = ni.gpus.len(),
        telemetry_valid = ni.telemetry_valid,
        "telemetry ready"
    );
    for g in &ni.gpus {
        info!(vendor = ?g.vendor, name = %g.name, vram = ?g.vram_bytes, util = ?g.utilization_percent, "gpu");
    }

    // 3. node-info HTTP server (GET /v1/node-info).
    let ni_node_id = node_id.clone();
    tokio::spawn(async move {
        if let Err(e) = nodeinfo_server::serve(nodeinfo_bind, ni_node_id, std::future::pending()).await
        {
            warn!(error = %e, "node-info server exited");
        }
    });
    info!(%nodeinfo_bind, "serving GET /v1/node-info");

    // 4. Local reverse proxy (loopback -> local engine).
    let backend_for_proxy = backend.clone();
    tokio::spawn(async move {
        if let Err(e) =
            pair_proxy::server::serve_local(proxy_bind, backend_for_proxy, std::future::pending()).await
        {
            warn!(error = %e, "proxy exited");
        }
    });
    info!(%proxy_bind, backend = %backend, "loopback Ollama/OpenAI proxy up");

    // 5. Discovery: advertise + browse.
    let discovery = Discovery::new()?;
    let mut record = NodeRecord::default();
    record.node_uuid = Some(node_id.clone());
    let host = hostname();
    if let Err(e) = discovery.advertise(&node_id, &host, advertise_port, &record) {
        warn!(error = %e, "advertise failed");
    } else {
        info!(port = advertise_port, "advertising _nvpair-node._tcp");
    }
    let peers = discovery.browse()?;
    tokio::task::spawn_blocking(move || {
        while let Ok(peer) = peers.recv() {
            info!(instance = %peer.instance, host = %peer.host, port = peer.port,
                  node = ?peer.record.node_uuid, "discovered peer");
        }
    });

    info!("openpair-node running (Ctrl-C to stop)");
    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    let _ = discovery.daemon().shutdown();
    Ok(())
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "openpair-node".to_string())
}
