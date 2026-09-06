//! `PairingInfo` -- the identity object each side embeds in EAP-NOOB
//! ServerInfo (inviter) / PeerInfo (joiner) (§7.2).
//!
//! It binds the node's certificate and cluster identity into the exchange: the
//! object is folded verbatim into the Completion MACs, so a peer that alters it
//! fails MAC verification. On receipt the embedded cert's principal MUST equal
//! `nodeUuid`, or the PairingInfo is rejected (a cert cannot be presented under a
//! UUID it does not certify).
//!
//! Schema is identical in both directions and matches the upstream wire form
//! byte-for-byte in field names; because the confirmation MAC captures whatever
//! bytes actually crossed the wire, each side need only be self-consistent.

use serde::{Deserialize, Serialize};

/// Current PairingInfo schema version. v>=2 carries a mandatory non-zero
/// `admissionEpoch`; v1 peers omitted it and map to admission epoch 1.
pub const PAIRING_INFO_VERSION: u32 = 2;

/// Deterministic admission incarnation assigned to v1 / pre-admission peers.
pub const LEGACY_ADMISSION_EPOCH: u64 = 1;

/// The identity object embedded in EAP-NOOB ServerInfo/PeerInfo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingInfo {
    #[serde(rename = "v")]
    pub v: u32,
    #[serde(rename = "nodeUuid")]
    pub node_uuid: String,
    #[serde(rename = "nodeId")]
    pub node_id: String,
    pub name: String,
    #[serde(rename = "clusterId")]
    pub cluster_id: String,
    #[serde(
        rename = "admissionEpoch",
        default,
        skip_serializing_if = "is_zero_u64"
    )]
    pub admission_epoch: u64,
    #[serde(rename = "clusterFriendlyName")]
    pub cluster_friendly_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub addr: String,
    /// PEM-encoded X.509 certificate (the node's mTLS identity).
    pub cert: String,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

impl PairingInfo {
    /// Serialize to the compact JSON string put into ServerInfo/PeerInfo. The
    /// exact bytes returned here are what the EAP-NOOB machine folds into the
    /// MAC, so the caller must pass this same string to
    /// [`pair_pairing::Server::with_server_info`] /
    /// [`pair_pairing::Peer::with_peer_info`].
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("PairingInfo serializes")
    }
}

/// Parse and validate a peer's PairingInfo from the authenticated EAP-NOOB
/// transcript: decode the JSON, extract the embedded certificate, and require
/// its principal (node URN / CN) to equal `nodeUuid`. Returns the info plus the
/// certificate DER for pinning. v1 peers with no admission epoch are normalized
/// to [`LEGACY_ADMISSION_EPOCH`].
pub fn parse_pairing_info(raw: &[u8]) -> anyhow::Result<(PairingInfo, Vec<u8>)> {
    let mut pi: PairingInfo =
        serde_json::from_slice(raw).map_err(|e| anyhow::anyhow!("decode PairingInfo: {e}"))?;
    anyhow::ensure!(!pi.node_uuid.is_empty(), "PairingInfo missing nodeUuid");
    let der = pair_trust::identity::pem_cert_to_der(&pi.cert)
        .ok_or_else(|| anyhow::anyhow!("PairingInfo has no CERTIFICATE block"))?;
    let principal = pair_trust::node_uuid_from_cert(&der)
        .ok_or_else(|| anyhow::anyhow!("PairingInfo cert has no node UUID"))?;
    anyhow::ensure!(
        principal == pi.node_uuid,
        "PairingInfo cert principal {principal:?} != nodeUuid {:?}",
        pi.node_uuid
    );
    match () {
        _ if pi.v >= PAIRING_INFO_VERSION && pi.admission_epoch == 0 => {
            anyhow::bail!("PairingInfo v{} missing admissionEpoch", pi.v)
        }
        _ if pi.v < PAIRING_INFO_VERSION && pi.admission_epoch == 0 => {
            pi.admission_epoch = LEGACY_ADMISSION_EPOCH;
        }
        _ => {}
    }
    Ok((pi, der))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pair_trust::Identity;

    fn info_for(id: &Identity, addr: &str) -> PairingInfo {
        PairingInfo {
            v: PAIRING_INFO_VERSION,
            node_uuid: id.node_uuid.clone(),
            node_id: id.fingerprint(),
            name: "test-node".into(),
            cluster_id: "cluster-abc".into(),
            admission_epoch: 1,
            cluster_friendly_name: "Test Cluster".into(),
            addr: addr.into(),
            cert: id.cert_pem.clone(),
        }
    }

    #[test]
    fn roundtrip_and_principal_binding() {
        let id = Identity::generate().unwrap();
        let pi = info_for(&id, "10.0.0.5:14321");
        let json = pi.to_json();
        let (parsed, der) = parse_pairing_info(json.as_bytes()).unwrap();
        assert_eq!(parsed.node_uuid, id.node_uuid);
        assert_eq!(parsed.addr, "10.0.0.5:14321");
        assert_eq!(der, id.cert_der);
    }

    #[test]
    fn rejects_cert_uuid_mismatch() {
        let id = Identity::generate().unwrap();
        let other = Identity::generate().unwrap();
        let mut pi = info_for(&id, "");
        // Present someone else's cert under our nodeUuid: must be rejected.
        pi.cert = other.cert_pem.clone();
        let err = parse_pairing_info(pi.to_json().as_bytes()).unwrap_err();
        assert!(err.to_string().contains("principal"));
    }

    #[test]
    fn rejects_v2_without_admission_epoch() {
        let id = Identity::generate().unwrap();
        let mut pi = info_for(&id, "");
        pi.admission_epoch = 0;
        let err = parse_pairing_info(pi.to_json().as_bytes()).unwrap_err();
        assert!(err.to_string().contains("admissionEpoch"));
    }
}
