# ROCm / AMD エンドツーエンド検証

*English: [ROCM_E2E.md](ROCM_E2E.md)*

AMD の検出＋テレメトリ経路は全プラットフォームで実装済みです：

| プラットフォーム | 静的な列挙 | ライブの VRAM 使用量 / 使用率 |
|----------|------------------|------------------------------|
| Linux    | `amdgpu` sysfs（`/sys/class/drm/card*/device/*`） | 同 sysfs（`mem_info_vram_used`、`gpu_busy_percent`）— **ROCm ツール不要** |
| Linux    | `amd-smi` / `rocm-smi`（フォールバック / 補完） | `amd-smi metric` / `rocm-smi --showuse` |
| Windows  | `Win32_VideoController`（WMI） | （`amd-smi` があれば compute テレメトリ） |
| macOS    | `system_profiler SPDisplaysDataType` | — |

**Windows では実機検証済み**です（ROCm ツール無しで AMD Radeon iGPU を列挙）。以下は
**実 AMD / ROCm (Linux) ホスト**で全経路を検証する手順です。ライブの AMD GPU テレメトリと
AMD ノードへの推論ルーティングを実際に動かせるのはこの環境だけです。

## 1. 検出セルフチェック（AMD ホスト上）

```sh
cargo run -p pair-node --bin openpair-node -- --gpucheck
```

`amdgpu-sysfs` 行に AMD GPU が **非 null** の `vram=…`、`used=…`、`util=…` 付きで
出ることを確認します。例：

```
[amdgpu-sysfs] 1 GPU(s)
  - Amd AMD GPU (amdgpu 0x744c)  vram=Some(25753026560) used=Some(1288490188) util=Some(37)
[merged] ...
[detect_gpus] 1 GPU(s)
  - Amd ...
```

`amdgpu-sysfs` が空の場合は、カーネル `amdgpu` ドライバがロードされ sysfs 属性が
存在するか確認します：

```sh
ls /sys/class/drm/card0/device/mem_info_vram_total /sys/class/drm/card0/device/gpu_busy_percent
```

## 2. node-info テレメトリ

```sh
OPENPAIR_BACKEND=127.0.0.1:11434 cargo run -p pair-node --bin openpair-node
curl -s http://127.0.0.1:7071/v1/node-info | jq
```

AMD GPU が `vram_bytes`、`vram_used_bytes`、`utilization_percent`、`telemetryValid: true`
付きで現れることを確認します。

## 3. 2 ノードで AMD ノードへ推論をルーティング

AMD ホストでモデルを pull した Ollama（ROCm ビルド）を動かし、そこで `openpair-node` を
起動します。2 台目のホストでもう 1 つの `openpair-node` を起動します。信頼を確立
（クラスタ dir 共有 / ペアリング）し、2 台目から **AMD ホストだけが持つモデル**への
推論リクエストを送ります：

```sh
curl http://127.0.0.1:11435/api/generate -d '{"model":"<AMD ホストのモデル>","prompt":"hi"}'
```

成功条件：リクエストが mutual TLS `/ingress` で AMD ノードへルーティングされ、その ROCm
Ollama が処理し、応答が返る — すなわち AMD/ROCm マシンがクラスタの第一級推論ターゲットに
なっていること。

## 補足
- Windows の WMI `AdapterRAM` は 32bit で 4 GiB に頭打ちします。VRAM が 4 GB を超える AMD
  カードでは Windows 上で `amd-smi` を入れると正確になります（Linux の sysfs 経路は正確）。
- Linux sysfs 経路のマーケティング名には PCI-ID データベースが必要です。コードは
  `AMD GPU (amdgpu <device-id>)` と報告します。`amd-smi` があれば製品名を補います。
