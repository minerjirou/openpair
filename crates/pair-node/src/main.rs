//! openpair node daemon entrypoint (clean-room, work in progress).

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let gpus = pair_nodeinfo::detect_gpus();
    tracing::info!(gpu_count = gpus.len(), "openpair-node starting; detected GPUs");
    for g in &gpus {
        println!(
            "GPU {:?} {} vram={:?} used={:?} util={:?}",
            g.vendor, g.name, g.vram_bytes, g.vram_used_bytes, g.utilization_percent
        );
    }
    Ok(())
}
