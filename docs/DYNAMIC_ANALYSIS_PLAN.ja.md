# 動的解析計画 — `[live]` 項目を詰める

*English: [DYNAMIC_ANALYSIS_PLAN.md](DYNAMIC_ANALYSIS_PLAN.md)*

静的解析でプロトコルはごく僅かな byte 厳密シリアライズを残すところまで確定しました。
これらは稼働中の参照実装を観測してのみ確定できます。本書は「何を、どう観測するか」を
まとめ、小さく的を絞った作業で byte 厳密な相互運用を仕上げられるようにします。

## まだ確定が必要な項目
1. **EAP-NOOB MACs/MACp の association 配列** — Kms/Kmp で HMAC する順序付き要素列
   （RFC 9140 §3.3.2）：要素順、各フィールドが生か JSON クオート済みか、先頭 `Dir` 値。
2. **KDF FixedInfo の順序** — `Np ‖ Ns` か `Ns ‖ Np` か、長さ接頭辞の有無。
3. **KDF 320 バイト出力の分割** — MSK/EMSK/AMSK/MethodId/Kms/Kmp/Kz の正確なオフセット。
4. **ノード広告 JSON** — ノードが peer に公開する正確なオブジェクト
   （`nodeUuid`/`hostUuid`/`clusterUuid`/`certPem`/…）と mDNS TXT キーの綴り。
5. **endorsement / tombstone ペイロード** — 署名対象 ASCII ブロブのフィールド順。
6. **証明書 Subject と有効期間** — O/OU/CN と not-before/after。
7. **再接続（Type 7–9）** — Kz ベースの高速再接続交換。

## 取得方法（LAN 上の参照ノード 2 台）
1. **loopback IPC を計装**。スーパーバイザ↔ワーカーは stdio 上の改行区切り JSON-RPC。
   各ワーカーを tee（またはローカル stdio プロキシ）で包んで全行をログ化。暗号を挟まず
   メソッドカタログの実 params とノード広告 JSON（#4）が得られます。
2. **2 ノードをペアリングし `/pairing` + `/invite*` を記録**。ペアリングはその peer 用の
   mTLS 確立前にメッシュ上で走るため、LAN キャプチャ（またはペアリングポートの localhost
   MITM）で EAP-NOOB メッセージ（JWK・nonce・`Noob`/`NoobId`・MAC）が得られ、#1〜#3 を
   確定できます。
3. **導出鍵を突き合わせ**。捕捉した入力でハンドシェイクをオフライン再現し、順序/オフセットの
   小さな候補空間（#2/#3）を、自分の `Kms` が捕捉した `MACs` を再現するまで走査。手元に
   トランスクリプトがあれば有限探索です。
4. **メンバーシップブロブを 1 つ取得**。`/clustertrust/*` から endorsement + tombstone を
   1 つ捕捉し #5 を確定、Ed25519 検証が通ることを確認。
5. **リーフ証明書を 1 つ読む**。稼働ノードの証明書に `openssl x509 -text` で #6 を確定。

## 各結果のコード反映先
- #1–#3 → `pair-pairing::{kdf,mac}`（`TODO(interop)` の seam を置換）。
- #4 → `pair-proto::telemetry` / `pair-discovery` の TXT キー＋ノード広告型。
- #5 → `pair-trust::membership` モジュール（endorse/tombstone 検証）。
- #6 → `pair-trust::identity` の Subject/有効期間。
- #7 → `pair-pairing` の再接続状態。

## ROCm エンドツーエンド
AMD 経路（`pair-nodeinfo::amd`）は `amd-smi`/`rocm-smi` JSON と `amdgpu` sysfs に対して
実装済みですが、ツールのバージョン差によるフィールド綴りを実 AMD/ROCm ホストで検証する
必要があります。検証手順：Radeon/Instinct マシンで `openpair-node` を動かし、
`/v1/node-info` が非 null の `vram_bytes`/`vram_used_bytes`/`utilization_percent` を返すこと、
peer がその AMD マシンへ推論をルーティングできることを確認（`ROCM_E2E.ja.md`）。

## 安全 / スコープ
自分の LAN 上の自分の 2 ノードのみをキャプチャしてください。これは自分が動かす
ソフトウェアの相互運用テストです。第三者の通信を取得してはいけません。
