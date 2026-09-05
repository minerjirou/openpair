//! LAN discovery contract (mDNS / DNS-SD).
//!
//! Confirmed: the DNS-SD service type advertised/browsed by nodes is
//! `_nvpair-node._tcp`. Node metadata (UUID, cluster UUID, addresses) travels in
//! the service's TXT record. The exact TXT key set is being confirmed by the
//! protocol-analysis pass; `NodeRecord` models the keys observed so far.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// DNS-SD service type used for node discovery on the local network.
pub const MDNS_SERVICE_TYPE: &str = "_nvpair-node._tcp";

/// A node record as advertised in / parsed from an mDNS TXT record.
///
/// TXT records are a set of `key=value` byte strings. Known keys include the
/// node UUID and cluster UUID; unknown keys are preserved in `extra` so an
/// interoperating node neither drops nor corrupts fields it does not model yet.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeRecord {
    pub node_uuid: Option<String>,
    pub cluster_uuid: Option<String>,
    /// Advertised reachable addresses (host:port), when present in TXT.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// Any TXT keys not yet modelled, preserved verbatim for round-tripping.
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl NodeRecord {
    /// Parse a set of raw `key=value` TXT strings into a record.
    ///
    /// TODO(interop): confirm the exact key spellings against the reference
    /// (`noderec.TXT` / `ClusterUUIDFromTXT`); current guesses are recorded here
    /// and everything unrecognised is retained in `extra`.
    pub fn from_txt<'a, I: IntoIterator<Item = &'a str>>(entries: I) -> Self {
        let mut rec = NodeRecord::default();
        for e in entries {
            let (k, v) = match e.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            match k {
                "uuid" | "node" | "nodeUuid" | "node_uuid" | "node-uuid" => {
                    rec.node_uuid = Some(v.to_string())
                }
                // `cluster-uuid` is byte-verified (ClusterUUIDFromTXT); accept the
                // other spellings defensively.
                "cluster" | "cluster-uuid" | "clusterUuid" | "cluster_uuid" => {
                    rec.cluster_uuid = Some(v.to_string())
                }
                "addr" | "addresses" => rec.addresses.push(v.to_string()),
                _ => {
                    rec.extra.insert(k.to_string(), v.to_string());
                }
            }
        }
        rec
    }
}

/// A node as published in the `discovery:node-discovered` notification.
/// Field names captured live from the reference worker.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeAdvert {
    #[serde(rename = "hostUuid", default, skip_serializing_if = "Option::is_none")]
    pub host_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(default)]
    pub ips: Vec<String>,
    #[serde(default)]
    pub trusted: bool,
    /// service name -> port/details (shape varies; kept as raw JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub services: Option<serde_json::Value>,
    #[serde(rename = "lastSeen", default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<i64>,
}

/// A cluster member as published in the `nodes:changed` notification.
/// Field names captured live from the reference worker.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ClusterMember {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "nodeUuid", default, skip_serializing_if = "Option::is_none")]
    pub node_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(rename = "ipAddress", default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(rename = "clusterId", default, skip_serializing_if = "Option::is_none")]
    pub cluster_id: Option<String>,
    #[serde(
        rename = "admissionEpoch",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub admission_epoch: Option<u64>,
    /// e.g. "member".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(rename = "joinedAt", default, skip_serializing_if = "Option::is_none")]
    pub joined_at: Option<i64>,
    #[serde(rename = "lastSeen", default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advert_parses_reference_capture() {
        // Captured from discovery:node-discovered.
        let raw = r#"{"hostUuid":"8daa6983","name":"HOST","ip":"192.168.0.130",
            "ips":["192.168.0.130"],"trusted":false,"services":{},"lastSeen":1788597359}"#;
        let a: NodeAdvert = serde_json::from_str(raw).unwrap();
        assert_eq!(a.host_uuid.as_deref(), Some("8daa6983"));
        assert_eq!(a.ip.as_deref(), Some("192.168.0.130"));
        assert!(!a.trusted);
    }

    #[test]
    fn member_parses_reference_capture() {
        // Captured from nodes:changed.
        let raw = r#"{"id":"HOST","nodeUuid":"2593fc2b","name":"HOST","ipAddress":"127.0.0.1",
            "port":14321,"clusterId":"d92ef9a9","admissionEpoch":1,"state":"member",
            "joinedAt":1788597719663,"lastSeen":null}"#;
        let m: ClusterMember = serde_json::from_str(raw).unwrap();
        assert_eq!(m.node_uuid.as_deref(), Some("2593fc2b"));
        assert_eq!(m.port, Some(14321));
        assert_eq!(m.state.as_deref(), Some("member"));
        assert_eq!(m.admission_epoch, Some(1));
    }

    #[test]
    fn service_type_is_exact() {
        assert_eq!(MDNS_SERVICE_TYPE, "_nvpair-node._tcp");
    }

    #[test]
    fn txt_parse_keeps_unknown() {
        let rec = NodeRecord::from_txt(["uuid=n1", "cluster=c1", "weird=42"]);
        assert_eq!(rec.node_uuid.as_deref(), Some("n1"));
        assert_eq!(rec.cluster_uuid.as_deref(), Some("c1"));
        assert_eq!(rec.extra.get("weird").map(String::as_str), Some("42"));
    }
}
