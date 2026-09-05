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
//!   OPENPAIR_CLUSTER_DIR  reference-compatible trust dir (node.crt/node.key/trusted/)
//!   OPENPAIR_INGRESS_BIND mTLS /ingress bind      (default: 0.0.0.0:7443)

mod nodeinfo_server;
mod trust;

use pair_discovery::Discovery;
use pair_proto::NodeRecord;
use pair_trust::{Identity, PeerPinStore};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Print which GPUs each detection backend found, then exit. Use on a real
/// AMD/ROCm host to validate the AMD path end to end.
fn gpu_check() {
    let show = |label: &str, gpus: Vec<pair_proto::Gpu>| {
        println!("[{label}] {} GPU(s)", gpus.len());
        for g in gpus {
            println!(
                "  - {:?} {}  vram={:?} used={:?} util={:?}",
                g.vendor, g.name, g.vram_bytes, g.vram_used_bytes, g.utilization_percent
            );
        }
    };
    show("nvidia-smi", pair_nodeinfo::nvidia::detect());
    show("amdgpu-sysfs", pair_nodeinfo::amd::detect_sysfs());
    show("amd-smi/rocm-smi", pair_nodeinfo::amd::detect_tools());
    show("os-inventory", pair_nodeinfo::gpu_os::os_gpus());
    println!("[merged] used by the node:");
    show("detect_gpus", pair_nodeinfo::detect_gpus());
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|a| a == "--gpucheck") {
        gpu_check();
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let data_dir = PathBuf::from(env_or("OPENPAIR_DATA_DIR", "./openpair-data"));
    let backend = env_or("OPENPAIR_BACKEND", "127.0.0.1:11434");
    let proxy_bind: std::net::SocketAddr =
        env_or("OPENPAIR_PROXY_BIND", "127.0.0.1:11435").parse()?;
    let nodeinfo_bind: std::net::SocketAddr =
        env_or("OPENPAIR_NODEINFO_BIND", "127.0.0.1:7071").parse()?;
    let advertise_port: u16 = env_or("OPENPAIR_ADVERTISE_PORT", "7443").parse()?;

    // 1. Identity + trust store.
    //    Prefer a reference-compatible cluster dir (node.crt/node.key/trusted/)
    //    so an openpair node can share a cluster directory with the reference;
    //    otherwise fall back to a standalone identity in the data dir.
    let (identity, pins): (Identity, pair_trust::SharedPins) =
        if let Ok(cdir) = std::env::var("OPENPAIR_CLUSTER_DIR") {
            let (id, store) = pair_trust::load_cluster_dir(&PathBuf::from(&cdir))?;
            info!(dir = %cdir, pinned = store.len(), "loaded reference-compatible cluster dir");
            (id, Arc::new(RwLock::new(store)))
        } else {
            let id = Identity::load_or_generate(&data_dir)?;
            (id, Arc::new(RwLock::new(PeerPinStore::new())))
        };
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
        if let Err(e) =
            nodeinfo_server::serve(nodeinfo_bind, ni_node_id, std::future::pending()).await
        {
            warn!(error = %e, "node-info server exited");
        }
    });
    info!(%nodeinfo_bind, "serving GET /v1/node-info");

    // 4. Cluster trust + mTLS /ingress receiver.
    //    Peer trust is normally established by pairing (EAP-NOOB). For local
    //    multi-node testing before that is byte-exact, an optional dev-trust
    //    directory lets nodes publish their cert and pin each other's.
    if let Ok(dir) = std::env::var("OPENPAIR_TRUST_DIR") {
        match trust::bootstrap_dev_trust(&PathBuf::from(&dir), &identity, &pins) {
            Ok(n) => info!(dir = %dir, pinned = n, "dev-trust: published cert and pinned peers"),
            Err(e) => warn!(error = %e, "dev-trust bootstrap failed"),
        }
    }
    let ingress_bind: std::net::SocketAddr =
        env_or("OPENPAIR_INGRESS_BIND", "0.0.0.0:7443").parse()?;
    let ing_id = identity.clone();
    let ing_pins = pins.clone();
    let ing_backend = backend.clone();
    tokio::spawn(async move {
        if let Err(e) = pair_proxy::ingress_server::serve_ingress(
            ingress_bind,
            &ing_id,
            ing_pins,
            ing_backend,
            std::future::pending(),
        )
        .await
        {
            warn!(error = %e, "ingress receiver exited");
        }
    });
    info!(%ingress_bind, "serving mutual-TLS /ingress for peers");

    // 5. Routing table + model pollers + cluster-aware loopback proxy.
    let routing = Arc::new(RwLock::new(pair_proxy::RoutingTable::new(node_id.clone())));
    let id_arc = Arc::new(identity.clone());

    // Poll the local engine's model list.
    {
        let routing = routing.clone();
        let backend = backend.clone();
        tokio::spawn(async move {
            loop {
                match pair_proxy::tags::fetch_local_models(&backend).await {
                    Ok(models) => {
                        let n = models.len();
                        routing.write().unwrap().set_local_models(models);
                        tracing::debug!(models = n, "refreshed local models");
                    }
                    Err(e) => tracing::debug!(error = %e, "local /api/tags unavailable"),
                }
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            }
        });
    }

    // Poll pinned peers' models through mutual-TLS /ingress.
    {
        let routing = routing.clone();
        let pins = pins.clone();
        let id_arc = id_arc.clone();
        tokio::spawn(async move {
            loop {
                let peers = routing.read().unwrap().pinned_peers();
                for (nid, host, port) in peers {
                    if let Ok(models) =
                        pair_proxy::tags::fetch_peer_models(&id_arc, pins.clone(), &host, port)
                            .await
                    {
                        routing.write().unwrap().set_peer_models(&nid, models);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            }
        });
    }

    // Cluster-aware proxy: routes each request to the local engine or a pinned
    // peer that advertises the requested model.
    {
        let ctx = pair_proxy::ProxyContext {
            backend: backend.clone(),
            identity: id_arc.clone(),
            pins: pins.clone(),
            routing: routing.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = pair_proxy::serve_routing(proxy_bind, ctx, std::future::pending()).await
            {
                warn!(error = %e, "proxy exited");
            }
        });
    }
    info!(%proxy_bind, backend = %backend, "cluster-aware Ollama/OpenAI proxy up");

    // 6. Discovery: advertise + browse -> populate the routing table.
    let discovery = Discovery::new()?;
    let record = NodeRecord {
        node_uuid: Some(node_id.clone()),
        ..Default::default()
    };
    let host = hostname();
    if let Err(e) = discovery.advertise(&node_id, &host, advertise_port, &record) {
        warn!(error = %e, "advertise failed");
    } else {
        info!(port = advertise_port, "advertising _nvpair-node._tcp");
    }
    let peers = discovery.browse()?;
    {
        let routing = routing.clone();
        let pins = pins.clone();
        let self_id = node_id.clone();
        tokio::task::spawn_blocking(move || {
            while let Ok(peer) = peers.recv() {
                let pnid = match peer.record.node_uuid.clone() {
                    Some(id) if id != self_id => id,
                    _ => continue,
                };
                let host = peer
                    .addresses
                    .first()
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| peer.host.trim_end_matches('.').to_string());
                let pinned = pins.read().unwrap().get(&pnid).is_some();
                routing
                    .write()
                    .unwrap()
                    .upsert_peer_meta(&pnid, host, peer.port, pinned);
                info!(peer = %pnid, port = peer.port, pinned, "peer added to routing");
            }
        });
    }

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
