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
//!   OPENPAIR_ADVERTISE_PORT mDNS advertised port  (default: the node-info port)
//!   OPENPAIR_CLUSTER_DIR  reference-compatible trust dir (node.crt/node.key/trusted/)
//!   OPENPAIR_INGRESS_BIND mTLS /ingress bind      (default: 0.0.0.0:7443)
//!   OPENPAIR_UI_BIND      dashboard UI bind       (default: 127.0.0.1:7070)

mod nodeinfo_server;
mod pairing;
mod trust;

use pair_cluster::TrustSink;
use pair_discovery::Discovery;
use pair_proto::NodeRecord;
use pair_trust::{Identity, PeerPinStore};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::{info, warn};

/// Default plain-HTTP pairing-channel bind (`/v1/cluster/pairing`, §7.2).
const DEFAULT_PAIRING_BIND: &str = "0.0.0.0:14321";

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
    // Operator-driven cluster pairing subcommands (§7.2):
    //   openpair-node invite <joiner-host[:port]>   grow this node's cluster
    //   openpair-node join                          wait to be invited, enter PIN
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("invite") => {
            init_tracing();
            return run_invite(args.get(2).cloned()).await;
        }
        Some("join") => {
            init_tracing();
            return run_join().await;
        }
        _ => {}
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

    // 4b. Cluster pairing channel (plain-HTTP /v1/cluster/pairing, §7.2).
    //     Serving it lets a cluster inviter reach this node; the operator drives
    //     the PIN step via `openpair-node join`. Pairing establishes mutual-TLS
    //     trust by pinning the peer's authenticated certificate.
    {
        let pairing_bind: std::net::SocketAddr =
            env_or("OPENPAIR_PAIRING_BIND", DEFAULT_PAIRING_BIND).parse()?;
        let cluster_dir = std::env::var("OPENPAIR_CLUSTER_DIR").ok().map(PathBuf::from);
        let sink = pairing::ClusterTrustSink::new(
            pins.clone(),
            cluster_dir,
            !pins.read().unwrap().is_empty(),
        );
        let advertised = format!(
            "{}:{}",
            local_ip().unwrap_or_else(|| "127.0.0.1".into()),
            pairing_bind.port()
        );
        let profile = pairing::node_profile(
            Arc::new(identity.clone()),
            hostname(),
            advertised.clone(),
            String::new(),
            String::new(),
        );
        let node = pairing::build_node(profile, sink);
        tokio::spawn(async move {
            if let Err(e) =
                pair_cluster::serve_pairing(pairing_bind, node, std::future::pending()).await
            {
                warn!(error = %e, "pairing channel exited");
            }
        });
        info!(%pairing_bind, advertised = %advertised, "serving /v1/cluster/pairing (EAP-NOOB)");
    }

    // 5. Routing table + model pollers + cluster-aware loopback proxy.
    let routing = Arc::new(RwLock::new(pair_proxy::RoutingTable::new(node_id.clone())));
    let id_arc = Arc::new(identity.clone());

    // UI dashboard + control API (operator console; bind to loopback).
    {
        let ui_bind: std::net::SocketAddr = env_or("OPENPAIR_UI_BIND", "127.0.0.1:7070").parse()?;
        let ui_ctx = Arc::new(pair_ui::UiContext {
            node_id: node_id.clone(),
            identity: id_arc.clone(),
            pins: pins.clone(),
            routing: routing.clone(),
            ingress_port: ingress_bind.port(),
        });
        tokio::spawn(async move {
            if let Err(e) = pair_ui::serve(ui_bind, ui_ctx, std::future::pending()).await {
                warn!(error = %e, "ui server exited");
            }
        });
        info!(%ui_bind, "serving dashboard UI");
    }

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
    //    Confirmed from a reference advertisement: the SRV port is the node-info
    //    port (peers fetch /v1/node-info from it), and the TXT carries
    //    v=1 / uuid=<node> / ip=<addr>. OPENPAIR_ADVERTISE_PORT overrides.
    let discovery = Discovery::new()?;
    let local_ip = local_ip();
    let record = NodeRecord {
        node_uuid: Some(node_id.clone()),
        addresses: local_ip.iter().cloned().collect(),
        ..Default::default()
    };
    let advertise_port = std::env::var("OPENPAIR_ADVERTISE_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| nodeinfo_bind.port());
    let host = hostname();
    if let Err(e) = discovery.advertise(&node_id, &host, advertise_port, &record) {
        warn!(error = %e, "advertise failed");
    } else {
        info!(port = advertise_port, ip = ?local_ip, "advertising _nvpair-node._tcp");
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

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .try_init();
}

/// Load this node's identity + live pin store, preferring a reference-compatible
/// cluster directory. Returns the optional cluster dir for trust persistence.
fn load_identity_pins() -> anyhow::Result<(Identity, pair_trust::SharedPins, Option<PathBuf>)> {
    if let Ok(cdir) = std::env::var("OPENPAIR_CLUSTER_DIR") {
        let dir = PathBuf::from(&cdir);
        let (id, store) = pair_trust::load_cluster_dir(&dir)?;
        Ok((id, Arc::new(RwLock::new(store)), Some(dir)))
    } else {
        let data_dir = PathBuf::from(env_or("OPENPAIR_DATA_DIR", "./openpair-data"));
        let id = Identity::load_or_generate(&data_dir)?;
        Ok((id, Arc::new(RwLock::new(PeerPinStore::new())), None))
    }
}

/// Build a [`pair_cluster::PairingNode`] + its serving future for an
/// operator-driven pairing flow, returning the node, the trust sink, and the
/// bound pairing address.
async fn pairing_endpoint() -> anyhow::Result<(
    Arc<pair_cluster::PairingNode>,
    Arc<pairing::ClusterTrustSink>,
    std::net::SocketAddr,
)> {
    let (identity, pins, cluster_dir) = load_identity_pins()?;
    let pairing_bind: std::net::SocketAddr =
        env_or("OPENPAIR_PAIRING_BIND", DEFAULT_PAIRING_BIND).parse()?;
    let already = !pins.read().unwrap().is_empty();
    let sink = pairing::ClusterTrustSink::new(pins, cluster_dir, already);
    let advertised = format!(
        "{}:{}",
        local_ip().unwrap_or_else(|| "127.0.0.1".into()),
        pairing_bind.port()
    );
    let profile = pairing::node_profile(
        Arc::new(identity),
        hostname(),
        advertised,
        String::new(),
        String::new(),
    );
    let node = pairing::build_node(profile, sink.clone());
    let srv = node.clone();
    tokio::spawn(async move {
        if let Err(e) = pair_cluster::serve_pairing(pairing_bind, srv, std::future::pending()).await
        {
            warn!(error = %e, "pairing channel exited");
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    Ok((node, sink, pairing_bind))
}

/// Inviter: drive an Initial Exchange to `joiner`, print the PIN, and wait for
/// the joiner to complete the join (§7.2).
async fn run_invite(joiner: Option<String>) -> anyhow::Result<()> {
    let joiner = joiner.ok_or_else(|| {
        anyhow::anyhow!("usage: openpair-node invite <joiner-host[:port]>")
    })?;
    let joiner_addr = normalize_pairing_addr(&joiner);
    let (node, sink, _bind) = pairing_endpoint().await?;

    info!(joiner = %joiner_addr, "inviting node to this cluster");
    let (invite_id, pin) = node.create_invite(&joiner_addr).await?;
    println!("\n=== Cluster invite created ===");
    println!("  invite id : {invite_id}");
    println!("  PIN       : {pin}");
    println!("Enter this PIN on the joining node (`openpair-node join`).\n");

    // Wait for the joiner-driven Completion Exchange to establish trust.
    for _ in 0..600 {
        if sink.is_clustered() {
            println!("Peer joined and its certificate is pinned. Pairing complete.");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    anyhow::bail!("timed out waiting for the joiner to enter the PIN")
}

/// Joiner: wait to be invited, then enter the PIN shown by the inviter (§7.2).
async fn run_join() -> anyhow::Result<()> {
    let (node, sink, bind) = pairing_endpoint().await?;
    let advertised = format!(
        "{}:{}",
        local_ip().unwrap_or_else(|| "127.0.0.1".into()),
        bind.port()
    );
    println!("\nWaiting to be invited at {advertised}.");
    println!("Ask the cluster owner to run: openpair-node invite {advertised}\n");

    // Wait for an inbound invite (the inviter drives the Initial Exchange to us).
    let invite = loop {
        if sink.is_clustered() {
            println!("Already clustered; nothing to do.");
            return Ok(());
        }
        let pending = node.pending_invites().await;
        if let Some(inv) = pending.into_iter().next() {
            break inv;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    };
    println!(
        "Invited to cluster \"{}\" by {} ({}).",
        invite.cluster_friendly_name, invite.from_name, invite.from_node_uuid
    );

    let pin = prompt_pin().await?;
    let paired = node.submit_pin(&invite.invite_id, &pin).await?;
    println!(
        "\nJoined cluster \"{}\" via {}. Inviter certificate pinned.",
        paired.peer.cluster_friendly_name, paired.peer.name
    );
    Ok(())
}

/// Normalize a bare `host` to `host:14321`, leaving an explicit port intact.
fn normalize_pairing_addr(input: &str) -> String {
    if input.rsplit_once(':').map(|(_, p)| p.parse::<u16>().is_ok()) == Some(true) {
        input.to_string()
    } else {
        format!("{input}:{}", pair_cluster::DEFAULT_PAIRING_PORT)
    }
}

/// Read a six-digit PIN from stdin (blocking read off the async runtime).
async fn prompt_pin() -> anyhow::Result<String> {
    print!("Enter the 6-digit PIN shown by the inviter: ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let pin = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map(|_| line)
    })
    .await??;
    Ok(pin.trim().to_string())
}

/// Best-effort primary (outbound) IPv4 of this host, for the mDNS `ip` TXT.
fn local_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // No packet is sent; connect just selects a source address.
    sock.connect("8.8.8.8:80").ok()?;
    sock.local_addr().ok().map(|a| a.ip().to_string())
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
