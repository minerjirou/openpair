//! End-to-end cluster pairing over real localhost HTTP: an inviter node grows
//! its cluster and a joiner node joins it, exercising the full two-exchange
//! EAP-NOOB flow, PairingInfo authentication, and trust pinning.

use pair_cluster::{serve_pairing, NodeProfile, Paired, PairingNode, TrustSink};
use pair_trust::Identity;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

/// A trust sink that records every pinned peer.
#[derive(Default)]
struct RecordingSink {
    pinned: Mutex<Vec<Paired>>,
    clustered: Mutex<bool>,
}

impl TrustSink for RecordingSink {
    fn pin_peer(&self, paired: &Paired) {
        self.pinned.lock().unwrap().push(paired.clone());
    }
    fn is_clustered(&self) -> bool {
        *self.clustered.lock().unwrap()
    }
}

async fn free_addr() -> SocketAddr {
    // Bind :0 to claim a free port, then drop the listener so serve_pairing can
    // rebind it.
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap()
}

struct TestNode {
    node: Arc<PairingNode>,
    sink: Arc<RecordingSink>,
    addr: SocketAddr,
    cert_der: Vec<u8>,
}

async fn spawn_node(name: &str, cluster_id: &str, friendly: &str) -> TestNode {
    let addr = free_addr().await;
    let identity = Arc::new(Identity::generate().unwrap());
    let cert_der = identity.cert_der.clone();
    let profile = NodeProfile {
        identity,
        name: name.to_string(),
        cluster_id: cluster_id.to_string(),
        cluster_friendly_name: friendly.to_string(),
        admission_epoch: 1,
        advertised_addr: addr.to_string(),
    };
    let sink = Arc::new(RecordingSink::default());
    let node = PairingNode::new(profile, sink.clone());
    let srv = node.clone();
    tokio::spawn(async move {
        let _ = serve_pairing(addr, srv, std::future::pending::<()>()).await;
    });
    // Give the listener a moment to come up.
    tokio::time::sleep(Duration::from_millis(50)).await;
    TestNode {
        node,
        sink,
        addr,
        cert_der,
    }
}

#[tokio::test]
async fn full_cluster_join_over_http() {
    let inviter = spawn_node("inviter", "cluster-xyz", "Living Room").await;
    let joiner = spawn_node("joiner", "", "").await;

    // Inviter drives the Initial Exchange to the joiner and mints a PIN.
    let (invite_id, pin) = inviter
        .node
        .create_invite(&joiner.addr.to_string())
        .await
        .expect("create invite");
    assert_eq!(pin.len(), 6);

    // The joiner now has a pending invite awaiting the PIN.
    let pending = joiner.node.pending_invites().await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].invite_id, invite_id);
    assert_eq!(pending[0].from_name, "inviter");
    assert_eq!(pending[0].cluster_friendly_name, "Living Room");
    assert_eq!(pending[0].inviter_addr, inviter.addr.to_string());

    // The user enters the PIN on the joiner, which drives the Completion Exchange.
    let joiner_paired = joiner
        .node
        .submit_pin(&invite_id, &pin)
        .await
        .expect("submit pin");

    // Both sides pinned each other with an identical association key Kz.
    tokio::time::sleep(Duration::from_millis(50)).await; // let the inviter's ack settle
    let inviter_pins = inviter.sink.pinned.lock().unwrap();
    let joiner_pins = joiner.sink.pinned.lock().unwrap();
    assert_eq!(inviter_pins.len(), 1, "inviter pinned the joiner");
    assert_eq!(joiner_pins.len(), 1, "joiner pinned the inviter");

    let inviter_view = &inviter_pins[0]; // the joiner, as the inviter sees it
    let joiner_view = &joiner_pins[0]; // the inviter, as the joiner sees it
    assert_eq!(inviter_view.kz, joiner_view.kz, "shared Kz agrees");
    assert_eq!(joiner_paired.kz, joiner_view.kz);

    // Each side authenticated the other's real identity + certificate.
    assert_eq!(joiner_view.peer.name, "inviter");
    assert_eq!(joiner_view.peer.cluster_id, "cluster-xyz");
    assert_eq!(joiner_view.peer_cert_der, inviter.cert_der);
    assert_eq!(inviter_view.peer.name, "joiner");
    assert_eq!(inviter_view.peer_cert_der, joiner.cert_der);
}

#[tokio::test]
async fn wrong_pin_fails_and_pins_nothing() {
    let inviter = spawn_node("inviter", "cluster-xyz", "Den").await;
    let joiner = spawn_node("joiner", "", "").await;

    let (invite_id, pin) = inviter
        .node
        .create_invite(&joiner.addr.to_string())
        .await
        .expect("create invite");

    // Deliberately submit a different PIN.
    let wrong: String = {
        let n: u32 = pin.parse().unwrap();
        format!("{:06}", (n + 1) % 1_000_000)
    };
    let err = joiner
        .node
        .submit_pin(&invite_id, &wrong)
        .await
        .expect_err("wrong pin must fail");
    assert!(err.to_string().contains("incorrect pin"), "got: {err}");

    assert!(joiner.sink.pinned.lock().unwrap().is_empty());
    assert!(inviter.sink.pinned.lock().unwrap().is_empty());
}
