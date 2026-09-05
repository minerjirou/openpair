# Cluster Pairing (EAP-NOOB / joining a real cluster)

*日本語: [PAIRING.ja.md](PAIRING.ja.md)*

How an openpair node **joins** a real PAIR cluster (or **grows** its own). It carries
RFC 9140 EAP-NOOB onto the plain-HTTP `/v1/cluster/pairing` channel a real cluster
speaks, doing six-digit-PIN mutual authentication through to certificate pinning
(establishing mutual-TLS trust).

Implementation: crate [`pair-cluster`](../crates/pair-cluster). The state machine itself
lives in [`pair-pairing`](../crates/pair-pairing) (see [PROTOCOL.md](PROTOCOL.md) §5 and
crypto in [protocol-crypto.md](protocol-crypto.md)).

---

## 1. Roles

| Role | EAP-NOOB | OOB direction | Drives |
|------|----------|---------------|--------|
| **inviter** (grows a cluster) | Server | server→peer (`Dirs=2`) | Initial Exchange |
| **joiner** (joins a cluster) | Peer | `PreferDir=2` | Completion Exchange |

The PIN is **displayed by the inviter** and **entered on the joiner** (server→peer OOB).

---

## 2. Wire contract

Everything is `POST /v1/cluster/pairing` (**plain HTTP** — there is no trust yet; the
EAP-NOOB MACs, not the transport, authenticate the exchange).

### Envelope
```json
{
  "inviteId": "<uuid>",
  "phase":    "initial | completion | cancel | decline | fail | ack | expired",
  "msg":      "<base64(standard) of the EAP-NOOB blob>",
  "rejected": false,
  "reason":   ""
}
```
- `msg` is the **standard base64** of the EAP-NOOB message (compact JSON); empty for the
  Completion kickoff.
- A joiner that explicitly refuses replies `409` with `{rejected:true, reason:"already-clustered"}`.

### Phases
- `initial` / `completion` — the EAP-NOOB handshake (§4 below).
- `cancel` — inviter → joiner; drop a pending inbound invite.
- `decline` / `fail` / `expired` — joiner → inviter terminal signals (immediate teardown).
  `fail` with `reason:"incorrect-pin"` is a wrong PIN.
- `ack` — joiner → inviter; acknowledges the durable commit.

---

## 3. PairingInfo (§7.2)

The identity object each node embeds in EAP-NOOB **ServerInfo (inviter) / PeerInfo
(joiner)**. It is **bound into the Completion MAC**, so tampering with it fails MAC
verification.

```json
{
  "v": 2,
  "nodeUuid": "<uuid>",
  "nodeId": "sha256:<hex>",
  "name": "<host name>",
  "clusterId": "<cluster uuid | empty>",
  "admissionEpoch": 1,
  "clusterFriendlyName": "<display name>",
  "addr": "<host:port>",
  "cert": "-----BEGIN CERTIFICATE----- …"
}
```
- On receipt, the embedded certificate's **principal (URN/CN) must equal `nodeUuid`**;
  a mismatch is rejected (you cannot present someone else's cert under your UUID).
- `addr` is **required** on the inviter's ServerInfo (the joiner drives Completion there);
  advisory on the joiner's PeerInfo.
- `v>=2` requires a non-zero `admissionEpoch`; `v1` (no epoch) normalizes to epoch 1.

---

## 4. Handshake (two exchanges)

EAP-NOOB is two HTTP-separated exchanges with the human PIN step between them.

```mermaid
sequenceDiagram
    participant I as inviter (Server)
    participant J as joiner (Peer)

    Note over I,J: Initial Exchange (inviter-driven)
    I->>J: POST initial  Type1 (Discovery)
    J-->>I: Type1 (PeerState, NAI)
    I->>J: POST initial  Type2 (Negotiation: Vers/Cryptosuites/Dirs/ServerInfo)
    J-->>I: Type2 (Verp/Cryptosuitep/Dirp/PeerInfo)
    I->>J: POST initial  Type3 (KeyExchange: PKs/Ns)
    J-->>I: Type3 (PKp/Np)  ← both Waiting

    Note over I,J: Out-of-band (human)
    I->>I: display PIN (server→peer)
    J->>J: enter PIN → Noob = noobFromPIN(pin)

    Note over I,J: Completion Exchange (joiner-driven)
    J->>I: POST completion  msg="" (kickoff)
    I-->>J: Type1 (Server.Start)
    J->>I: POST completion  Type1 (PeerState=OobReceived)
    I-->>J: Type5 (NoobId request)
    J->>I: POST completion  Type5 (NoobId)
    I-->>J: Type6 (MACs)
    J->>I: POST completion  Type6 (MACp)
    I-->>J: eap:"success"  ← both Registered; each pins the other's cert
    J->>I: POST ack
```

- Completion key derivation: ECDH `Z` plus `Np/Ns/Noob` feed a NIST SP 800-56C one-step
  KDF (SHA-256); the 320-byte output splits into MSK/EMSK/AMSK/MethodId/Kms/Kmp/**Kz**.
  See [protocol-crypto.md](protocol-crypto.md).
- Confirmation MACs: `MACs` (Kms, lead=2) and `MACp` (Kmp, lead=1) are HMAC-SHA256 over a
  17-element verbatim JSON array that includes ServerInfo/PeerInfo (= PairingInfo), so each
  side's identity is authenticated by the MAC.

### PIN → Noob
The six-digit PIN encodes to a 16-byte **big-endian** (left zero-padded) Noob, matching
upstream `noobFromPIN` (`big.Int.FillBytes`). E.g. `123456` → `00…00 01 E2 40`.

---

## 5. Failure handling

EAP-NOOB error notifications are `{Type:0, ErrorCode, ErrorInfo}` (`eap` is reserved for
success/failure). The receiver surfaces the peer's code as `Outcome.error_code`.

| Code | Meaning | Class |
|------|---------|-------|
| 2003 `UnrecognizedOOBMsgID` | NoobId mismatch (the wrong-PIN signature) | **wrong-PIN** |
| 4001 `HMACVerificationFailed` | Completion MAC failed | **wrong-PIN** |
| 3001/3002/3003 | unsupported version / cryptosuite / OOB direction | negotiation |
| 2001/2002/1003 | PeerId / state / invalid data | protocol |

A wrong PIN is classified **by code** (not string matching). The joiner, on detecting it,
sends the inviter `fail` + `reason:"incorrect-pin"` so both sides tear down immediately.

---

## 6. Establishing trust

On success the peer's **authenticated certificate** (the PEM embedded in PairingInfo, bound
into the MAC, with principal==nodeUuid verified) is handed to a `TrustSink`. The daemon's
`ClusterTrustSink`:
1. pins the certificate into the live `SharedPins` (mutual TLS accepts it immediately);
2. when `OPENPAIR_CLUSTER_DIR` is set, writes `trusted/<uuid>.crt` so trust survives a restart;
3. marks the node clustered, refusing further inbound joins (the reference single-cluster
   invariant).

The pinned peer is then picked up by the existing mDNS discovery for model polling and
cluster routing over mutual-TLS `/ingress` (see [PROTOCOL.md](PROTOCOL.md) §3).

---

## 7. Operating (openpair-node)

The daemon always serves the pairing channel (`OPENPAIR_PAIRING_BIND`). Operator-driven
pairing uses two subcommands.

### Joiner (join a cluster)
```bash
OPENPAIR_PAIRING_BIND=0.0.0.0:14321 openpair-node join
```
Prints this node's reachable address and waits for an invite, then prompts for the PIN.
Ask the cluster owner to run `openpair-node invite <that address>`.

### Inviter (grow a cluster)
```bash
OPENPAIR_PAIRING_BIND=0.0.0.0:14322 openpair-node invite <joiner-host[:port]>
```
Drives the Initial Exchange to the joiner and prints an **invite id and six-digit PIN**.
Once the joiner enters the PIN and completes, certificate pinning happens automatically.

> A bare host gets `:14321` appended (the upstream default inter-node port). Bind `0.0.0.0`
> so the inviter listens on the address it advertises.

### Environment
| Var | Default | Purpose |
|-----|---------|---------|
| `OPENPAIR_PAIRING_BIND` | `0.0.0.0:14321` | pairing-channel bind |
| `OPENPAIR_CLUSTER_DIR` | (unset) | reference-compatible trust dir (`node.crt`/`node.key`/`trusted/`); persists pins when set |
| `OPENPAIR_DATA_DIR` | `./openpair-data` | standalone identity store |

---

## 8. Verification status

- Library E2E ([`crates/pair-cluster/tests/e2e.rs`](../crates/pair-cluster/tests/e2e.rs)):
  pairs two nodes over real localhost HTTP (happy path + wrong PIN, mutual cert pinning,
  agreeing Kz).
- Verified end-to-end between two live daemons (`invite`/`join` PIN handoff → both complete
  Completion → each logs certificate pinned / mutual-TLS trust established).
- `#7 reconnect` (Types 7–9, KeyingMode 3 / Kz) is **reserved / unimplemented** upstream, so
  it is not needed for interop.

Field interop against a real PAIR cluster remains to be validated on hardware with a real
peer cluster.
