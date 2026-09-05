//! Browse _nvpair-node._tcp and dump raw TXT key/values from every resolved
//! service. Used to capture the reference's exact mDNS TXT keys.
use mdns_sd::{ServiceDaemon, ServiceEvent};

fn main() -> anyhow::Result<()> {
    let daemon = ServiceDaemon::new()?;
    let recv = daemon.browse("_nvpair-node._tcp.local.")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    while std::time::Instant::now() < deadline {
        if let Ok(ServiceEvent::ServiceResolved(info)) =
            recv.recv_timeout(std::time::Duration::from_millis(500))
        {
            println!(
                "== resolved: {} host={} port={}",
                info.get_fullname(),
                info.get_hostname(),
                info.get_port()
            );
            for a in info.get_addresses() {
                println!("   addr: {a}");
            }
            for p in info.get_properties().iter() {
                println!("   TXT: {}={}", p.key(), p.val_str());
            }
        }
    }
    Ok(())
}
