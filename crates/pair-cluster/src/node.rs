//! [`PairingNode`]: the stateful endpoint that joins/forms clusters over the
//! plain-HTTP `/v1/cluster/pairing` channel.
//!
//! It plays both EAP-NOOB roles: as **joiner** it answers inbound Initial
//! Exchanges (serving [`handle_request`]) and, once the user supplies the PIN,
//! drives the Completion Exchange to the inviter ([`PairingNode::submit_pin`]);
//! as **inviter** it drives an Initial Exchange to a joiner
//! ([`PairingNode::create_invite`]), mints a PIN, and serves the joiner-driven
//! Completion. On success a freshly-paired peer's certificate is handed to the
//! [`TrustSink`] for pinning.

use crate::session::{InviterSession, JoinerSession};
use crate::wire::{phase, reason, PairingEnvelope, DEFAULT_PAIRING_PORT, PAIRING_PATH};
use crate::PairingInfo;
use pair_pairing::State;
use pair_trust::Identity;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// This node's identity + cluster context, from which a [`PairingInfo`] is
/// built for each exchange.
#[derive(Clone)]
pub struct NodeProfile {
    pub identity: Arc<Identity>,
    pub name: String,
    pub cluster_id: String,
    pub cluster_friendly_name: String,
    pub admission_epoch: u64,
    /// Reachable `host:port` this node serves the pairing channel on. Required
    /// on an inviter's ServerInfo (the joiner drives Completion here); advisory
    /// on a joiner's PeerInfo.
    pub advertised_addr: String,
}

impl NodeProfile {
    /// Build this node's PairingInfo. `addr` overrides [`Self::advertised_addr`]
    /// when the reachable address is known only per-exchange.
    pub fn pairing_info(&self, addr: Option<&str>) -> PairingInfo {
        PairingInfo {
            v: crate::info::PAIRING_INFO_VERSION,
            node_uuid: self.identity.node_uuid.clone(),
            node_id: self.identity.fingerprint(),
            name: self.name.clone(),
            cluster_id: self.cluster_id.clone(),
            admission_epoch: self.admission_epoch.max(1),
            cluster_friendly_name: self.cluster_friendly_name.clone(),
            addr: addr.unwrap_or(&self.advertised_addr).to_string(),
            cert: self.identity.cert_pem.clone(),
        }
    }
}

/// A successfully completed pairing: the peer's authenticated identity,
/// certificate, and the derived association key `Kz`.
#[derive(Debug, Clone)]
pub struct Paired {
    pub peer: PairingInfo,
    pub peer_cert_der: Vec<u8>,
    pub kz: [u8; 32],
    pub cryptosuitep: u8,
}

/// Where a freshly-paired peer is pinned, and the gate on accepting new joins.
pub trait TrustSink: Send + Sync + 'static {
    /// Pin a peer's certificate + identity + association key after a successful
    /// pairing.
    fn pin_peer(&self, paired: &Paired);
    /// Whether this node is already a cluster member (a joiner must refuse a new
    /// pairing while clustered).
    fn is_clustered(&self) -> bool {
        false
    }
}

/// A pending inbound invite awaiting the local user's PIN (joiner side).
#[derive(Debug, Clone)]
pub struct PendingInvite {
    pub invite_id: String,
    pub from_node_uuid: String,
    pub from_name: String,
    pub cluster_id: String,
    pub cluster_friendly_name: String,
    pub inviter_addr: String,
}

/// HTTP-agnostic result of handling one pairing request.
pub struct Reply {
    pub status: u16,
    pub envelope: PairingEnvelope,
}

impl Reply {
    fn ok(envelope: PairingEnvelope) -> Self {
        Self {
            status: 200,
            envelope,
        }
    }
    fn conflict(envelope: PairingEnvelope) -> Self {
        Self {
            status: 409,
            envelope,
        }
    }
}

/// The stateful pairing endpoint. Wrap in an `Arc` and share across the HTTP
/// server and the invite/PIN drivers.
pub struct PairingNode {
    profile: NodeProfile,
    trust: Arc<dyn TrustSink>,
    joiners: Mutex<HashMap<String, Arc<Mutex<JoinerSession>>>>,
    inviters: Mutex<HashMap<String, Arc<Mutex<InviterSession>>>>,
    pending: Mutex<HashMap<String, PendingInvite>>,
}

impl PairingNode {
    pub fn new(profile: NodeProfile, trust: Arc<dyn TrustSink>) -> Arc<Self> {
        Arc::new(Self {
            profile,
            trust,
            joiners: Mutex::new(HashMap::new()),
            inviters: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        })
    }

    /// Invites awaiting a local PIN response.
    pub async fn pending_invites(&self) -> Vec<PendingInvite> {
        self.pending.lock().await.values().cloned().collect()
    }

    // --- server side: dispatch one inbound pairing request -----------------

    /// Handle one decoded pairing request. Transport-agnostic so it can be
    /// served over any HTTP stack (see [`crate::serve_pairing`]).
    pub async fn handle_request(self: &Arc<Self>, env: PairingEnvelope) -> Reply {
        if env.invite_id.is_empty() {
            return Reply {
                status: 400,
                envelope: PairingEnvelope::signal("", env.phase.as_str(), "missing inviteId"),
            };
        }
        match env.phase.as_str() {
            phase::INITIAL => self.on_initial_request(env).await,
            phase::COMPLETION => self.on_completion_request(env).await,
            // Terminal / liveness signals: best-effort teardown, always 200.
            phase::CANCEL | phase::DECLINE | phase::FAIL | phase::EXPIRED | phase::ACK => {
                self.on_signal(&env).await;
                Reply::ok(PairingEnvelope::default())
            }
            other => Reply {
                status: 400,
                envelope: PairingEnvelope::signal(&env.invite_id, other, "unknown phase"),
            },
        }
    }

    /// Joiner side of the Initial Exchange.
    async fn on_initial_request(self: &Arc<Self>, env: PairingEnvelope) -> Reply {
        let msg = env.blob();
        let session = {
            let mut joiners = self.joiners.lock().await;
            if let Some(s) = joiners.get(&env.invite_id) {
                s.clone()
            } else {
                // First message must be Type 1 (Discovery).
                if first_type(&msg) != Some(1) {
                    return Reply {
                        status: 400,
                        envelope: PairingEnvelope::signal(
                            &env.invite_id,
                            phase::INITIAL,
                            "invalid EAP-NOOB message",
                        ),
                    };
                }
                if self.trust.is_clustered() {
                    return Reply::conflict(PairingEnvelope::rejected(reason::ALREADY_CLUSTERED));
                }
                let info = self.profile.pairing_info(None);
                let s = Arc::new(Mutex::new(JoinerSession::new(&info)));
                joiners.insert(env.invite_id.clone(), s.clone());
                s
            }
        };

        let (reply, pending) = {
            let mut s = session.lock().await;
            match s.on_initial(&msg) {
                Ok(bytes) => {
                    let pending = if s.state() == State::Waiting {
                        s.inviter().map(|inv| PendingInvite {
                            invite_id: env.invite_id.clone(),
                            from_node_uuid: inv.info.node_uuid.clone(),
                            from_name: inv.info.name.clone(),
                            cluster_id: inv.info.cluster_id.clone(),
                            cluster_friendly_name: inv.info.cluster_friendly_name.clone(),
                            inviter_addr: inv.info.addr.clone(),
                        })
                    } else {
                        None
                    };
                    (
                        Reply::ok(PairingEnvelope::with_msg(
                            &env.invite_id,
                            phase::INITIAL,
                            &bytes,
                        )),
                        pending,
                    )
                }
                Err(e) => {
                    self.joiners.lock().await.remove(&env.invite_id);
                    (
                        Reply {
                            status: 400,
                            envelope: PairingEnvelope::signal(
                                &env.invite_id,
                                phase::INITIAL,
                                &e.to_string(),
                            ),
                        },
                        None,
                    )
                }
            }
        };
        if let Some(p) = pending {
            self.pending.lock().await.insert(p.invite_id.clone(), p);
        }
        reply
    }

    /// Inviter side of the joiner-driven Completion Exchange.
    async fn on_completion_request(self: &Arc<Self>, env: PairingEnvelope) -> Reply {
        let session = match self.inviters.lock().await.get(&env.invite_id) {
            Some(s) => s.clone(),
            None => {
                return Reply::conflict(PairingEnvelope::signal(
                    &env.invite_id,
                    phase::COMPLETION,
                    "unknown or expired inviteId",
                ));
            }
        };
        let blob = env.blob();
        let (reply, paired) = {
            let mut s = session.lock().await;
            if blob.is_empty() {
                // Kickoff: emit the Server's first Completion message.
                let out = s.start();
                (
                    Reply::ok(PairingEnvelope::with_msg(
                        &env.invite_id,
                        phase::COMPLETION,
                        &out,
                    )),
                    None,
                )
            } else {
                match s.on_completion(&blob) {
                    Ok(step) => {
                        let mut paired = None;
                        if step.done && step.success {
                            if let (Some(peer), Some(assoc)) = (s.joiner(), s.association()) {
                                paired = Some(Paired {
                                    peer: peer.info.clone(),
                                    peer_cert_der: peer.cert_der.clone(),
                                    kz: assoc.kz,
                                    cryptosuitep: assoc.cryptosuitep,
                                });
                            }
                        }
                        let send = step.send.unwrap_or_default();
                        (
                            Reply::ok(PairingEnvelope::with_msg(
                                &env.invite_id,
                                phase::COMPLETION,
                                &send,
                            )),
                            paired,
                        )
                    }
                    Err(e) => (
                        Reply {
                            status: 400,
                            envelope: PairingEnvelope::signal(
                                &env.invite_id,
                                phase::COMPLETION,
                                &e.to_string(),
                            ),
                        },
                        None,
                    ),
                }
            }
        };
        if let Some(p) = paired {
            self.trust.pin_peer(&p);
        }
        reply
    }

    async fn on_signal(self: &Arc<Self>, env: &PairingEnvelope) {
        // A cancel clears a pending inbound invite; other signals are inviter-side
        // teardown. Both are idempotent best-effort.
        match env.phase.as_str() {
            phase::CANCEL => {
                self.joiners.lock().await.remove(&env.invite_id);
                self.pending.lock().await.remove(&env.invite_id);
            }
            phase::DECLINE | phase::FAIL | phase::EXPIRED => {
                self.inviters.lock().await.remove(&env.invite_id);
            }
            _ => {}
        }
    }

    // --- client side: drive an exchange ------------------------------------

    /// Inviter: drive the Initial Exchange to `joiner_addr`, mint a PIN, and
    /// leave an [`InviterSession`] awaiting the joiner-driven Completion. Returns
    /// the `(inviteId, pin)` to display to the human.
    pub async fn create_invite(
        self: &Arc<Self>,
        joiner_addr: &str,
    ) -> anyhow::Result<(String, String)> {
        let invite_id = uuid::Uuid::new_v4().to_string();
        let info = self.profile.pairing_info(None);
        let mut session = InviterSession::new(&info);

        // Drive Initial as HTTP client until the Server reaches Waiting.
        let mut msg = session.start();
        loop {
            let env = PairingEnvelope::with_msg(&invite_id, phase::INITIAL, &msg);
            let resp = crate::http::post_pairing(joiner_addr, &env).await?;
            if resp.rejected {
                anyhow::bail!("joiner rejected pairing: {}", resp.reason);
            }
            let step = session.on_initial(&resp.blob());
            if let Some(e) = &step.error {
                anyhow::bail!("initial exchange failed: {e}");
            }
            if step.done {
                break;
            }
            msg = step
                .send
                .ok_or_else(|| anyhow::anyhow!("inviter stalled in Initial"))?;
        }
        anyhow::ensure!(
            session.state() == State::Waiting,
            "Initial ended in state {:?}, want Waiting",
            session.state()
        );

        let pin = mint_pin();
        session.set_pin(&pin)?;
        self.inviters
            .lock()
            .await
            .insert(invite_id.clone(), Arc::new(Mutex::new(session)));
        Ok((invite_id, pin))
    }

    /// Joiner: feed the PIN and drive the Completion Exchange to the inviter.
    /// On success the inviter is pinned and [`Paired`] is returned.
    pub async fn submit_pin(
        self: &Arc<Self>,
        invite_id: &str,
        pin: &str,
    ) -> anyhow::Result<Paired> {
        if !pair_pairing::is_valid_pin(pin) {
            anyhow::bail!("pin must be six digits");
        }
        let session = self
            .joiners
            .lock()
            .await
            .get(invite_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("unknown or evicted inviteId"))?;

        let inviter_addr = {
            let mut s = session.lock().await;
            let addr = s
                .inviter_addr()
                .filter(|a| !a.is_empty())
                .map(|a| a.to_string())
                .ok_or_else(|| anyhow::anyhow!("inviter address unknown"))?;
            s.feed_pin(pin)?;
            addr
        };

        // Kickoff: empty msg so the inviter's Server.Start() emits the first blob.
        let mut resp = crate::http::post_pairing(
            &inviter_addr,
            &PairingEnvelope::signal(invite_id, phase::COMPLETION, ""),
        )
        .await?;
        let paired = loop {
            let step = {
                let mut s = session.lock().await;
                s.on_completion(&resp.blob())
            };
            if step.done {
                if step.success {
                    let s = session.lock().await;
                    let inv = s
                        .inviter()
                        .ok_or_else(|| anyhow::anyhow!("no inviter identity captured"))?;
                    let assoc = s
                        .association()
                        .ok_or_else(|| anyhow::anyhow!("no association after success"))?;
                    break Paired {
                        peer: inv.info.clone(),
                        peer_cert_der: inv.cert_der.clone(),
                        kz: assoc.kz,
                        cryptosuitep: assoc.cryptosuitep,
                    };
                }
                // Mirror the failure to the inviter so it tears down immediately;
                // only a definitive wrong-PIN carries reason:"incorrect-pin".
                let reason_out = if step.auth_failed {
                    reason::INCORRECT_PIN
                } else {
                    ""
                };
                let _ = crate::http::post_pairing(
                    &inviter_addr,
                    &PairingEnvelope::signal(invite_id, phase::FAIL, reason_out),
                )
                .await;
                self.joiners.lock().await.remove(invite_id);
                self.pending.lock().await.remove(invite_id);
                anyhow::bail!(
                    "completion failed{}: {}",
                    if step.auth_failed {
                        " (incorrect pin)"
                    } else {
                        ""
                    },
                    step.error.unwrap_or_default()
                );
            }
            let send = step
                .send
                .ok_or_else(|| anyhow::anyhow!("joiner stalled in Completion"))?;
            resp = crate::http::post_pairing(
                &inviter_addr,
                &PairingEnvelope::with_msg(invite_id, phase::COMPLETION, &send),
            )
            .await?;
        };

        self.trust.pin_peer(&paired);
        self.joiners.lock().await.remove(invite_id);
        self.pending.lock().await.remove(invite_id);
        // Acknowledge the inviter's durable commit (best-effort).
        let _ = crate::http::post_pairing(
            &inviter_addr,
            &PairingEnvelope::signal(invite_id, phase::ACK, ""),
        )
        .await;
        Ok(paired)
    }
}

/// The first `Type` field of an EAP-NOOB blob, if any.
fn first_type(blob: &[u8]) -> Option<i64> {
    #[derive(serde::Deserialize)]
    struct T {
        #[serde(rename = "Type")]
        type_: Option<i64>,
    }
    serde_json::from_slice::<T>(blob).ok().and_then(|t| t.type_)
}

/// A fresh random six-digit PIN.
fn mint_pin() -> String {
    use rand::Rng;
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000))
}

/// The default `host:port` for a bare invite host (upstream appends 14321).
pub fn default_pairing_addr(host: &str) -> String {
    format!("{host}:{DEFAULT_PAIRING_PORT}")
}

/// The pairing channel path, re-exported for HTTP servers.
pub const PATH: &str = PAIRING_PATH;
