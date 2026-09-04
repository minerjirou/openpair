//! openpair node daemon entrypoint (clean-room, work in progress).
//!
//! Current milestone: collect GPU/host telemetry (NVIDIA + AMD/ROCm), advertise
//! this host on the cluster mDNS service, and log discovered peers. Cluster
//! security (pairing/mTLS) and the proxy data plane are added in later phases.

use pair_discovery::Discovery;
use pair_proto::NodeRecord;
use tracing::info;

fn node_uuid() -> String {
    // Prefer a stable host id; fall back to a random v4 UUID.
    for p in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(s) = std::fs::read_to_string(p) {
            let s = s.trim();
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    uuid::Uuid::new_v4().to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let node_id = node_uuid();
    info!(node = %node_id, "openpair-node starting");

    // 1. Telemetry (NVIDIA + AMD/ROCm).
    let ni = pair_nodeinfo::collect(Some(node_id.clone()), None);
    info!(
        gpus = ni.gpus.len(),
        telemetry_valid = ni.telemetry_valid,
        free_vram_bytes = ni.free_vram_bytes(),
        "collected node telemetry"
    );
    for g in &ni.gpus {
        info!(
            vendor = ?g.vendor, name = %g.name,
            vram_bytes = ?g.vram_bytes, used = ?g.vram_used_bytes, util = ?g.utilization_percent,
            "gpu"
        );
    }

    // 2. Discovery: advertise self + browse peers.
    let discovery = Discovery::new()?;
    let mut record = NodeRecord::default();
    record.node_uuid = Some(node_id.clone());
    let host = hostname();
    let advertise_port = 7443; // TODO(interop): confirm the reference's advertised port
    if let Err(e) = discovery.advertise(&node_id, &host, advertise_port, &record) {
        info!(error = %e, "advertise failed (continuing to browse)");
    } else {
        info!(port = advertise_port, "advertising on _nvpair-node._tcp");
    }

    let peers = discovery.browse()?;
    info!("browsing for peers (Ctrl-C to stop)");
    // Drain discovered peers on a blocking task and log them.
    tokio::task::spawn_blocking(move || {
        while let Ok(peer) = peers.recv() {
            info!(
                instance = %peer.instance,
                host = %peer.host,
                port = peer.port,
                node = ?peer.record.node_uuid,
                cluster = ?peer.record.cluster_uuid,
                "discovered peer"
            );
        }
    });

    // Run until interrupted.
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
