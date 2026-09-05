# openpair

**An independent Rust implementation of a node that interoperates with the
*Personal AI Router* (PAIR) LAN AI-inference clustering protocol — with
first-class AMD / ROCm GPU support.**

[![CI](https://github.com/minerjirou/openpair/actions/workflows/ci.yml/badge.svg)](https://github.com/minerjirou/openpair/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](./LICENSE)

*日本語: [README.ja.md](README.ja.md)*

> **Independent implementation.** openpair is an independent Rust implementation
> compatible with the **[NVIDIA Personal AI Router](https://github.com/NVIDIA/Personal-AI-Router)**
> (Apache-2.0). It reproduces the PAIR protocol's interoperability contract to
> interoperate with PAIR clusters and is informed by the upstream source. Both
> projects are Apache-2.0. See [`NOTICE`](./NOTICE) and [Legal](#legal).

---

## Why

A PAIR cluster lets several machines on a LAN act as one inference system: an
app talks to a local endpoint, and requests transparently route to whichever
machine has a free GPU and the right model. The reference stack's GPU-telemetry
layer only understands `nvidia-smi`, so **AMD boxes can't pull their weight**.

openpair does two things:

1. **Interoperates** with a PAIR cluster — the same mDNS discovery, JSON-RPC
   IPC, mutual-TLS trust model, and HTTP data plane; and
2. **Supports AMD / ROCm** (and NVIDIA, and Intel) *uniformly*, so a
   Radeon/Instinct host is a first-class cluster member.

## Features

- **GPU telemetry, any vendor** — NVIDIA (`nvidia-smi`), AMD (kernel `amdgpu`
  sysfs — *no ROCm tools needed* — or `amd-smi`/`rocm-smi`), and OS-level
  inventory (Windows WMI, macOS `system_profiler`). Cross-platform CPU/memory
  via `sysinfo`.
- **mDNS discovery** of the `_nvpair-node._tcp` service, advertise + browse.
- **Cluster trust** — Ed25519 node certificates, certificate **pinning**, and
  **TLS 1.3 mutual auth**; a reference-compatible `node.crt` / `node.key` /
  `trusted/` cluster directory.
- **EAP-NOOB (RFC 9140) pairing primitives** — X25519 / P-256 cryptosuites, the
  NIST SP 800-56C one-step KDF, HMAC-SHA256 confirmation.
- **Data plane** — a loopback Ollama (`/api/*`) / OpenAI (`/v1/*`) reverse proxy
  that routes each request to the local engine or a pinned peer that advertises
  the requested model, forwarding over mutual-TLS `/ingress`.
- One integrated daemon: `openpair-node`.

## Workspace layout

| crate | role |
|-------|------|
| [`pair-proto`](crates/pair-proto) | wire types: JSON-RPC 2.0 envelope, telemetry schema, mDNS/TXT contract, confirmed method/endpoint constants |
| [`pair-rpc`](crates/pair-rpc) | newline-delimited JSON-RPC 2.0 stdio transport |
| [`pair-nodeinfo`](crates/pair-nodeinfo) | CPU/memory + GPU telemetry (NVIDIA / AMD / OS inventory) |
| [`pair-discovery`](crates/pair-discovery) | mDNS `_nvpair-node._tcp` advertise + browse |
| [`pair-trust`](crates/pair-trust) | Ed25519 identity, cert pinning, mutual-TLS, cluster dir |
| [`pair-pairing`](crates/pair-pairing) | EAP-NOOB (RFC 9140) cryptosuites, KDF, MACs, messages |
| [`pair-proxy`](crates/pair-proxy) | reverse proxy, model-based routing, mTLS `/ingress` |
| [`pair-ui`](crates/pair-ui) | node web dashboard + control API (pairing, status) |
| [`pair-node`](crates/pair-node) | the `openpair-node` daemon |

## Quick start

```sh
# Build & test everything
cargo build --workspace
cargo test  --workspace

# See what GPUs are detected (and via which backend)
cargo run -p pair-node --bin openpair-node -- --gpucheck

# Run a node (talks to a local Ollama at 127.0.0.1:11434 by default)
cargo run -p pair-node --bin openpair-node
curl -s http://127.0.0.1:7071/v1/node-info | jq

# then open the dashboard
#   http://127.0.0.1:7070
```

### Configuration (environment)

| Variable | Default | Meaning |
|----------|---------|---------|
| `OPENPAIR_BACKEND` | `127.0.0.1:11434` | local engine (Ollama) authority |
| `OPENPAIR_PROXY_BIND` | `127.0.0.1:11435` | loopback Ollama/OpenAI proxy |
| `OPENPAIR_NODEINFO_BIND` | `127.0.0.1:7071` | `GET /v1/node-info` |
| `OPENPAIR_INGRESS_BIND` | `0.0.0.0:7443` | mutual-TLS `/ingress` for peers |
| `OPENPAIR_UI_BIND` | `127.0.0.1:7070` | web dashboard + control API |
| `OPENPAIR_ADVERTISE_PORT` | node-info port | mDNS advertised port |
| `OPENPAIR_CLUSTER_DIR` | — | reference-compatible trust dir (`node.crt`/`node.key`/`trusted/`) |
| `OPENPAIR_DATA_DIR` | `./openpair-data` | standalone identity store (when no cluster dir) |

## Documentation

- [`docs/PROTOCOL.md`](docs/PROTOCOL.md) — the interoperability contract, each
  item tagged **[confirmed]** or **[live]** (pending dynamic capture).
- [`docs/ROCM_E2E.md`](docs/ROCM_E2E.md) — validating the AMD/ROCm path on real
  hardware.
- [`docs/DYNAMIC_ANALYSIS_PLAN.md`](docs/DYNAMIC_ANALYSIS_PLAN.md) — how to close
  the remaining byte-exact `[live]` items.
- [`ROADMAP.md`](ROADMAP.md) — phased plan and status.

## Status

Early but functional and tested (60+ unit/integration tests). The
statically-determinable protocol surface is implemented and verified, including
live checks against real hardware and the reference `nvpair-node-info` worker
(the `/v1/node-info` wire shape matches). A short list of byte-exact pairing
serializations still needs confirmation from a live two-node capture — these are
clearly marked `TODO(interop)` in the source and enumerated in
`docs/DYNAMIC_ANALYSIS_PLAN.md`.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Security reports: [`SECURITY.md`](SECURITY.md).

## Legal

openpair is an **independent Rust implementation** compatible with the
**[NVIDIA Personal AI Router](https://github.com/NVIDIA/Personal-AI-Router)**,
which is licensed under the Apache License 2.0. openpair reproduces the PAIR
protocol's interoperability contract and is informed by the upstream source;
both projects are Apache-2.0. When using or redistributing material derived from
the upstream project, comply with its Apache-2.0 license and retain its
attributions and NOTICE.

"NVIDIA", "PAIR", and "Personal AI Router" are trademarks of their respective
owners. **This project is not affiliated with, endorsed, or sponsored by
NVIDIA.** Names are used only nominatively to describe compatibility. You are
responsible for ensuring your use complies with the licenses and terms that
apply to any software you interoperate with.

## License

Licensed under the [Apache License, Version 2.0](LICENSE). See [`NOTICE`](NOTICE).
