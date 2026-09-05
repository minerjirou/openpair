# openpair

**PAIR（Personal AI Router）の LAN AI 推論クラスタ・プロトコルと相互接続する、
Rust による独立実装ノード。AMD / ROCm GPU を第一級でサポートします。**

[![CI](https://github.com/minerjirou/openpair/actions/workflows/ci.yml/badge.svg)](https://github.com/minerjirou/openpair/actions/workflows/ci.yml)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](./LICENSE)

*English: [README.md](README.md)*

> **独立実装です。** openpair は **[NVIDIA Personal AI Router](https://github.com/NVIDIA/Personal-AI-Router)**
> （Apache-2.0）と互換な独立 Rust 実装です。PAIR プロトコルの相互運用契約を再現して
> PAIR クラスタと相互接続し、上流ソースを参照しています。両プロジェクトとも Apache-2.0 です。
> [`NOTICE`](./NOTICE) と [法的事項](#法的事項) を参照してください。

---

## なぜ作るのか

PAIR クラスタは、LAN 上の複数マシンを 1 つの推論システムとして扱います。アプリは
ローカルのエンドポイントに話しかけるだけで、リクエストは空いた GPU と対象モデルを持つ
マシンへ透過的にルーティングされます。参照実装の GPU テレメトリ層は `nvidia-smi` しか
理解しないため、**AMD マシンが戦力になれません**。

openpair は次の 2 つを実現します：

1. **相互接続** — 同じ mDNS 探索・JSON-RPC IPC・mutual TLS 信頼モデル・HTTP データプレーン
2. **AMD / ROCm（および NVIDIA、Intel）を均一にサポート** — Radeon / Instinct マシンも
   クラスタの第一級メンバーになれます

## 特徴

- **ベンダー非依存の GPU テレメトリ** — NVIDIA（`nvidia-smi`）、AMD（カーネル `amdgpu`
  sysfs＝*ROCm ツール不要*、または `amd-smi`/`rocm-smi`）、OS レベルの列挙（Windows WMI、
  macOS `system_profiler`）。CPU/メモリは `sysinfo` でクロスプラットフォーム対応。
- **mDNS 探索** — `_nvpair-node._tcp` サービスの広告と発見。
- **クラスタ信頼** — Ed25519 ノード証明書、証明書**ピンニング**、**TLS 1.3 相互認証**。
  参照互換の `node.crt` / `node.key` / `trusted/` クラスタディレクトリ。
- **EAP-NOOB（RFC 9140）ペアリングのプリミティブ** — X25519 / P-256 スイート、
  NIST SP 800-56C one-step KDF、HMAC-SHA256 による確認。
- **データプレーン** — loopback の Ollama（`/api/*`）/ OpenAI（`/v1/*`）リバースプロキシ。
  リクエストごとにローカルエンジン、または対象モデルを広告する pin 済み peer を選び、
  mutual TLS `/ingress` へ転送します。
- 統合デーモン 1 本：`openpair-node`。

## ワークスペース構成

| クレート | 役割 |
|-------|------|
| [`pair-proto`](crates/pair-proto) | ワイヤ型：JSON-RPC 2.0 エンベロープ、テレメトリスキーマ、mDNS/TXT 契約、確定済みメソッド/エンドポイント定数 |
| [`pair-rpc`](crates/pair-rpc) | 改行区切り JSON-RPC 2.0 stdio トランスポート |
| [`pair-nodeinfo`](crates/pair-nodeinfo) | CPU/メモリ＋GPU テレメトリ（NVIDIA / AMD / OS 列挙） |
| [`pair-discovery`](crates/pair-discovery) | mDNS `_nvpair-node._tcp` の広告＋発見 |
| [`pair-trust`](crates/pair-trust) | Ed25519 アイデンティティ、証明書 pin、mutual TLS、クラスタ dir |
| [`pair-pairing`](crates/pair-pairing) | EAP-NOOB（RFC 9140）スイート、KDF、MAC、メッセージ |
| [`pair-proxy`](crates/pair-proxy) | リバースプロキシ、モデルベースのルーティング、mTLS `/ingress` |
| [`pair-ui`](crates/pair-ui) | ノード Web ダッシュボード＋制御API（ペアリング・状態） |
| [`pair-node`](crates/pair-node) | `openpair-node` デーモン |

## クイックスタート

```sh
# 全体をビルド＆テスト
cargo build --workspace
cargo test  --workspace

# 検出された GPU（とその検出経路）を表示
cargo run -p pair-node --bin openpair-node -- --gpucheck

# ノードを起動（既定でローカル Ollama 127.0.0.1:11434 と通信）
cargo run -p pair-node --bin openpair-node
curl -s http://127.0.0.1:7071/v1/node-info | jq

# ダッシュボードを開く
#   http://127.0.0.1:7070
```

### 設定（環境変数）

| 変数 | 既定値 | 意味 |
|----------|---------|---------|
| `OPENPAIR_BACKEND` | `127.0.0.1:11434` | ローカルエンジン（Ollama）のアドレス |
| `OPENPAIR_PROXY_BIND` | `127.0.0.1:11435` | loopback の Ollama/OpenAI プロキシ |
| `OPENPAIR_NODEINFO_BIND` | `127.0.0.1:7071` | `GET /v1/node-info` |
| `OPENPAIR_INGRESS_BIND` | `0.0.0.0:7443` | peer 向け mutual TLS `/ingress` |
| `OPENPAIR_UI_BIND` | `127.0.0.1:7070` | Web ダッシュボード＋制御API |
| `OPENPAIR_ADVERTISE_PORT` | node-infoポート | mDNS 広告ポート |
| `OPENPAIR_CLUSTER_DIR` | — | 参照互換の信頼 dir（`node.crt`/`node.key`/`trusted/`） |
| `OPENPAIR_DATA_DIR` | `./openpair-data` | 単体運用時のアイデンティティ保存先 |

## ドキュメント

- [`docs/PROTOCOL.ja.md`](docs/PROTOCOL.ja.md) — 相互運用契約。各項目に **[confirmed]**
  または **[live]**（動的キャプチャで要確認）を付記。
- [`docs/PAIRING.ja.md`](docs/PAIRING.ja.md) — クラスタ・ペアリング（EAP-NOOB）の全体像：
  `/v1/cluster/pairing` のワイヤ契約・参加フロー・`openpair-node invite` / `join`。
- [`docs/ROCM_E2E.ja.md`](docs/ROCM_E2E.ja.md) — 実 AMD/ROCm ハードでの検証手順。
- [`docs/DYNAMIC_ANALYSIS_PLAN.ja.md`](docs/DYNAMIC_ANALYSIS_PLAN.ja.md) — 残る byte 厳密な
  `[live]` 項目の詰め方。
- [`ROADMAP.ja.md`](ROADMAP.ja.md) — フェーズ計画と進捗。

## ステータス

初期段階ですが動作し、テスト済みです（60 以上のユニット/統合テスト）。静的に確定できる
プロトコル面は実装・検証済みで、実ハードウェアおよび参照 `nvpair-node-info` ワーカーに
対するライブ確認（`/v1/node-info` のワイヤ形状が一致）も含みます。ペアリングの一部 byte
厳密なシリアライズはライブ 2 ノードのキャプチャで確定する必要があり、ソース内で
`TODO(interop)` として明示、`docs/DYNAMIC_ANALYSIS_PLAN.ja.md` に列挙しています。

## コントリビュート

[`CONTRIBUTING.ja.md`](CONTRIBUTING.ja.md) を参照。脆弱性報告は
[`SECURITY.ja.md`](SECURITY.ja.md) を参照してください。

## 法的事項

openpair は **[NVIDIA Personal AI Router](https://github.com/NVIDIA/Personal-AI-Router)**
（Apache License 2.0）と互換な**独立した Rust 実装**です。PAIR プロトコルの相互運用契約を
再現し、上流ソースを参照しています。両プロジェクトとも Apache-2.0 です。上流由来の成果物を
利用・再配布する際は、上流の Apache-2.0 ライセンスに従い、帰属表示と NOTICE を保持してください。

「NVIDIA」「PAIR」「Personal AI Router」は各所有者の商標です。**本プロジェクトは
NVIDIA と提携・承認・後援の関係にありません。** 名称は互換性を説明するための
指示的使用に限られます。相互運用する各ソフトウェアのライセンス・利用規約への適合は
利用者の責任です。

## ライセンス

[Apache License, Version 2.0](LICENSE) の下でライセンスされます。[`NOTICE`](NOTICE) も参照。
