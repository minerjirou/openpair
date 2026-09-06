# openpair 使い方マニュアル

*English: [USAGE.md](USAGE.md)*

openpair ノード（`openpair-node`）の導入から、単体運用・クラスタ参加・推論プロキシ利用・
トラブルシュートまでの実務手順です。プロトコルの詳細は [PROTOCOL.ja.md](PROTOCOL.ja.md)、
ペアリングの内部は [PAIRING.ja.md](PAIRING.ja.md) を参照してください。

---

## 1. openpair とは

LAN 内の複数マシンを 1 つの推論システムにまとめる分散ノードです（NVIDIA Personal-AI-Router
互換の独立実装, Apache-2.0）。各ノードが同じソフトを動かし、中央サーバはありません。

- **ローカルの推論エンジン**（Ollama など）の前段に立つプロキシを提供
- 要求されたモデルを持つ**ピア**があれば相互 TLS 経由でそこへルーティング
- NVIDIA / **AMD(ROCm)** 両対応の GPU テレメトリ
- EAP-NOOB(PIN) による安全なクラスタ参加、証明書ピン留めによる相互 TLS 信頼

---

## 2. 導入（ビルド）

必要: Rust 1.98 以上、（推論するなら）ローカルに Ollama など。

```bash
git clone https://github.com/minerjirou/openpair
cd openpair
cargo build --release
# 生成物: target/release/openpair-node
```

GPU 検出だけ試す:
```bash
./target/release/openpair-node --gpucheck
```
各バックエンド（nvidia-smi / amdgpu-sysfs / amd-smi・rocm-smi / OS 列挙 / マージ結果）が
見つけた GPU を表示します。

---

## 3. クイックスタート（単体ノード）

```bash
# 既定: proxy=127.0.0.1:11435, UI=127.0.0.1:7070, backend=127.0.0.1:11434(Ollama)
./target/release/openpair-node
```

起動後:
- **ダッシュボード**: ブラウザで <http://127.0.0.1:7070>
- **推論プロキシ**: クライアントの向き先を `http://127.0.0.1:11435` にする
  - Ollama 互換: `POST /api/generate`, `/api/chat`, `GET /api/tags` など
  - OpenAI 互換: `POST /v1/chat/completions` など

例（Ollama クライアント）:
```bash
curl http://127.0.0.1:11435/api/tags
curl http://127.0.0.1:11435/api/chat -d '{"model":"llama3","messages":[{"role":"user","content":"hi"}]}'
```

`Ctrl-C` で停止します。

---

## 4. 設定（環境変数）

| 変数 | 既定 | 用途 |
|------|------|------|
| `OPENPAIR_DATA_DIR` | `./openpair-data` | スタンドアロン識別情報（`node-cert.pem`/`node-key.pem`）の保存先 |
| `OPENPAIR_BACKEND` | `127.0.0.1:11434` | ローカル推論エンジンの権威（Ollama 既定ポート） |
| `OPENPAIR_PROXY_BIND` | `127.0.0.1:11435` | クラスタ対応ループバックプロキシの bind |
| `OPENPAIR_UI_BIND` | `127.0.0.1:7070` | ダッシュボード + 制御 API の bind（**loopback 推奨**） |
| `OPENPAIR_NODEINFO_BIND` | `127.0.0.1:7071` | `GET /v1/node-info`（ハード構成＋テレメトリ）の bind |
| `OPENPAIR_ADVERTISE_PORT` | node-info ポート | mDNS で広告するポート |
| `OPENPAIR_INGRESS_BIND` | `0.0.0.0:7443` | ピア受信用の相互 TLS `/ingress` bind |
| `OPENPAIR_PAIRING_BIND` | `0.0.0.0:14321` | クラスタ・ペアリング `/v1/cluster/pairing` の bind |
| `OPENPAIR_CLUSTER_DIR` | （未設定） | 参照互換の信頼ディレクトリ（`node.crt`/`node.key`/`trusted/`）。設定するとピン留めが永続化 |
| `OPENPAIR_TRUST_DIR` | （未設定） | **開発用**の証明書共有ディレクトリ（PIN 無しでローカル多ノードを相互信頼） |
| `RUST_LOG` | `info` | ログ量（例 `debug`, `pair_cluster=debug`） |

> **ポート早見表**: UI 7070 / node-info 7071 / proxy 11435 / ingress(mTLS) 7443 /
> pairing 14321。1 台で複数ノードを動かす検証では**全ポートを別値に**してください（§8 参照）。

---

## 5. ダッシュボード

<http://127.0.0.1:7070>（`OPENPAIR_UI_BIND`）で以下を表示・操作できます。

- **Hardware**: CPU / メモリ / GPU（VRAM・利用率バー）
- **Trusted peers (pinned)**: 信頼済みピアの UUID と指紋
- **Routing**: ローカルモデル一覧、探索済みピアと各モデル
- **Cluster pairing (EAP-NOOB)**: クラスタ参加・拡張（§6）
- **Dev trust (certificate exchange)**: 開発用の証明書交換ペアリング（PIN 無し・ローカル検証専用）

自動更新は 2 秒間隔です。

---

## 6. クラスタ・ペアリング（参加／拡張）

PIN を用いた EAP-NOOB ペアリングで、相互 TLS 信頼を確立します。**招く側（inviter）**が
6 桁 PIN を表示し、**参加する側（joiner）**がそれを入力します。GUI と CLI の 2 通り。

到達性の注意:
- inviter は自分が広告するアドレス（`ローカルIP:pairingポート`）で待ち受ける必要があるため、
  `OPENPAIR_PAIRING_BIND=0.0.0.0:<port>` を推奨。
- joiner の到達アドレスは inviter が指定する必要があります（下記の `<host>`）。

### 6-A. GUI で行う
1. **参加する側 (B)** のダッシュボードを開いておく（招待が来ると
   「Invitations to this node」に表示されます）。
2. **招く側 (A)** のダッシュボード → 「Cluster pairing」→ 相手 B のアドレス
   （`HOST` または `HOST:14321`）を入力し **Invite a node**。
3. A に表示された **6 桁 PIN** を、B の一覧行の PIN 欄に入力して **Join**。
4. 成功すると双方の **Trusted peers** に相手が現れます（相互 TLS 確立）。

### 6-B. CLI で行う
参加する側 (B):
```bash
OPENPAIR_PAIRING_BIND=0.0.0.0:14321 openpair-node join
# 表示された自ノードのアドレスを A の管理者へ伝える → PIN 入力を促される
```
招く側 (A):
```bash
OPENPAIR_PAIRING_BIND=0.0.0.0:14322 openpair-node invite <Bのhost[:port]>
# invite id と 6 桁 PIN を表示 → B で PIN を入力すると自動で証明書ピン留めまで完了
```
> `host` のみ指定した場合は `:14321`（既定 inter-node ポート）が付加されます。

### 信頼の永続化
`OPENPAIR_CLUSTER_DIR` を設定して起動すると、ペアリングで得たピア証明書が
`<dir>/trusted/<uuid>.crt` に書き出され、再起動後も信頼が保たれます。未設定時はメモリ上のみ
（プロセス終了で失われます）。

### PIN 誤り
6 桁が一致しないと Completion で失敗し（`incorrect pin`）、両側とも自動でテアダウンします。
再度招待からやり直してください。

詳細な内部仕様は [PAIRING.ja.md](PAIRING.ja.md)。

---

## 7. GPU / ROCm

- 検出は自動（`--gpucheck` で内訳確認）。NVIDIA は `nvidia-smi`、AMD は
  `amd-smi`/`rocm-smi`＋`amdgpu` sysfs、無い環境は OS 列挙（Windows WMI / macOS
  system_profiler / Linux sysfs）にフォールバック。
- テレメトリは `GET /v1/node-info`（`OPENPAIR_NODEINFO_BIND`）で確認できます。
- 実 AMD/ROCm 機での推論 E2E 検証手順は [ROCM_E2E.ja.md](ROCM_E2E.ja.md)。

---

## 8. 1 台で複数ノードを動かす（検証）

全ポートを衝突しないようにずらします。例（ノード B）:
```bash
OPENPAIR_DATA_DIR=./data-B \
OPENPAIR_UI_BIND=127.0.0.1:7075 OPENPAIR_NODEINFO_BIND=127.0.0.1:7076 \
OPENPAIR_PROXY_BIND=127.0.0.1:11436 OPENPAIR_INGRESS_BIND=0.0.0.0:7444 \
OPENPAIR_PAIRING_BIND=0.0.0.0:14321 \
openpair-node
```
ノード A は既定ポート＋ `OPENPAIR_PAIRING_BIND=0.0.0.0:14322` などにして、§6 の手順で
ペアリングできます。

---

## 9. トラブルシュート

| 症状 | 原因 / 対処 |
|------|-------------|
| `address in use` / `os error 10048` | ポート衝突。§8 のように各 bind をずらす。前回プロセスの残骸は `pkill -f openpair-node`（Win は `taskkill`）で掃除 |
| ペアリングが `connection refused`（joiner 側 Completion） | inviter が広告アドレスで待ち受けていない。inviter を `OPENPAIR_PAIRING_BIND=0.0.0.0:<port>` で起動 |
| `pin must be six digits` | PIN は 6 桁の数字。前後空白なしで入力 |
| `completion failed (incorrect pin)` | PIN 誤り。招待からやり直す |
| `already-clustered`（招待が 409 rejected） | 参加側が既に別クラスタに所属。先に離脱が必要 |
| ダッシュボードが空 | ノード起動直後。数秒待つ。`RUST_LOG=debug` でログ確認 |
| プロキシがモデルを返さない | `OPENPAIR_BACKEND` の推論エンジンが起動しているか確認（`curl http://127.0.0.1:11434/api/tags`） |

ログは `RUST_LOG=debug openpair-node` で詳細化できます（`pair_cluster=debug` などクレート単位も可）。

---

## 10. セキュリティ運用の注意

- **ダッシュボード/制御 API は認証がありません**。必ず loopback（既定）に bind し、外部公開しない。
- クラスタ内通信（`/ingress`）は**ピン留め証明書の相互 TLS 1.3 のみ**受理します。
- `OPENPAIR_TRUST_DIR`（開発用証明書交換）は信頼できないディレクトリに向けない。本番は EAP-NOOB
  ペアリング（§6）を使ってください。
- 秘密鍵は `node-key.pem`（データ/クラスタディレクトリ）に保存されます。取り扱いに注意。
