# Changelog

All notable changes to this project are documented here. The format is loosely
based on [Keep a Changelog](https://keepachangelog.com/); this project has not
yet made a tagged release.

## [Unreleased]

### Added
- **Web dashboard + control API** (`pair-ui`, `openpair-node` on `OPENPAIR_UI_BIND`,
  default 127.0.0.1:7070): node status, hardware/GPU telemetry, discovered peers
  and routing, and one-click **pairing** between two openpair nodes by exchanging
  certificates (establishing mutual-TLS trust).
- Clean-room Rust workspace implementing a PAIR-interoperable node:
  `pair-proto`, `pair-rpc`, `pair-nodeinfo`, `pair-discovery`, `pair-trust`,
  `pair-pairing`, `pair-proxy`, and the `openpair-node` daemon (`pair-node`).
- GPU telemetry for **NVIDIA** (`nvidia-smi`) and **AMD/ROCm** (kernel `amdgpu`
  sysfs — no ROCm tools required — plus `amd-smi`/`rocm-smi`), OS-level GPU
  inventory (Windows WMI, macOS `system_profiler`), and portable CPU/memory
  (`sysinfo`). `openpair-node --gpucheck` self-check.
- mDNS `_nvpair-node._tcp` advertise + browse.
- Ed25519 node identity, certificate pinning, TLS 1.3 mutual-TLS, and a
  reference-compatible `node.crt`/`node.key`/`trusted/` cluster directory.
- EAP-NOOB (RFC 9140) cryptosuites (X25519/P-256), SP 800-56C one-step KDF,
  HMAC-SHA256 confirmation, and wire messages.
- Loopback Ollama/OpenAI reverse proxy with model-based routing to the local
  engine or a pinned peer over mutual-TLS `/ingress`.
- Documentation: `docs/PROTOCOL.md`, `docs/ROCM_E2E.md`,
  `docs/DYNAMIC_ANALYSIS_PLAN.md`, `ROADMAP.md`.
- Japanese documentation (`*.ja.md`): README, CONTRIBUTING, SECURITY, ROADMAP,
  and the `docs/` guides, cross-linked with the English versions.

### Changed
- Relicensing/attribution: the upstream **NVIDIA Personal AI Router** is
  Apache-2.0 open source (https://github.com/NVIDIA/Personal-AI-Router). Docs
  updated from a "clean-room / reverse-engineered" framing to an independent,
  Apache-2.0-compatible implementation that references and attributes the
  upstream. NOTICE updated accordingly.

### Known limitations
- Some byte-exact EAP-NOOB serializations are marked `TODO(interop)` pending a
  live two-node capture.
- ROCm-compute end-to-end requires validation on real AMD/ROCm hardware
  (see `docs/ROCM_E2E.md`).
