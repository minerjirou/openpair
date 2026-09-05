//! Transport-agnostic wrappers around the EAP-NOOB [`Server`]/[`Peer`] that
//! capture the peer's authenticated [`PairingInfo`] + certificate at the right
//! moments, so a driver only has to shuttle bytes.
//!
//! Role mapping for cluster pairing (§7.2):
//! * inviter = EAP-NOOB **Server** (server-to-peer OOB, `Dirs=2`) -> [`InviterSession`]
//! * joiner  = EAP-NOOB **Peer** (`PreferDir=2`) -> [`JoinerSession`]
//!
//! The Initial Exchange (Types 1-4) is driven by the inviter; the Completion
//! Exchange (Types 1,5,6) is driven by the joiner after the human PIN step.

use crate::info::{parse_pairing_info, PairingInfo};
use pair_pairing::machine::is_wrong_pin_code;
use pair_pairing::{noob_from_pin, Association, Outcome, Peer, Server, State};

/// The other side's authenticated identity, captured from the EAP-NOOB
/// transcript once its PairingInfo is available and validated.
#[derive(Debug, Clone)]
pub struct PeerIdentity {
    pub info: PairingInfo,
    pub cert_der: Vec<u8>,
}

/// One step of a completion drive.
#[derive(Debug, Default)]
pub struct Step {
    /// Bytes to send to the other side, if any.
    pub send: Option<Vec<u8>>,
    /// The exchange reached a terminal state.
    pub done: bool,
    /// Terminal state was success (Registered).
    pub success: bool,
    /// The failure was an EAP-NOOB NoobId / MAC mismatch -- i.e. a wrong PIN,
    /// classified by the protocol error code (2003 / 4001), as distinct from a
    /// transport/protocol error.
    pub auth_failed: bool,
    /// EAP-NOOB error code (RFC 9140 §3.6.4) when this is a protocol failure.
    pub error_code: Option<i64>,
    /// Human-readable error, when `done && !success`.
    pub error: Option<String>,
}

impl Step {
    fn from_outcome(o: Outcome) -> Self {
        let auth_failed = o.error_code.map(is_wrong_pin_code).unwrap_or(false);
        Step {
            send: o.send,
            done: o.done,
            success: o.success,
            auth_failed,
            error_code: o.error_code,
            error: o.error,
        }
    }
}

// --- joiner (Peer) ---------------------------------------------------------

/// The joiner side of a pairing: an EAP-NOOB Peer plus the inviter identity it
/// authenticates. Created on the first Initial-Exchange message; kept alive
/// across the human PIN step and the joiner-driven Completion Exchange.
pub struct JoinerSession {
    peer: Peer,
    inviter: Option<PeerIdentity>,
}

impl JoinerSession {
    /// Build a joiner carrying `peer_info` (this node's PairingInfo) as PeerInfo.
    pub fn new(peer_info: &PairingInfo) -> Self {
        Self {
            peer: Peer::new().with_peer_info(peer_info.to_json()),
            inviter: None,
        }
    }

    pub fn state(&self) -> State {
        self.peer.state()
    }

    /// The authenticated inviter identity, available once the Initial Exchange
    /// reaches Waiting.
    pub fn inviter(&self) -> Option<&PeerIdentity> {
        self.inviter.as_ref()
    }

    /// The inviter's reachable `host:port` for the joiner-driven Completion
    /// Exchange (from its authenticated PairingInfo).
    pub fn inviter_addr(&self) -> Option<&str> {
        self.inviter.as_ref().map(|i| i.info.addr.as_str())
    }

    /// Process one Initial-Exchange message from the inviter and return the
    /// reply bytes. When the Peer reaches Waiting the inviter's PairingInfo is
    /// parsed, validated, and captured.
    pub fn on_initial(&mut self, blob: &[u8]) -> anyhow::Result<Vec<u8>> {
        let out = self.peer.receive(blob);
        if let Some(e) = &out.error {
            anyhow::bail!("eap-noob: {e}");
        }
        if self.peer.state() == State::Waiting && self.inviter.is_none() {
            let raw = self.peer.server_info();
            anyhow::ensure!(!raw.is_empty(), "inviter sent no ServerInfo");
            let (info, cert_der) = parse_pairing_info(raw.as_bytes())?;
            self.inviter = Some(PeerIdentity { info, cert_der });
        }
        Ok(out.send.unwrap_or_default())
    }

    /// Feed the human PIN, deriving the OOB Noob (server-to-peer). Requires the
    /// Peer to be at Waiting.
    pub fn feed_pin(&mut self, pin: &str) -> anyhow::Result<()> {
        self.peer.oob_input_noob(&noob_from_pin(pin))
    }

    /// Process one Completion-Exchange response from the inviter.
    pub fn on_completion(&mut self, resp: &[u8]) -> Step {
        Step::from_outcome(self.peer.receive(resp))
    }

    /// The completed association (Registered), for pinning.
    pub fn association(&self) -> Option<&Association> {
        self.peer.association()
    }
}

// --- inviter (Server) ------------------------------------------------------

/// The inviter side of a pairing: an EAP-NOOB Server plus the joiner identity it
/// authenticates. Drives the Initial Exchange, mints a PIN, and serves the
/// joiner-driven Completion Exchange.
pub struct InviterSession {
    server: Server,
    joiner: Option<PeerIdentity>,
}

impl InviterSession {
    /// Build an inviter carrying `server_info` (this node's PairingInfo) as
    /// ServerInfo; `with_server_info` fixes server-to-peer OOB (`Dirs=2`).
    pub fn new(server_info: &PairingInfo) -> Self {
        Self {
            server: Server::new().with_server_info(server_info.to_json()),
            joiner: None,
        }
    }

    pub fn state(&self) -> State {
        self.server.state()
    }

    /// The authenticated joiner identity, available once Registered.
    pub fn joiner(&self) -> Option<&PeerIdentity> {
        self.joiner.as_ref()
    }

    /// Begin an EAP conversation: the Type 1 Discovery request. Used to start
    /// both the Initial Exchange and (again) the Completion Exchange.
    pub fn start(&mut self) -> Vec<u8> {
        self.server.start()
    }

    /// Process one Initial-Exchange response from the joiner. Terminates
    /// (`done`) when the Server reaches Waiting.
    pub fn on_initial(&mut self, resp: &[u8]) -> Step {
        Step::from_outcome(self.server.receive(resp))
    }

    /// Mint the OOB Noob from the displayed PIN (server-to-peer). Requires the
    /// Server to be at Waiting.
    pub fn set_pin(&mut self, pin: &str) -> anyhow::Result<()> {
        self.server.oob_output_with(&noob_from_pin(pin))
    }

    /// Process one Completion-Exchange message from the joiner. When the Server
    /// reaches Registered the joiner's PairingInfo is captured for pinning.
    pub fn on_completion(&mut self, blob: &[u8]) -> anyhow::Result<Step> {
        let step = Step::from_outcome(self.server.receive(blob));
        if self.server.state() == State::Registered && self.joiner.is_none() {
            let raw = self.server.peer_info();
            anyhow::ensure!(!raw.is_empty(), "joiner sent no PeerInfo");
            let (info, cert_der) = parse_pairing_info(raw.as_bytes())?;
            self.joiner = Some(PeerIdentity { info, cert_der });
        }
        Ok(step)
    }

    /// The completed association (Registered), for pinning.
    pub fn association(&self) -> Option<&Association> {
        self.server.association()
    }
}
