//! Confirmed protocol identifiers: JSON-RPC method names, HTTP endpoint paths,
//! and mDNS TXT keys. Reproduced verbatim from the observed wire contract so the
//! implementation and the reference agree byte-for-byte. See `docs/protocol-rpc.md`
//! for the full catalog and provenance.

/// Domain JSON-RPC method / notification names (Electron ↔ broker ↔ workers).
pub mod methods {
    // discovery / nodes
    pub const DISCOVERY_SUBSCRIBE: &str = "discovery:subscribe";
    pub const DISCOVERY_GET_NODES: &str = "discovery:get-nodes";
    pub const DISCOVERY_NODES_CHANGED: &str = "discovery:nodes-changed";
    pub const DISCOVERY_REGISTER: &str = "discovery:register";
    pub const NODE_ADD: &str = "node/add";
    pub const NODE_ADD_MANUAL: &str = "node/add-manual";
    pub const NODE_DISCOVERED: &str = "node/discovered";
    pub const NODE_UPDATED: &str = "node/updated";
    pub const NODE_REMOVED: &str = "node/removed";
    pub const NODE_SET_PRIORITY: &str = "node/set-priority";

    // scheduler
    pub const SCHEDULE_PRIORITY: &str = "schedule:priority";

    // engine lifecycle
    pub const ENGINE_SUBSCRIBE: &str = "engine:subscribe";
    pub const ENGINE_STATE_CHANGED: &str = "engine:state-changed";
    pub const ENGINE_START: &str = "engine:start";
    pub const ENGINE_STOP: &str = "engine:stop";
    pub const ENGINE_INSTALL: &str = "engine:install";
    pub const ENGINE_INSTALL_PROGRESS: &str = "engine:install-progress";
    pub const ENGINE_READY: &str = "engine:ready";
    pub const ENGINE_GET_INSTALLED: &str = "engine:get-installed";

    // cluster / pairing
    pub const CLUSTER_INVITE_NODE: &str = "cluster:invite-node";
    pub const CLUSTER_RESPOND_TO_INVITE: &str = "cluster:respond-to-invite";
    pub const CLUSTER_INVITE_STATUS: &str = "cluster:invite-status";
    pub const CLUSTER_INVITE_RECEIVED: &str = "cluster:invite-received";
    pub const CLUSTER_GET_INITIAL: &str = "cluster:get-initial";
    pub const CLUSTER_LEAVE: &str = "cluster:leave";
    pub const CLUSTER_SET_IDENTITY: &str = "cluster:set-identity";
    pub const CLUSTER_IDENTITY_CHANGED: &str = "cluster:identity-changed";

    // errors / settings / logging / proxy
    pub const ERRORS_REPORT: &str = "errors:report";
    pub const ERRORS_UPDATE: &str = "errors:update";
    pub const SETTINGS_GET_CLUSTER_ID: &str = "settings/get-cluster-id";
    pub const SETTINGS_SET_CLUSTER_ID: &str = "settings/set-cluster-id";
    pub const LOG_SET_LEVEL: &str = "log/set-level";
    pub const PROXY_SUBSCRIBE: &str = "proxy:subscribe";

    // renderer<->main bridge tunnel
    pub const SERVICE_BRIDGE_INVOKE: &str = "service-bridge:invoke";
    pub const SERVICE_BRIDGE_PUSH: &str = "service-bridge:push";
}

/// HTTP endpoint paths per service. Several services multiplex plain HTTP and
/// mutual-TLS on one port ("splitlisten").
pub mod endpoints {
    // proxies (ollama-proxy / lmstudio-proxy)
    pub const OLLAMA_API_PREFIX: &str = "/api/"; // Ollama-native
    pub const OPENAI_V1_PREFIX: &str = "/v1/"; // OpenAI-compatible
    /// Peer request forwarding: body {host,port,path,name,data,txt,code}.
    pub const INGRESS: &str = "/ingress";
    pub const SET_PRIORITY: &str = "/set-priority";
    pub const NODE_ACTIVITY: &str = "/nodeactivity";

    // node-info
    pub const NODE_INFO: &str = "/v1/node-info";

    // cluster-manager (pairing / membership)
    pub const PAIRING: &str = "/pairing";
    pub const INVITE: &str = "/invite";
    pub const INVITE_STATUS: &str = "/invite_status";
    pub const INVITE_EXPIRY: &str = "/invite_expiry";
    pub const INVITE_PROVENANCE: &str = "/invite_provenance";
    pub const CLUSTER_PAIRING: &str = "/v1/cluster/pairing";
    pub const CLUSTER_ROSTER: &str = "/v1/cluster/roster";
    pub const CLUSTER_MEMBERS_REMOVE: &str = "/v1/cluster/members/remove";

    // shared clustertrust mesh (mTLS)
    pub const CLUSTERTRUST_MEMBERSHIP: &str = "/clustertrust/membership";
    pub const CLUSTERTRUST_MESH: &str = "/clustertrust/mesh";
    pub const CLUSTERTRUST_PEERCLIENT: &str = "/clustertrust/peerclient";
    pub const CLUSTERTRUST_WATCH: &str = "/clustertrust/watch";

    // engine-manager control
    pub const V1_ENGINES: &str = "/v1/engines";
    pub const MODELOPS: &str = "/modelops";
    pub const REMOTEPEERS: &str = "/remotepeers";

    // workload-manager / errors
    pub const WORKLOAD_EVENTS: &str = "/v1/workloads/events"; // SSE
    pub const PEERSYNC: &str = "/peersync";
}

/// mDNS TXT keys carried in the `_nvpair-node._tcp` service record.
///
/// Captured live from a reference advertisement: `v=1`, `uuid=<node-uuid>`,
/// `ip=<addr>`. The SRV record's port is the node-info port (where
/// `GET /v1/node-info` is served), not the cluster ingress port. `cluster-uuid`
/// is expected only once the node has joined a cluster.
pub mod txt_keys {
    /// TXT record version (observed `v=1`).
    pub const VERSION: &str = "v";
    /// Node UUID (observed key is `uuid`, not `nodeUuid`).
    pub const NODE_UUID: &str = "uuid";
    /// Primary advertised IP.
    pub const IP: &str = "ip";
    /// Cluster UUID (byte-verified in ClusterUUIDFromTXT; present when clustered).
    pub const CLUSTER_UUID: &str = "cluster-uuid";
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spot_check() {
        assert_eq!(methods::NODE_SET_PRIORITY, "node/set-priority");
        assert_eq!(endpoints::INGRESS, "/ingress");
        assert_eq!(endpoints::NODE_INFO, "/v1/node-info");
        assert_eq!(txt_keys::NODE_UUID, "uuid");
        assert_eq!(txt_keys::VERSION, "v");
    }
}
