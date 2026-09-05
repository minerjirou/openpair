//! mDNS / DNS-SD discovery for the PAIR cluster service `_nvpair-node._tcp`.
//!
//! The reference implementation ships a custom mDNS responder/browser, but it
//! speaks standard DNS-SD, so a standards-compliant stack interoperates. This
//! crate advertises this host and browses for peers, mapping the service TXT
//! record to/from [`pair_proto::NodeRecord`].
//!
//! TODO(interop): the exact TXT key spellings are being confirmed by protocol
//! analysis; [`pair_proto::NodeRecord`] preserves unknown keys so nothing is
//! lost meanwhile.

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use pair_proto::{NodeRecord, MDNS_SERVICE_TYPE};
use std::collections::HashMap;
use tracing::{debug, warn};

/// Wraps an mDNS daemon and exposes advertise + browse for the cluster service.
pub struct Discovery {
    daemon: ServiceDaemon,
}

/// A peer discovered on the LAN.
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    /// mDNS instance/fullname.
    pub instance: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<std::net::IpAddr>,
    pub record: NodeRecord,
}

impl Discovery {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            daemon: ServiceDaemon::new()?,
        })
    }

    /// Advertise this node as `_nvpair-node._tcp` with the given instance name,
    /// port and TXT properties derived from a [`NodeRecord`].
    pub fn advertise(
        &self,
        instance_name: &str,
        host_name: &str,
        port: u16,
        record: &NodeRecord,
    ) -> anyhow::Result<()> {
        let props = record_to_txt(record);
        // host_name must end with ".local." for mDNS.
        let host = if host_name.ends_with(".local.") {
            host_name.to_string()
        } else {
            format!("{host_name}.local.")
        };
        let info = ServiceInfo::new(
            &format!("{MDNS_SERVICE_TYPE}.local."),
            instance_name,
            &host,
            "",
            port,
            props,
        )?
        .enable_addr_auto();
        self.daemon.register(info)?;
        debug!(
            instance = instance_name,
            port, "advertising _nvpair-node._tcp"
        );
        Ok(())
    }

    /// Start browsing for peers. Returns a receiver of [`DiscoveredPeer`]s;
    /// resolution events are translated as they arrive.
    pub fn browse(&self) -> anyhow::Result<std::sync::mpsc::Receiver<DiscoveredPeer>> {
        let recv = self.daemon.browse(&format!("{MDNS_SERVICE_TYPE}.local."))?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(event) = recv.recv() {
                if let ServiceEvent::ServiceResolved(info) = event {
                    let record = txt_to_record(&info);
                    let peer = DiscoveredPeer {
                        instance: info.get_fullname().to_string(),
                        host: info.get_hostname().to_string(),
                        port: info.get_port(),
                        addresses: info.get_addresses().iter().copied().collect(),
                        record,
                    };
                    if tx.send(peer).is_err() {
                        break;
                    }
                }
            }
            warn!("mDNS browse channel closed");
        });
        Ok(rx)
    }

    /// Access the underlying daemon (e.g. to shut down).
    pub fn daemon(&self) -> &ServiceDaemon {
        &self.daemon
    }
}

/// Convert a [`NodeRecord`] to mDNS TXT key/value properties.
fn record_to_txt(record: &NodeRecord) -> HashMap<String, String> {
    use pair_proto::contract::txt_keys;
    let mut props = HashMap::new();
    if let Some(u) = &record.node_uuid {
        props.insert(txt_keys::NODE_UUID.to_string(), u.clone());
    }
    if let Some(c) = &record.cluster_uuid {
        props.insert(txt_keys::CLUSTER_UUID.to_string(), c.clone());
    }
    for (k, v) in &record.extra {
        props.insert(k.clone(), v.clone());
    }
    props
}

/// Convert resolved mDNS TXT properties into a [`NodeRecord`].
fn txt_to_record(info: &ServiceInfo) -> NodeRecord {
    let entries: Vec<String> = info
        .get_properties()
        .iter()
        .map(|p| format!("{}={}", p.key(), p.val_str()))
        .collect();
    let mut rec = NodeRecord::from_txt(entries.iter().map(String::as_str));
    for addr in info.get_addresses() {
        rec.addresses.push(format!("{addr}:{}", info.get_port()));
    }
    rec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_roundtrip_keys() {
        let rec = NodeRecord {
            node_uuid: Some("n-1".into()),
            cluster_uuid: Some("c-1".into()),
            ..Default::default()
        };
        let props = record_to_txt(&rec);
        assert_eq!(props.get("nodeUuid").map(String::as_str), Some("n-1"));
        assert_eq!(props.get("cluster-uuid").map(String::as_str), Some("c-1"));
    }
}
