# openpair User Manual

*日本語: [USAGE.ja.md](USAGE.ja.md)*

Practical steps for running an openpair node (`openpair-node`): from install to
standalone operation, joining a cluster, using the inference proxy, and
troubleshooting. Protocol details are in [PROTOCOL.md](PROTOCOL.md); pairing
internals in [PAIRING.md](PAIRING.md).

---

## 1. What openpair is

A distributed node that pools several LAN machines into one inference system (an
independent, Apache-2.0 implementation compatible with NVIDIA Personal-AI-Router).
Every node runs the same software; there is no central server.

- A proxy in front of a **local inference engine** (e.g. Ollama)
- Routes a request to a **peer** that has the requested model, over mutual TLS
- GPU telemetry for NVIDIA and **AMD (ROCm)**
- Secure cluster join via EAP-NOOB (PIN), trust by certificate pinning

---

## 2. Install (build)

Requires Rust 1.98+ and (to actually infer) a local engine such as Ollama.

```bash
git clone https://github.com/minerjirou/openpair
cd openpair
cargo build --release
# produces target/release/openpair-node
```

Check GPU detection only:
```bash
./target/release/openpair-node --gpucheck
```
Prints what each backend found (nvidia-smi / amdgpu-sysfs / amd-smi·rocm-smi / OS
inventory / merged result).

---

## 3. Quick start (single node)

```bash
# defaults: proxy=127.0.0.1:11435, UI=127.0.0.1:7070, backend=127.0.0.1:11434 (Ollama)
./target/release/openpair-node
```

Then:
- **Dashboard**: <http://127.0.0.1:7070>
- **Inference proxy**: point your client at `http://127.0.0.1:11435`
  - Ollama-compatible: `POST /api/generate`, `/api/chat`, `GET /api/tags`, …
  - OpenAI-compatible: `POST /v1/chat/completions`, …

Example (Ollama client):
```bash
curl http://127.0.0.1:11435/api/tags
curl http://127.0.0.1:11435/api/chat -d '{"model":"llama3","messages":[{"role":"user","content":"hi"}]}'
```

Stop with `Ctrl-C`.

---

## 4. Configuration (environment)

| Variable | Default | Purpose |
|----------|---------|---------|
| `OPENPAIR_DATA_DIR` | `./openpair-data` | standalone identity store (`node-cert.pem`/`node-key.pem`) |
| `OPENPAIR_BACKEND` | `127.0.0.1:11434` | local inference engine authority (Ollama default port) |
| `OPENPAIR_PROXY_BIND` | `127.0.0.1:11435` | cluster-aware loopback proxy bind |
| `OPENPAIR_UI_BIND` | `127.0.0.1:7070` | dashboard + control API bind (**keep on loopback**) |
| `OPENPAIR_NODEINFO_BIND` | `127.0.0.1:7071` | `GET /v1/node-info` (hardware + telemetry) bind |
| `OPENPAIR_ADVERTISE_PORT` | node-info port | port advertised over mDNS |
| `OPENPAIR_INGRESS_BIND` | `0.0.0.0:7443` | mutual-TLS `/ingress` receiver for peers |
| `OPENPAIR_PAIRING_BIND` | `0.0.0.0:14321` | cluster pairing `/v1/cluster/pairing` bind |
| `OPENPAIR_CLUSTER_DIR` | (unset) | reference-compatible trust dir (`node.crt`/`node.key`/`trusted/`); persists pins when set |
| `OPENPAIR_TRUST_DIR` | (unset) | **dev-only** shared cert dir (local multi-node mutual trust without a PIN) |
| `RUST_LOG` | `info` | log verbosity (e.g. `debug`, `pair_cluster=debug`) |

> **Port cheat sheet**: UI 7070 / node-info 7071 / proxy 11435 / ingress (mTLS)
> 7443 / pairing 14321. Running several nodes on one host requires **distinct
> ports for all of them** (see §8).

---

## 5. Dashboard

<http://127.0.0.1:7070> (`OPENPAIR_UI_BIND`) shows and controls:

- **Hardware**: CPU / memory / GPU (VRAM + utilization bars)
- **Trusted peers (pinned)**: trusted peer UUIDs and fingerprints
- **Routing**: local models, discovered peers and their models
- **Cluster pairing (EAP-NOOB)**: join or grow a cluster (§6)
- **Dev trust (certificate exchange)**: dev-only cert-swap pairing (no PIN, local testing)

Auto-refreshes every 2 seconds.

---

## 6. Cluster pairing (join / grow)

EAP-NOOB PIN pairing establishes mutual-TLS trust. The **inviter** displays a
six-digit PIN; the **joiner** enters it. Two ways: GUI or CLI.

Reachability notes:
- The inviter must listen on the address it advertises (`localIP:pairingPort`), so
  prefer `OPENPAIR_PAIRING_BIND=0.0.0.0:<port>`.
- The joiner's reachable address is what the inviter targets (`<host>` below).

### 6-A. Via the GUI
1. Open the **joiner (B)** dashboard (incoming invites appear under
   "Invitations to this node").
2. On the **inviter (A)** dashboard → "Cluster pairing" → enter B's address
   (`HOST` or `HOST:14321`) and click **Invite a node**.
3. Type the **six-digit PIN** shown on A into B's row PIN field, click **Join**.
4. On success each side's **Trusted peers** lists the other (mutual TLS established).

### 6-B. Via the CLI
Joiner (B):
```bash
OPENPAIR_PAIRING_BIND=0.0.0.0:14321 openpair-node join
# prints this node's address for A's owner; then prompts for the PIN
```
Inviter (A):
```bash
OPENPAIR_PAIRING_BIND=0.0.0.0:14322 openpair-node invite <B-host[:port]>
# prints an invite id + six-digit PIN; when B enters it, pinning completes automatically
```
> A bare `host` gets `:14321` appended (the default inter-node port).

### Persisting trust
Start with `OPENPAIR_CLUSTER_DIR` set and paired peer certificates are written to
`<dir>/trusted/<uuid>.crt`, so trust survives a restart. Without it, pins are
in-memory only (lost on exit).

### Wrong PIN
A mismatched six digits fails Completion (`incorrect pin`) and both sides tear down
automatically; start over from the invite.

Full internals: [PAIRING.md](PAIRING.md).

---

## 7. GPU / ROCm

- Detection is automatic (`--gpucheck` shows the breakdown). NVIDIA via
  `nvidia-smi`; AMD via `amd-smi`/`rocm-smi` + `amdgpu` sysfs; otherwise OS
  inventory (Windows WMI / macOS system_profiler / Linux sysfs).
- Telemetry is served at `GET /v1/node-info` (`OPENPAIR_NODEINFO_BIND`).
- End-to-end validation on real AMD/ROCm hardware: [ROCM_E2E.md](ROCM_E2E.md).

---

## 8. Multiple nodes on one host (testing)

Shift every port so nothing collides. Example (node B):
```bash
OPENPAIR_DATA_DIR=./data-B \
OPENPAIR_UI_BIND=127.0.0.1:7075 OPENPAIR_NODEINFO_BIND=127.0.0.1:7076 \
OPENPAIR_PROXY_BIND=127.0.0.1:11436 OPENPAIR_INGRESS_BIND=0.0.0.0:7444 \
OPENPAIR_PAIRING_BIND=0.0.0.0:14321 \
openpair-node
```
Run node A on default ports plus e.g. `OPENPAIR_PAIRING_BIND=0.0.0.0:14322`, then
pair per §6.

---

## 9. Troubleshooting

| Symptom | Cause / fix |
|---------|-------------|
| `address in use` / `os error 10048` | port collision. Shift each bind (§8). Clean leftovers with `pkill -f openpair-node` (Windows: `taskkill`) |
| pairing `connection refused` (joiner Completion) | inviter isn't listening on its advertised address. Start it with `OPENPAIR_PAIRING_BIND=0.0.0.0:<port>` |
| `pin must be six digits` | PIN is six digits, no surrounding whitespace |
| `completion failed (incorrect pin)` | wrong PIN; start over from the invite |
| `already-clustered` (invite 409 rejected) | the joiner already belongs to another cluster; it must leave first |
| dashboard is empty | node just started; wait a few seconds. Inspect with `RUST_LOG=debug` |
| proxy returns no models | check the `OPENPAIR_BACKEND` engine is up (`curl http://127.0.0.1:11434/api/tags`) |

Increase logging with `RUST_LOG=debug openpair-node` (per-crate too, e.g.
`pair_cluster=debug`).

---

## 10. Security notes

- **The dashboard / control API is unauthenticated.** Always bind it to loopback
  (the default); never expose it.
- Intra-cluster traffic (`/ingress`) accepts **only pinned-certificate mutual TLS 1.3**.
- Do not point `OPENPAIR_TRUST_DIR` (dev cert-swap) at an untrusted directory; use
  EAP-NOOB pairing (§6) in production.
- The private key lives in `node-key.pem` (data/cluster dir); handle with care.
