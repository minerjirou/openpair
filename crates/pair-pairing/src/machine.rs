//! EAP-NOOB Server + Peer state machines (RFC 9140, Types 1-6), a clean-room
//! Rust port of the transport-agnostic upstream library (Apache-2.0).
//!
//! Both roles consume/produce EAP-NOOB message bytes (compact JSON) and are
//! driven by relaying each side's outbound bytes to the other. MAC-relevant
//! fields are captured **verbatim** (via `RawValue`) so the confirmation MACs
//! match a real peer byte-for-byte. Reconnect (Types 7-9) is reserved upstream
//! and not modelled.

use crate::kdf::DerivedKeys;
use crate::mac::{compute_hoob, compute_mac, mac_equal, MacInputs};
use crate::suite::{Jwk, KeyPair, Suite};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// EAP-NOOB error notification codes (RFC 9140 §3.6.4), byte-confirmed against
/// the upstream implementation. Only the codes this port emits/classifies are
/// listed.
pub mod error_code {
    pub const INVALID_DATA: i64 = 1003;
    pub const UNEXPECTED_PEER_ID: i64 = 2001;
    pub const STATE_MISMATCH: i64 = 2002;
    /// Invalid ECDHE key / unrecognized NoobId -- the wrong-PIN signature.
    pub const UNRECOGNIZED_OOB_MSG_ID: i64 = 2003;
    pub const UNSUPPORTED_VERSION: i64 = 3001;
    pub const UNSUPPORTED_CRYPTOSUITE: i64 = 3002;
    pub const NO_MUTUAL_OOB: i64 = 3003;
    /// Completion MAC verification failed -- also a wrong-PIN signature.
    pub const HMAC_VERIFICATION_FAILED: i64 = 4001;
}

/// Whether an error code is the signature of a wrong PIN (a different OOB Noob
/// yields a NoobId the peer cannot recognize, or -- if it collided -- a MAC that
/// will not verify).
pub fn is_wrong_pin_code(code: i64) -> bool {
    code == error_code::UNRECOGNIZED_OOB_MSG_ID || code == error_code::HMAC_VERIFICATION_FAILED
}

/// EAP-NOOB association state (RFC 9140, Figure 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Unregistered = 0,
    Waiting = 1,
    OobReceived = 2,
    Reconnecting = 3,
    Registered = 4,
}

/// Result of processing one inbound message.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub send: Option<Vec<u8>>,
    pub done: bool,
    pub success: bool,
    pub error: Option<String>,
    /// EAP-NOOB error code (RFC 9140 §3.6.4) when this outcome is a protocol
    /// failure -- set both when this side fails and when it receives the peer's
    /// error notification. See [`error_code`] and [`is_wrong_pin_code`].
    pub error_code: Option<i64>,
}

/// A completed association: the shared key Kz plus identifying metadata.
#[derive(Debug, Clone)]
pub struct Association {
    pub peer_id: String,
    pub nai: String,
    pub cryptosuitep: u8,
    pub kz: [u8; 32],
}

pub const DEFAULT_NAI: &str = "noob@eap-noob.arpa";

// --- wire message ----------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
struct Wire {
    #[serde(rename = "Type", default, skip_serializing_if = "Option::is_none")]
    type_: Option<i64>,
    #[serde(rename = "PeerState", default, skip_serializing_if = "Option::is_none")]
    peer_state: Option<i64>,
    #[serde(rename = "PeerId", default, skip_serializing_if = "Option::is_none")]
    peer_id: Option<Box<RawValue>>,
    #[serde(rename = "NAI", default, skip_serializing_if = "Option::is_none")]
    nai: Option<Box<RawValue>>,
    #[serde(rename = "NewNAI", default, skip_serializing_if = "Option::is_none")]
    new_nai: Option<Box<RawValue>>,
    #[serde(rename = "Vers", default, skip_serializing_if = "Option::is_none")]
    vers: Option<Box<RawValue>>,
    #[serde(rename = "Verp", default, skip_serializing_if = "Option::is_none")]
    verp: Option<Box<RawValue>>,
    #[serde(
        rename = "Cryptosuites",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    cryptosuites: Option<Box<RawValue>>,
    #[serde(
        rename = "Cryptosuitep",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    cryptosuitep: Option<Box<RawValue>>,
    #[serde(rename = "Dirs", default, skip_serializing_if = "Option::is_none")]
    dirs: Option<Box<RawValue>>,
    #[serde(rename = "Dirp", default, skip_serializing_if = "Option::is_none")]
    dirp: Option<Box<RawValue>>,
    #[serde(
        rename = "ServerInfo",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    server_info: Option<Box<RawValue>>,
    #[serde(rename = "PeerInfo", default, skip_serializing_if = "Option::is_none")]
    peer_info: Option<Box<RawValue>>,
    #[serde(rename = "PKs", default, skip_serializing_if = "Option::is_none")]
    pks: Option<Box<RawValue>>,
    #[serde(rename = "PKp", default, skip_serializing_if = "Option::is_none")]
    pkp: Option<Box<RawValue>>,
    #[serde(rename = "Ns", default, skip_serializing_if = "Option::is_none")]
    ns: Option<Box<RawValue>>,
    #[serde(rename = "Np", default, skip_serializing_if = "Option::is_none")]
    np: Option<Box<RawValue>>,
    #[serde(rename = "SleepTime", default, skip_serializing_if = "Option::is_none")]
    sleep_time: Option<i64>,
    #[serde(rename = "NoobId", default, skip_serializing_if = "Option::is_none")]
    noob_id: Option<Box<RawValue>>,
    #[serde(rename = "MACs", default, skip_serializing_if = "Option::is_none")]
    macs: Option<Box<RawValue>>,
    #[serde(rename = "MACp", default, skip_serializing_if = "Option::is_none")]
    macp: Option<Box<RawValue>>,
    #[serde(rename = "ErrorCode", default, skip_serializing_if = "Option::is_none")]
    error_code: Option<i64>,
    #[serde(rename = "ErrorInfo", default, skip_serializing_if = "Option::is_none")]
    error_info: Option<String>,
    #[serde(rename = "eap", default, skip_serializing_if = "Option::is_none")]
    eap: Option<String>,
}

fn raw(s: String) -> Box<RawValue> {
    RawValue::from_string(s).expect("valid json fragment")
}
fn jstr(s: &str) -> String {
    serde_json::to_string(s).expect("string")
}
fn get(r: &Option<Box<RawValue>>) -> String {
    r.as_ref().map(|v| v.get().to_string()).unwrap_or_default()
}
fn parse_str(r: &Option<Box<RawValue>>) -> Option<String> {
    serde_json::from_str(r.as_ref()?.get()).ok()
}
fn parse_int(r: &Option<Box<RawValue>>) -> Option<i64> {
    serde_json::from_str(r.as_ref()?.get()).ok()
}
fn parse_ints(r: &Option<Box<RawValue>>) -> Option<Vec<i64>> {
    serde_json::from_str(r.as_ref()?.get()).ok()
}
fn b64(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(bytes)
}
fn unb64(s: &str) -> Option<Vec<u8>> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.decode(s).ok()
}

// Accumulated verbatim fields feeding the MAC/Hoob arrays.
#[derive(Default)]
struct Method {
    suite: Option<Suite>,
    keypair: Option<KeyPair>,
    peer_id: String,
    nai: String,
    z: Vec<u8>,
    ns: Vec<u8>,
    np: Vec<u8>,
    noob: Vec<u8>,
    noob_b64_json: String, // e.g. "\"Tm9v\""
    noob_id: [u8; 16],
    keym: Option<DerivedKeys>,
    inp: MacInputs,
}

impl Method {
    fn set_noob(&mut self, noob: &[u8]) {
        self.noob = noob.to_vec();
        self.noob_b64_json = jstr(&b64(noob));
        self.inp.noob = self.noob_b64_json.clone();
        self.noob_id = crate::mac::compute_noob_id(&self.noob_b64_json);
    }
    fn derive(&mut self) {
        self.keym = Some(crate::kdf::derive_completion(
            &self.z, &self.np, &self.ns, &self.noob,
        ));
    }
    fn hoob(&self, dir: i64) -> [u8; 16] {
        compute_hoob(dir, &self.inp)
    }
}

fn err_bytes(code: i64, info: &str) -> Vec<u8> {
    // RFC 9140 §3.6.4 error notification: Type=0, ErrorCode, ErrorInfo. `eap` is
    // reserved for the terminating success/failure result and must stay unset so
    // a real peer surfaces this as a ProtocolError carrying the code.
    serde_json::to_vec(&Wire {
        type_: Some(0),
        error_code: Some(code),
        error_info: Some(info.to_string()),
        ..Default::default()
    })
    .unwrap_or_default()
}
fn result_bytes(kind: &str) -> Vec<u8> {
    serde_json::to_vec(&Wire {
        eap: Some(kind.to_string()),
        ..Default::default()
    })
    .unwrap_or_default()
}

// --- Server ----------------------------------------------------------------

pub struct Server {
    versions: Vec<i64>,
    cryptosuites: Vec<i64>, // server priority, e.g. [2,1]
    dirs: i64,
    state: State,
    m: Method,
    peer_state: i64,
    csp: i64,
    dirp: i64,
    server_info: String,
    assoc: Option<Association>,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            versions: vec![1],
            cryptosuites: vec![2, 1],
            dirs: 3,
            state: State::Unregistered,
            m: Method::default(),
            peer_state: 0,
            csp: 0,
            dirp: 0,
            server_info: "{}".to_string(),
            assoc: None,
        }
    }
}

impl Server {
    pub fn new() -> Self {
        Self::default()
    }
    /// Carry a JSON ServerInfo object (e.g. this node's PairingInfo) that the
    /// peer authenticates via the Completion MAC. Dirs default to server-to-peer.
    pub fn with_server_info(mut self, info_json: String) -> Self {
        self.server_info = info_json;
        self.dirs = 2;
        self
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn association(&self) -> Option<&Association> {
        self.assoc.as_ref()
    }
    /// The peer's authenticated PeerInfo (verbatim JSON), valid once Registered.
    pub fn peer_info(&self) -> &str {
        &self.m.inp.peer_info
    }

    /// Start a new EAP conversation: Type 1 Discovery request.
    pub fn start(&mut self) -> Vec<u8> {
        serde_json::to_vec(&Wire {
            type_: Some(1),
            ..Default::default()
        })
        .unwrap_or_default()
    }

    pub fn receive(&mut self, input: &[u8]) -> Outcome {
        let wm: Wire = match serde_json::from_slice(input) {
            Ok(w) => w,
            Err(_) => return self.fail("malformed JSON"),
        };
        if let Some(code) = wm.error_code {
            return Outcome {
                done: true,
                error: Some(peer_error_msg(code, &wm.error_info)),
                error_code: Some(code),
                ..Default::default()
            };
        }
        match wm.type_ {
            Some(1) => self.on_discovery(&wm),
            Some(2) => self.on_negotiation(&wm),
            Some(3) => self.on_key_exchange(&wm),
            Some(4) => self.done_failure(),
            Some(5) => self.on_noob_id(&wm),
            Some(6) => self.on_completion(&wm),
            _ => self.fail("unexpected type"),
        }
    }

    fn on_discovery(&mut self, wm: &Wire) -> Outcome {
        self.peer_state = wm.peer_state.unwrap_or(0);
        if let Some(nai) = parse_str(&wm.nai) {
            self.m.nai = nai;
        }
        if self.m.nai.is_empty() {
            self.m.nai = DEFAULT_NAI.to_string();
        }
        let (ss, ps) = (self.state as i64, self.peer_state);
        if (ss == 2 && (ps == 1 || ps == 2)) || (ps == 2 && (ss == 1 || ss == 2)) {
            self.start_completion()
        } else if ss == 1 && ps == 1 {
            self.build_waiting()
        } else if ss == 0 || ps == 0 {
            self.build_negotiation()
        } else {
            self.fail("state mismatch")
        }
    }

    fn build_negotiation(&mut self) -> Outcome {
        let peer_id = new_peer_id();
        self.m.peer_id = peer_id.clone();
        let vers = serde_json::to_string(&self.versions).unwrap();
        let cs = serde_json::to_string(&self.cryptosuites).unwrap();
        let dirs = self.dirs.to_string();
        let server_info = self.server_info.clone();
        self.m.inp.vers = vers.clone();
        self.m.inp.peer_id = jstr(&peer_id);
        self.m.inp.cryptosuites = cs.clone();
        self.m.inp.dirs = dirs.clone();
        self.m.inp.server_info = server_info.clone();
        let w = Wire {
            type_: Some(2),
            vers: Some(raw(vers)),
            peer_id: Some(raw(jstr(&peer_id))),
            cryptosuites: Some(raw(cs)),
            dirs: Some(raw(dirs)),
            server_info: Some(raw(server_info)),
            ..Default::default()
        };
        self.send(w)
    }

    fn on_negotiation(&mut self, wm: &Wire) -> Outcome {
        if let Err(e) = self.check_peer_id(wm) {
            return self.fail(&e);
        }
        let verp = parse_int(&wm.verp).unwrap_or(-1);
        if !self.versions.contains(&verp) {
            return self.fail("unsupported version");
        }
        let csp = parse_int(&wm.cryptosuitep).unwrap_or(-1);
        if !self.cryptosuites.contains(&csp) || Suite::from_id(csp as u8).is_none() {
            return self.fail("unsupported cryptosuite");
        }
        let dirp = parse_int(&wm.dirp).unwrap_or(-1);
        if dirp & self.dirs == 0 || !(1..=3).contains(&dirp) {
            return self.fail("no mutual OOB direction");
        }
        self.m.suite = Suite::from_id(csp as u8);
        self.csp = csp;
        self.dirp = dirp;
        self.m.inp.verp = get(&wm.verp);
        self.m.inp.cryptosuitep = get(&wm.cryptosuitep);
        self.m.inp.dirp = get(&wm.dirp);
        self.m.inp.peer_info = ensure_obj(get(&wm.peer_info));
        self.m.inp.nai = jstr(&self.m.nai);
        self.build_key_exchange()
    }

    fn build_key_exchange(&mut self) -> Outcome {
        let suite = self.m.suite.unwrap();
        let kp = suite.generate();
        let pks = serde_json::to_string(&kp.public_jwk).unwrap();
        self.m.keypair = Some(kp);
        let ns = rand32();
        self.m.ns = ns.to_vec();
        let ns_json = jstr(&b64(&ns));
        self.m.inp.pks = pks.clone();
        self.m.inp.ns = ns_json.clone();
        let w = Wire {
            type_: Some(3),
            peer_id: Some(raw(jstr(&self.m.peer_id))),
            pks: Some(raw(pks)),
            ns: Some(raw(ns_json)),
            ..Default::default()
        };
        self.send(w)
    }

    fn on_key_exchange(&mut self, wm: &Wire) -> Outcome {
        if let Err(e) = self.check_peer_id(wm) {
            return self.fail(&e);
        }
        let jwk: Jwk = match serde_json::from_str(&get(&wm.pkp)) {
            Ok(j) => j,
            Err(_) => return self.fail("bad PKp"),
        };
        let z = match self.m.keypair.as_ref().unwrap().compute_z(&jwk) {
            Ok(z) => z,
            Err(_) => return self.fail("ECDHE failure"),
        };
        let np_b64 = match parse_str(&wm.np) {
            Some(s) => s,
            None => return self.fail("bad Np"),
        };
        self.m.np = match unb64(&np_b64) {
            Some(n) => n,
            None => return self.fail("bad Np encoding"),
        };
        self.m.z = z;
        self.m.inp.pkp = get(&wm.pkp);
        self.m.inp.np = get(&wm.np);
        self.state = State::Waiting;
        Outcome {
            send: Some(result_bytes("failure")),
            done: true,
            ..Default::default()
        }
    }

    fn build_waiting(&mut self) -> Outcome {
        let w = Wire {
            type_: Some(4),
            peer_id: Some(raw(jstr(&self.m.peer_id))),
            ..Default::default()
        };
        self.send(w)
    }

    fn start_completion(&mut self) -> Outcome {
        if self.m.noob.is_empty() {
            return self.fail("no OOB processed");
        }
        if self.peer_state == 2 {
            let w = Wire {
                type_: Some(5),
                peer_id: Some(raw(jstr(&self.m.peer_id))),
                ..Default::default()
            };
            self.send(w)
        } else {
            self.build_completion()
        }
    }

    fn on_noob_id(&mut self, wm: &Wire) -> Outcome {
        if let Err(e) = self.check_peer_id(wm) {
            return self.fail(&e);
        }
        let got = parse_str(&wm.noob_id)
            .and_then(|s| unb64(&s))
            .unwrap_or_default();
        if got != self.m.noob_id {
            return self.fail_code(error_code::UNRECOGNIZED_OOB_MSG_ID, "unrecognized NoobId");
        }
        self.build_completion()
    }

    fn build_completion(&mut self) -> Outcome {
        self.m.derive();
        let kms = self.m.keym.as_ref().unwrap().kms;
        let macs = compute_mac(&kms, 2, &self.m.inp);
        let w = Wire {
            type_: Some(6),
            peer_id: Some(raw(jstr(&self.m.peer_id))),
            noob_id: Some(raw(jstr(&b64(&self.m.noob_id)))),
            macs: Some(raw(jstr(&b64(&macs)))),
            ..Default::default()
        };
        self.send(w)
    }

    fn on_completion(&mut self, wm: &Wire) -> Outcome {
        if let Err(e) = self.check_peer_id(wm) {
            return self.fail(&e);
        }
        let macp = parse_str(&wm.macp)
            .and_then(|s| unb64(&s))
            .unwrap_or_default();
        let kmp = self.m.keym.as_ref().unwrap().kmp;
        let expected = compute_mac(&kmp, 1, &self.m.inp);
        if !mac_equal(&macp, &expected) {
            return self.fail_code(
                error_code::HMAC_VERIFICATION_FAILED,
                "MACp verification failed",
            );
        }
        self.assoc = Some(self.finish());
        self.state = State::Registered;
        Outcome {
            send: Some(result_bytes("success")),
            done: true,
            success: true,
            ..Default::default()
        }
    }

    fn finish(&self) -> Association {
        Association {
            peer_id: self.m.peer_id.clone(),
            nai: self.m.nai.clone(),
            cryptosuitep: self.csp as u8,
            kz: self.m.keym.as_ref().unwrap().kz,
        }
    }

    /// Server-to-peer OOB using a caller-supplied 16-byte Noob (PIN-derived).
    /// Requires Waiting state and a negotiated direction including server-to-peer.
    pub fn oob_output_with(&mut self, noob: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(self.state == State::Waiting, "OOB requires Waiting state");
        anyhow::ensure!(
            self.dirp == 2 || self.dirp == 3,
            "server-to-peer OOB not negotiated"
        );
        anyhow::ensure!(noob.len() == 16, "Noob must be 16 bytes");
        self.m.set_noob(noob);
        let _ = self.m.hoob(2); // fingerprint (informational for the PIN flow)
        Ok(())
    }

    fn send(&self, w: Wire) -> Outcome {
        Outcome {
            send: Some(serde_json::to_vec(&w).unwrap_or_default()),
            ..Default::default()
        }
    }
    fn done_failure(&self) -> Outcome {
        Outcome {
            send: Some(result_bytes("failure")),
            done: true,
            ..Default::default()
        }
    }
    fn check_peer_id(&self, wm: &Wire) -> Result<(), String> {
        if self.m.peer_id.is_empty() {
            return Ok(());
        }
        match parse_str(&wm.peer_id) {
            Some(got) if got == self.m.peer_id => Ok(()),
            _ => Err("PeerId mismatch".into()),
        }
    }
    fn fail(&self, info: &str) -> Outcome {
        self.fail_code(error_code::INVALID_DATA, info)
    }
    fn fail_code(&self, code: i64, info: &str) -> Outcome {
        Outcome {
            send: Some(err_bytes(code, info)),
            done: true,
            error: Some(info.to_string()),
            error_code: Some(code),
            ..Default::default()
        }
    }
}

// --- Peer ------------------------------------------------------------------

pub struct Peer {
    versions: Vec<i64>,
    cryptosuites: Vec<i64>,
    prefer_dir: i64,
    state: State,
    m: Method,
    csp: i64,
    dirp: i64,
    peer_info: String,
    assoc: Option<Association>,
}

impl Default for Peer {
    fn default() -> Self {
        Peer {
            versions: vec![1],
            cryptosuites: vec![1, 2],
            prefer_dir: 2,
            state: State::Unregistered,
            m: Method {
                nai: DEFAULT_NAI.to_string(),
                ..Default::default()
            },
            csp: 0,
            dirp: 0,
            peer_info: "{}".to_string(),
            assoc: None,
        }
    }
}

impl Peer {
    pub fn new() -> Self {
        Self::default()
    }
    /// Carry a JSON PeerInfo object (e.g. this node's PairingInfo) that the
    /// server authenticates via the Completion MAC.
    pub fn with_peer_info(mut self, info_json: String) -> Self {
        self.peer_info = info_json;
        self
    }
    pub fn state(&self) -> State {
        self.state
    }
    pub fn association(&self) -> Option<&Association> {
        self.assoc.as_ref()
    }
    /// The server's authenticated ServerInfo (verbatim JSON), valid once Registered.
    pub fn server_info(&self) -> &str {
        &self.m.inp.server_info
    }

    pub fn receive(&mut self, input: &[u8]) -> Outcome {
        let wm: Wire = match serde_json::from_slice(input) {
            Ok(w) => w,
            Err(_) => return self.fail("malformed JSON"),
        };
        if let Some(eap) = &wm.eap {
            return self.on_result(eap);
        }
        if let Some(code) = wm.error_code {
            return Outcome {
                done: true,
                error: Some(peer_error_msg(code, &wm.error_info)),
                error_code: Some(code),
                ..Default::default()
            };
        }
        match wm.type_ {
            Some(1) => self.on_discovery(),
            Some(2) => self.on_negotiation(&wm),
            Some(3) => self.on_key_exchange(&wm),
            Some(4) => self.on_waiting(),
            Some(5) => self.on_noob_id(),
            Some(6) => self.on_completion(&wm),
            _ => self.fail("unexpected type"),
        }
    }

    fn on_result(&mut self, eap: &str) -> Outcome {
        if eap == "success" {
            self.assoc = Some(Association {
                peer_id: self.m.peer_id.clone(),
                nai: self.m.nai.clone(),
                cryptosuitep: self.csp as u8,
                kz: self.m.keym.as_ref().map(|k| k.kz).unwrap_or([0u8; 32]),
            });
            self.state = State::Registered;
            Outcome {
                done: true,
                success: true,
                ..Default::default()
            }
        } else {
            Outcome {
                done: true,
                ..Default::default()
            }
        }
    }

    fn on_discovery(&mut self) -> Outcome {
        let mut w = Wire {
            type_: Some(1),
            peer_state: Some(self.state as i64),
            nai: Some(raw(jstr(&self.m.nai))),
            ..Default::default()
        };
        if self.state != State::Unregistered && !self.m.peer_id.is_empty() {
            w.peer_id = Some(raw(jstr(&self.m.peer_id)));
        }
        self.send(w)
    }

    fn on_negotiation(&mut self, wm: &Wire) -> Outcome {
        let peer_id = match parse_str(&wm.peer_id) {
            Some(p) => p,
            None => return self.fail("bad PeerId"),
        };
        self.m.peer_id = peer_id.clone();
        let svers = parse_ints(&wm.vers).unwrap_or_default();
        let verp = match best_version(&svers, &self.versions) {
            Some(v) => v,
            None => return self.fail("no common version"),
        };
        let scs = parse_ints(&wm.cryptosuites).unwrap_or_default();
        let csp = match first_supported(&scs, &self.cryptosuites) {
            Some(c) => c,
            None => return self.fail("no common cryptosuite"),
        };
        let sdirs = parse_int(&wm.dirs).unwrap_or(-1);
        let dirp = match choose_dir(sdirs, self.prefer_dir) {
            Some(d) => d,
            None => return self.fail("no mutual OOB direction"),
        };
        self.m.suite = Suite::from_id(csp as u8);
        self.csp = csp;
        self.dirp = dirp;
        if let Some(new_nai) = parse_str(&wm.new_nai) {
            self.m.nai = new_nai;
        }
        let peer_info = self.peer_info.clone();
        self.m.inp.vers = get(&wm.vers);
        self.m.inp.peer_id = get(&wm.peer_id);
        self.m.inp.cryptosuites = get(&wm.cryptosuites);
        self.m.inp.dirs = get(&wm.dirs);
        self.m.inp.server_info = ensure_obj(get(&wm.server_info));
        self.m.inp.verp = verp.to_string();
        self.m.inp.cryptosuitep = csp.to_string();
        self.m.inp.dirp = dirp.to_string();
        self.m.inp.peer_info = peer_info.clone();
        self.m.inp.nai = jstr(&self.m.nai);
        let w = Wire {
            type_: Some(2),
            verp: Some(raw(verp.to_string())),
            peer_id: Some(raw(jstr(&peer_id))),
            cryptosuitep: Some(raw(csp.to_string())),
            dirp: Some(raw(dirp.to_string())),
            peer_info: Some(raw(peer_info)),
            ..Default::default()
        };
        self.send(w)
    }

    fn on_key_exchange(&mut self, wm: &Wire) -> Outcome {
        let jwk: Jwk = match serde_json::from_str(&get(&wm.pks)) {
            Ok(j) => j,
            Err(_) => return self.fail("bad PKs"),
        };
        let ns = match parse_str(&wm.ns).and_then(|s| unb64(&s)) {
            Some(n) => n,
            None => return self.fail("bad Ns"),
        };
        let suite = self.m.suite.unwrap();
        let kp = suite.generate();
        let z = match kp.compute_z(&jwk) {
            Ok(z) => z,
            Err(_) => return self.fail("ECDHE failure"),
        };
        let np = rand32();
        let pkp = serde_json::to_string(&kp.public_jwk).unwrap();
        let np_json = jstr(&b64(&np));
        self.m.keypair = Some(kp);
        self.m.z = z;
        self.m.ns = ns;
        self.m.np = np.to_vec();
        self.m.inp.pks = get(&wm.pks);
        self.m.inp.ns = get(&wm.ns);
        self.m.inp.pkp = pkp.clone();
        self.m.inp.np = np_json.clone();
        let w = Wire {
            type_: Some(3),
            peer_id: Some(raw(jstr(&self.m.peer_id))),
            pkp: Some(raw(pkp)),
            np: Some(raw(np_json)),
            ..Default::default()
        };
        self.state = State::Waiting;
        self.send(w)
    }

    fn on_waiting(&mut self) -> Outcome {
        let w = Wire {
            type_: Some(4),
            peer_id: Some(raw(jstr(&self.m.peer_id))),
            ..Default::default()
        };
        self.send(w)
    }

    fn on_noob_id(&mut self) -> Outcome {
        if self.m.noob.is_empty() {
            return self.fail("no OOB received");
        }
        let w = Wire {
            type_: Some(5),
            peer_id: Some(raw(jstr(&self.m.peer_id))),
            noob_id: Some(raw(jstr(&b64(&self.m.noob_id)))),
            ..Default::default()
        };
        self.send(w)
    }

    fn on_completion(&mut self, wm: &Wire) -> Outcome {
        if self.m.noob.is_empty() {
            return self.fail("no OOB processed");
        }
        let got = parse_str(&wm.noob_id)
            .and_then(|s| unb64(&s))
            .unwrap_or_default();
        if got != self.m.noob_id {
            return self.fail_code(error_code::UNRECOGNIZED_OOB_MSG_ID, "unrecognized NoobId");
        }
        let macs = parse_str(&wm.macs)
            .and_then(|s| unb64(&s))
            .unwrap_or_default();
        self.m.derive();
        let kms = self.m.keym.as_ref().unwrap().kms;
        if !mac_equal(&macs, &compute_mac(&kms, 2, &self.m.inp)) {
            return self.fail_code(
                error_code::HMAC_VERIFICATION_FAILED,
                "MACs verification failed",
            );
        }
        let kmp = self.m.keym.as_ref().unwrap().kmp;
        let macp = compute_mac(&kmp, 1, &self.m.inp);
        let w = Wire {
            type_: Some(6),
            peer_id: Some(raw(jstr(&self.m.peer_id))),
            macp: Some(raw(jstr(&b64(&macp)))),
            ..Default::default()
        };
        self.send(w)
    }

    /// Server-to-peer OOB input: accept a 16-byte PIN-derived Noob at Waiting.
    pub fn oob_input_noob(&mut self, noob: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(self.state == State::Waiting, "OOB requires Waiting state");
        anyhow::ensure!(
            self.dirp == 2 || self.dirp == 3,
            "server-to-peer OOB not negotiated"
        );
        anyhow::ensure!(noob.len() == 16, "Noob must be 16 bytes");
        self.m.set_noob(noob);
        self.state = State::OobReceived;
        Ok(())
    }

    fn send(&self, w: Wire) -> Outcome {
        Outcome {
            send: Some(serde_json::to_vec(&w).unwrap_or_default()),
            ..Default::default()
        }
    }
    fn fail(&self, info: &str) -> Outcome {
        self.fail_code(error_code::INVALID_DATA, info)
    }
    fn fail_code(&self, code: i64, info: &str) -> Outcome {
        Outcome {
            send: Some(err_bytes(code, info)),
            done: true,
            error: Some(info.to_string()),
            error_code: Some(code),
            ..Default::default()
        }
    }
}

// --- helpers ---------------------------------------------------------------

/// Render a received peer error notification into a message, preferring the
/// peer's ErrorInfo when present.
fn peer_error_msg(code: i64, info: &Option<String>) -> String {
    match info {
        Some(s) if !s.is_empty() => format!("peer error {code}: {s}"),
        _ => format!("peer error {code}"),
    }
}

fn new_peer_id() -> String {
    b64(&rand16())
}
fn rand16() -> [u8; 16] {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b
}
fn rand32() -> [u8; 32] {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    b
}
fn ensure_obj(s: String) -> String {
    if s.is_empty() {
        "{}".to_string()
    } else {
        s
    }
}
fn best_version(server: &[i64], peer: &[i64]) -> Option<i64> {
    let mut best = None;
    for &v in server {
        if peer.contains(&v) && best.map(|b| v >= b).unwrap_or(true) {
            best = Some(v);
        }
    }
    best
}
fn first_supported(server: &[i64], peer: &[i64]) -> Option<i64> {
    server
        .iter()
        .copied()
        .find(|c| peer.contains(c) && Suite::from_id(*c as u8).is_some())
}
fn choose_dir(server_dirs: i64, prefer: i64) -> Option<i64> {
    match server_dirs {
        1 => Some(1),
        2 => Some(2),
        3 => Some(if prefer == 1 || prefer == 2 {
            prefer
        } else {
            2
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A full server<->peer handshake using a shared PIN-derived Noob reaches
    /// Registered on both sides with an identical Kz.
    #[test]
    fn full_handshake_registers_both_sides() {
        let mut s = Server::new();
        let mut p = Peer::new();

        // Initial Exchange: Type 1 -> ... -> Type 3, ending in EAP-Failure.
        let mut msg = s.start();
        loop {
            let po = p.receive(&msg);
            let pb = po.send.expect("peer sends");
            let so = s.receive(&pb);
            if so.done {
                // The server's terminating EAP-Failure is relayed to the peer.
                if let Some(b) = so.send {
                    let _ = p.receive(&b);
                }
                break;
            }
            msg = so.send.expect("server sends");
        }
        assert_eq!(s.state(), State::Waiting);
        assert_eq!(p.state(), State::Waiting);

        // Out-of-band: a shared 16-byte Noob (as if from a PIN).
        let noob = [7u8; 16];
        s.oob_output_with(&noob).unwrap();
        p.oob_input_noob(&noob).unwrap();

        // Completion Exchange: server restarts the conversation.
        let mut msg = s.start();
        let mut guard = 0;
        loop {
            guard += 1;
            assert!(guard < 12, "handshake did not converge");
            let po = p.receive(&msg);
            if po.success {
                break;
            }
            let pb = po.send.expect("peer sends");
            let so = s.receive(&pb);
            if let Some(b) = &so.send {
                // Deliver server output (may be EAP-Success) to the peer next loop.
                msg = b.clone();
            }
            if so.success {
                // Relay final success to the peer.
                let _ = p.receive(&msg);
                break;
            }
        }
        assert_eq!(s.state(), State::Registered);
        assert_eq!(p.state(), State::Registered);
        assert_eq!(s.association().unwrap().kz, p.association().unwrap().kz);
    }
}
