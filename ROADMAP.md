# openpair development roadmap

Goal: a clean-room, Rust node that (a) **interoperates with real PAIR clusters**
and (b) adds **ROCm/AMD** GPU support, with no third-party source reuse.

## Phases

### Phase 0 — foundation ✅ (in progress)
- [x] Cargo workspace, license/notice, git repo
- [x] `pair-proto`: JSON-RPC 2.0 envelope, telemetry schema, mDNS/TXT contract
- [x] `pair-nodeinfo`: NVIDIA (`nvidia-smi`) + **AMD (`amd-smi`/`rocm-smi`)** GPU
      telemetry, `/proc` CPU + memory
- [~] `pair-discovery`: mDNS `_nvpair-node._tcp` advertise + browse

### Phase 1 — protocol confirmation (analysis)
Deep RE passes (Ghidra via MCP + app.asar) to lock the wire contract. Outputs
land in `docs/protocol-rpc.md` and `docs/protocol-crypto.md`.
- [ ] stdio JSON-RPC framing (JSONL vs length-prefixed vs Content-Length)
- [ ] full JSON-RPC method catalog + params/result shapes
- [ ] HTTP endpoint contracts (`/ingress`, `/pairing`, `/invite*`, `/clustertrust/*`, `/api/*`, `/v1/*`)
- [ ] exact mDNS TXT keys
- [ ] EAP-NOOB parameters (suite id, curve, KDF, message fields, encoding)
- [ ] certificate profile (key type, CN/SAN/OID, validity, UUID derivation)
- [ ] endorsement/tombstone signed payload layout + signature algorithm

### Phase 2 — cluster security (interop-critical)
- [ ] `pair-trust`: identity mint (cert), peer cert pinning, mutual-TLS client/server config
- [ ] `pair-pairing`: EAP-NOOB (RFC 9140) server + peer state machines
- [ ] membership: signed endorsement / tombstone verify + apply

### Phase 3 — data plane
- [ ] `pair-proxy`: Ollama (`/api/*`) + OpenAI (`/v1/*`) reverse proxy, loopback-only ingress
- [ ] cluster `/ingress` over mTLS; candidate selection by advertised model + priority
- [ ] `pair-node` supervisor: wire discovery + trust + nodeinfo + proxy together

### Phase 4 — verification
- [ ] two-node local cluster: discovery → pairing → mTLS → inference routing
- [ ] interop test against a reference node (dynamic capture to confirm framing/crypto)
- [ ] ROCm end-to-end: AMD host advertises GPUs, receives routed inference

## Interoperability posture
Only the protocol contract is reproduced (identifiers, field names, framing,
crypto parameters) — the minimum required for interoperable software. No
proprietary code, binaries, or decompiler output are committed. See `NOTICE`.
