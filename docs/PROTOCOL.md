# openpair interoperability contract

*日本語: [PROTOCOL.ja.md](PROTOCOL.ja.md)*

This is the wire contract `openpair` implements to interoperate with a PAIR
LAN inference cluster. It records only the *interface* (identifiers, framing,
schemas, cryptographic parameters) — the minimum needed for interoperable
software. Confidence is marked **[confirmed]** (statically determined) or
**[live]** (still to be confirmed by dynamic capture; see
`DYNAMIC_ANALYSIS_PLAN.md`).

## 1. Discovery — mDNS / DNS-SD
- Service type **`_nvpair-node._tcp`** [confirmed].
- Service **TXT** keys **[confirmed by live capture]**: `v=1`, `uuid=<node-uuid>`,
  `ip=<addr>`, plus `cluster-uuid` once the node has joined a cluster. The SRV
  record's port is the **node-info port** (where `GET /v1/node-info` is served).
  Unknown keys are preserved on round-trip.

## 2. Supervisor IPC — JSON-RPC 2.0 over stdio
- Framing: **newline-delimited JSON** (one compact object per `\n` line)
  [confirmed]. Envelope `{jsonrpc:"2.0", id, method, params, result, error}`;
  integer `id` per connection, responses matched by id.
- The supervisor (`ui-broker` role) spawns workers and relays. Method catalog
  (~45) and Electron IPC channels (~44) are enumerated in
  `pair-proto::contract::methods` and the local analysis notes.

## 3. HTTP surfaces (per role) [confirmed paths]
Multiple protocols may share one port ("splitlisten": plain HTTP + cluster
mTLS distinguished by first bytes).
- **proxy**: `/api/*` (Ollama), `/v1/*` (OpenAI), `/ingress` (peer forwarding),
  `/set-priority`, `/nodeactivity`.
- **node-info**: `GET /v1/node-info` — hardware inventory + telemetry.
- **cluster-manager**: `/pairing`, `/invite`(+`_status`/`_expiry`/`_provenance`),
  `/v1/cluster/{pairing,roster,members/remove}`.
- **clustertrust mesh**: `/clustertrust/{membership,mesh,peerclient,watch}`.
- **engine-manager**: `/v1/engines`, `/modelops`, `/remotepeers`.
- **workload**: `/v1/workloads/events` (SSE). **errors**: `/peersync`.

### 2.1 `/ingress` envelope [confirmed fields]
`{host, port, path, name, data, txt, code}` — `data` carries the wrapped
request/response body; `code`/`txt` carry response status.

### 2.2 `/v1/node-info` telemetry schema [confirmed field names]
Per GPU: `vram_bytes`, `vram_used_bytes`, `utilization_percent`, `vendor`,
`vendor_id`, `product`/`name`, plus `GPUs`, `telemetryValid`, `node_uuid`.
`openpair` fills these from `nvidia-smi` **or** `amd-smi`/`rocm-smi` uniformly.

## 4. Cluster security
- **Identity** [confirmed]: Ed25519 self-signed X.509 leaf; SAN URI
  `urn:nvpair:node:<uuid>` **plus a DNS SAN of the hostname**; EKU
  serverAuth+clientAuth; KeyUsage=DigitalSignature (critical); BasicConstraints
  CA:FALSE (critical); 128-bit random serial; **validity = now .. +2 years**;
  Subject/Issuer `CN=<node-uuid>`; fingerprint `sha256:<hex-of-DER>`. Cluster
  UUID is a random-minted id. **[all confirmed by reading a reference cert]**
- **Transport** [confirmed]: TLS 1.3 only; mutual auth mandatory; trust is by
  **pinned raw certificate DER** (a peer's SAN UUID is validated before pinning).
- **Membership** [confirmed alg]: Ed25519 signatures over newline-joined,
  domain-prefixed ASCII payloads `nvpair-endorse:v2\n…` / `nvpair-remove:v2\n…`
  (endorsement / tombstone). Exact field layout: [live].

## 5. Pairing — EAP-NOOB (RFC 9140)
- Cryptosuites [confirmed]: **1 = X25519** (JWK OKP), **2 = P-256** (JWK EC);
  hash **SHA-256**, HMAC-SHA256 MACs.
- KDF [confirmed]: NIST SP 800-56C one-step, SHA-256, 32-bit BE counter from 1,
  `algorithm-id = "EAP-NOOB"`, `FixedInfo = "EAP-NOOB" ‖ Np ‖ Ns ‖ Noob`,
  320-byte output → MSK/EMSK/AMSK/MethodId/Kms/Kmp/Kz. All binary fields are
  base64url (no padding).
- Transport [confirmed by live capture]: pairing is a **phased** exchange POSTed
  to **`/v1/cluster/pairing`** over plain HTTP (before mTLS trust exists); an
  out-of-order phase returns `409 phase does not match session state`.
- Message sequence [confirmed]: `Type` 1..6 =
  Discovery → Negotiation → KeyExchange → Waiting → NoobID → Completion.
- **[confirmed from upstream source (Apache-2.0)]**: FixedInfo =
  `"EAP-NOOB" || Np || Ns || len(Noob) as one byte || Noob`; 320-byte split =
  MSK(64) EMSK(64) AMSK(64) MethodId(32) Kms(32) Kmp(32) Kz(32); MAC/Hoob input =
  a 17-element compact JSON array `[lead, Vers, Verp, PeerId, Cryptosuites, Dirs,
  ServerInfo, Cryptosuitep, Dirp, NAI, PeerInfo, 0, PKs, Ns, PKp, Np, Noob]`
  (lead: MACs=2, MACp=1, Hoob=dir), HMAC-SHA256[:32] / SHA-256[:16]; NoobId =
  H(["NoobId", Noob])[:16]. Membership: Ed25519 over
  `nvpair-endorse:v2
…` / `nvpair-remove:v2
…` newline-joined payloads,
  base64 signature. Only the reconnect exchange (KeyingMode 3, Kz) is unmodelled.

---
All of §1–§5 that is **[confirmed]** is implemented and unit/integration-tested
in this workspace. The **[live]** items are the remaining gate to byte-exact
interoperation with the reference and are the subject of the dynamic-analysis
plan.
