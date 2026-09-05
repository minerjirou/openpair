# openpair development roadmap

Goal: a clean-room, Rust node that (a) **interoperates with real PAIR clusters**
and (b) adds **ROCm/AMD** GPU support, with no third-party source reuse.

## Phases

### Phase 0 — foundation ✅ (in progress)
- [x] Cargo workspace, license/notice, git repo
- [x] `pair-proto`: JSON-RPC 2.0 envelope, telemetry schema, mDNS/TXT contract
- [x] `pair-nodeinfo`: NVIDIA (`nvidia-smi`) + **AMD (`amd-smi`/`rocm-smi`)** GPU
      telemetry, `/proc` CPU + memory
- [x] `pair-discovery`: mDNS `_nvpair-node._tcp` advertise + browse
- [x] `pair-rpc`: newline-delimited JSON-RPC 2.0 transport (confirmed framing)

### Phase 1 — protocol confirmation (analysis)
Deep RE passes (Ghidra via MCP + app.asar) to lock the wire contract. Outputs
land in `docs/protocol-rpc.md` and `docs/protocol-crypto.md`.
- [x] stdio JSON-RPC framing = **JSONL** (confirmed)
- [x] JSON-RPC method catalog (~45) + endpoint paths (encoded in `pair-proto::contract`)
- [x] HTTP endpoint contracts (`/ingress`, `/pairing`, `/invite*`, `/clustertrust/*`, `/api/*`, `/v1/*`)
- [x] mDNS TXT cluster key `cluster-uuid` (byte-verified); others local-noted
- [x] EAP-NOOB parameters (suites 1/2, SP800-56C KDF SHA-256, fields) — some byte-offsets pending live capture
- [x] certificate profile: Ed25519, SAN urn:nvpair:node:<uuid> (Subject/validity pending live)
- [ ] endorsement/tombstone signed payload layout + signature algorithm

### Phase 2 — cluster security (interop-critical)
- [x] `pair-trust`: identity mint, peer pinning, TLS 1.3 mutual-TLS config
- [~] `pair-pairing`: EAP-NOOB primitives (suites/KDF/MAC/messages) done; full state machine + byte-exact MAC assembly pending live capture
- [ ] membership: signed endorsement / tombstone verify + apply

### Phase 3 — data plane
- [x] `pair-proxy`: local + cluster-aware peer routing (model-based select -> mTLS /ingress), model discovery via /api/tags
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

## Dynamic-validation findings (against running reference nvpair-node-info 0.13.3)
- `/v1/node-info` wire shape confirmed and matched: GPU `{name, vram_bytes,
  vram_used_bytes?, utilization_percent?}`, `cpu {name, cores}`, `memory
  {total_bytes}`, `telemetryValid`, `msSince`, `hostUuid`. [done]
- Gap: AMD GPU **static inventory** must come from the OS (ghw/WMI/sysfs), not
  only `amd-smi`/`rocm-smi` — reference enumerates an AMD iGPU with no ROCm
  tools present. [todo: pair-nodeinfo OS GPU inventory]
- Gap: CPU/memory detection is `/proc`-only (Linux); add Windows/macOS. [todo]
