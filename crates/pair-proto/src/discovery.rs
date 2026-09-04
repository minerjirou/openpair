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

#[cfg(test)]
mod tests {
    use super::*;

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
