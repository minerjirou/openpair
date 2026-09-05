# ROCm / AMD end-to-end validation

*日本語: [ROCM_E2E.ja.md](ROCM_E2E.ja.md)*

The AMD detection + telemetry path is implemented across all platforms:

| Platform | Static inventory | Live VRAM-used / utilization |
|----------|------------------|------------------------------|
| Linux    | `amdgpu` sysfs (`/sys/class/drm/card*/device/*`) | same sysfs (`mem_info_vram_used`, `gpu_busy_percent`) — **no ROCm tools required** |
| Linux    | `amd-smi` / `rocm-smi` (fallback / enrichment) | `amd-smi metric` / `rocm-smi --showuse` |
| Windows  | `Win32_VideoController` (WMI) | (compute telemetry via `amd-smi` if installed) |
| macOS    | `system_profiler SPDisplaysDataType` | — |

It has been **live-verified on Windows** (an AMD Radeon iGPU is enumerated with
no ROCm tools present). The steps below validate the full path on a **real AMD /
ROCm Linux host**, which is the only environment that exercises live AMD GPU
telemetry + inference routing to an AMD node.

## 1. Detection self-check (on the AMD host)

```sh
cargo run -p pair-node --bin openpair-node -- --gpucheck
```

Expect the `amdgpu-sysfs` line to list your AMD GPU(s) with **non-null**
`vram=…`, `used=…`, `util=…`, e.g.:

```
[amdgpu-sysfs] 1 GPU(s)
  - Amd AMD GPU (amdgpu 0x744c)  vram=Some(25753026560) used=Some(1288490188) util=Some(37)
[merged] ...
[detect_gpus] 1 GPU(s)
  - Amd ...
```

If `amdgpu-sysfs` is empty, confirm the kernel `amdgpu` driver is loaded and the
sysfs attributes exist:

```sh
ls /sys/class/drm/card0/device/mem_info_vram_total /sys/class/drm/card0/device/gpu_busy_percent
```

## 2. node-info telemetry

```sh
OPENPAIR_BACKEND=127.0.0.1:11434 cargo run -p pair-node --bin openpair-node
curl -s http://127.0.0.1:7071/v1/node-info | jq
```

Confirm the AMD GPU appears with `vram_bytes`, `vram_used_bytes`,
`utilization_percent`, and `telemetryValid: true`.

## 3. Two-node inference routing to the AMD node

On the AMD host run an Ollama (ROCm build) with a model pulled, then start an
`openpair-node` there; on a second host start another `openpair-node`. Establish
trust (share a cluster dir / pair), and from the second node send an inference
request for a model only the AMD host has:

```sh
curl http://127.0.0.1:11435/api/generate -d '{"model":"<model-on-amd-host>","prompt":"hi"}'
```

Success criteria: the request is routed over mutual-TLS `/ingress` to the AMD
node, served by its ROCm Ollama, and the response returns — i.e. an AMD/ROCm box
is a first-class inference target in the cluster.

## Notes
- Windows WMI `AdapterRAM` is a 32-bit field capped at 4 GiB; for AMD cards with
  >4 GB VRAM on Windows, install `amd-smi` for accurate VRAM (the sysfs path on
  Linux is already exact).
- Marketing names on the Linux sysfs path need a PCI-ID database; the code
  reports `AMD GPU (amdgpu <device-id>)`. `amd-smi`, when present, supplies the
  market name.
