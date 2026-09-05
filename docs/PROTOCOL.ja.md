# openpair 相互運用契約

*English: [PROTOCOL.md](PROTOCOL.md)*

これは openpair が PAIR LAN 推論クラスタと相互接続するために実装するワイヤ契約です。
記載は*インターフェース*（識別子・フレーミング・スキーマ・暗号パラメータ）に限り、
相互運用可能なソフトウェアに必要な最小限です。確度は **[confirmed]**（静的に確定）か
**[live]**（動的キャプチャで要確認。`DYNAMIC_ANALYSIS_PLAN.ja.md` 参照）で示します。

## 1. 探索 — mDNS / DNS-SD
- サービス型 **`_nvpair-node._tcp`** [confirmed]。
- ノード情報はサービスの **TXT** レコード。クラスタ ID キーは `cluster-uuid` [confirmed]、
  node-uuid/host/port/addresses 系キー [名称は confirmed、完全な集合は live]。未知キーは
  往復時に保持します。

## 2. スーパーバイザ IPC — stdio 上の JSON-RPC 2.0
- フレーミング：**改行区切り JSON**（1 行 1 オブジェクト、`\n` 終端）[confirmed]。
  エンベロープ `{jsonrpc:"2.0", id, method, params, result, error}`、`id` は接続ごとの整数、
  応答は id で対応付け。
- スーパーバイザ（`ui-broker` 役）がワーカーを spawn しリレーします。約 45 のメソッドと
  約 44 の Electron IPC チャネルは `pair-proto::contract::methods` とローカル解析ノートに列挙。

## 3. HTTP サーフェス（役割別）[confirmed パス]
複数プロトコルが 1 ポートを共有（"splitlisten"：最初のバイトで平文 HTTP と mTLS を判別）。
- **proxy**：`/api/*`（Ollama）、`/v1/*`（OpenAI）、`/ingress`（peer 転送）、`/set-priority`、`/nodeactivity`。
- **node-info**：`GET /v1/node-info` — ハードウェア構成＋テレメトリ。
- **cluster-manager**：`/pairing`、`/invite`（＋`_status`/`_expiry`/`_provenance`）、
  `/v1/cluster/{pairing,roster,members/remove}`。
- **clustertrust メッシュ**：`/clustertrust/{membership,mesh,peerclient,watch}`。
- **engine-manager**：`/v1/engines`、`/modelops`、`/remotepeers`。
- **workload**：`/v1/workloads/events`（SSE）。**errors**：`/peersync`。

### 3.1 `/ingress` エンベロープ [confirmed フィールド]
`{host, port, path, name, data, txt, code}` — `data` はラップされたリクエスト/レスポンス本体、
`code`/`txt` は応答ステータス。

### 3.2 `/v1/node-info` テレメトリスキーマ [confirmed フィールド名]
GPU ごと：`vram_bytes`、`vram_used_bytes`、`utilization_percent`、`vendor`、`vendor_id`、
`product`/`name`。加えて `GPUs`、`telemetryValid`、`node_uuid`。openpair はこれらを
`nvidia-smi` **または** `amd-smi`/`rocm-smi` で均一に埋めます。

## 4. クラスタセキュリティ
- **アイデンティティ** [confirmed]：Ed25519 自己署名 X.509 リーフ、SAN URI
  `urn:nvpair:node:<uuid>`、EKU serverAuth+clientAuth、128 bit ランダムシリアル、
  fingerprint `sha256:<DER の hex>`。クラスタ UUID はランダム生成。
  （Subject O/OU/CN と有効期間は [live]。）
- **トランスポート** [confirmed]：TLS 1.3 のみ、相互認証必須、信頼は**pin 済み生証明書 DER**
  （pin 前に peer の SAN UUID を検証）。
- **メンバーシップ** [アルゴリズムは confirmed]：改行連結・ドメイン接頭辞付き ASCII への
  Ed25519 署名 `nvpair-endorse:v2\n…` / `nvpair-remove:v2\n…`（承認 / 失効）。
  正確なフィールド配置は [live]。

## 5. ペアリング — EAP-NOOB（RFC 9140）
- スイート [confirmed]：**1 = X25519**（JWK OKP）、**2 = P-256**（JWK EC）。ハッシュ
  **SHA-256**、HMAC-SHA256。
- KDF [confirmed]：NIST SP 800-56C one-step、SHA-256、32bit BE カウンタ（1 始まり）、
  `algorithm-id = "EAP-NOOB"`、`FixedInfo = "EAP-NOOB" ‖ Np ‖ Ns ‖ Noob`、320 バイト出力
  → MSK/EMSK/AMSK/MethodId/Kms/Kmp/Kz。全バイナリフィールドは base64url（無パディング）。
- メッセージ列 [confirmed]：`Type` 1..6 =
  Discovery → Negotiation → KeyExchange → Waiting → NoobID → Completion。
- **[live]**：FixedInfo 内の `Np`/`Ns` 順序、320 バイト分割オフセット、MACs/MACp の
  association データ配列の正確な組み立て。openpair はプリミティブを実装し、これらは
  明示された seam の背後に置いています。

---
上記 §1〜§5 の **[confirmed]** は本ワークスペースで実装・ユニット/統合テスト済みです。
**[live]** 項目が参照実装との byte 厳密な相互運用を残す唯一のゲートであり、動的解析計画の
対象です。
