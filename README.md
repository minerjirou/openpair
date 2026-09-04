# openpair

A clean-room, from-scratch **Rust** implementation of a node that interoperates
with the *Personal AI Router* (PAIR) LAN AI-inference clustering protocol — plus
first-class **AMD / ROCm** GPU support that the reference node lacks.

> Independent reimplementation. Contains no third-party source code; only the
> protocol interoperability contract is reproduced. See `NOTICE`.

## Why

The reference stack routes LLM inference requests across machines on a LAN
(mutual-TLS cluster, mDNS discovery, PIN pairing). Its GPU telemetry layer only
understands `nvidia-smi`, so AMD boxes cannot pull their weight. `openpair`:

1. **Interoperates** with real PAIR clusters (same discovery, IPC, and cluster
   security contract), and
2. **Supports ROCm/AMD** (and NVIDIA) uniformly, so a Radeon/Instinct host is a
   first-class cluster member.

## Workspace layout

| crate | role |
|-------|------|
| `pair-proto` | wire types: JSON-RPC 2.0 envelope, telemetry schema, mDNS/TXT contract |
| `pair-nodeinfo` | CPU/mem + GPU telemetry with **NVIDIA (`nvidia-smi`)** and **AMD (`amd-smi`/`rocm-smi`)** backends |
| `pair-discovery` | mDNS `_nvpair-node._tcp` responder + browser |
| `pair-trust` | mutual-TLS identity, certificate minting, peer pinning |
| `pair-pairing` | EAP-NOOB (RFC 9140) PIN pairing |
| `pair-proxy` | Ollama/OpenAI reverse proxy + cluster `/ingress` (mTLS) |
| `pair-node` | node daemon / supervisor binary (`openpair-node`) |

## Status

Early. `pair-proto` and `pair-nodeinfo` (incl. ROCm) are implemented and tested;
discovery / trust / pairing / proxy are being built as the protocol contract is
finalized. See `docs/`.

## Build

```sh
cargo build --workspace
cargo test  --workspace
```

## License

Apache-2.0. See `LICENSE` and `NOTICE`.
