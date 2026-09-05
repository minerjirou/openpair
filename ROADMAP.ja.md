# openpair 開発ロードマップ

*English: [ROADMAP.md](ROADMAP.md)（進捗の詳細・最新のチェックリストは英語版を参照）*

目標：第三者コードを再利用せず、(a) 実 PAIR クラスタと**相互接続**し、(b) **ROCm/AMD**
GPU をサポートする、Rust のクリーンルームノード。

## フェーズ概要

- **フェーズ 0 — 基盤**：ワークスペース／ライセンス・NOTICE／git、`pair-proto`、
  `pair-nodeinfo`（NVIDIA＋AMD/ROCm）、`pair-discovery`、`pair-rpc`。✅
- **フェーズ 1 — プロトコル確定**：stdio フレーミング（=JSONL 確定）、JSON-RPC メソッド
  カタログ、HTTP エンドポイント契約、mDNS TXT キー、EAP-NOOB パラメータ、証明書プロファイル、
  endorsement/tombstone。大半 ✅、一部 byte 厳密は要ライブキャプチャ。
- **フェーズ 2 — クラスタセキュリティ**：`pair-trust`（Ed25519・pin・mTLS・クラスタ dir）✅、
  `pair-pairing`（EAP-NOOB プリミティブ）✅、メンバーシップ検証（一部 live）。
- **フェーズ 3 — データプレーン**：`pair-proxy`（ローカル＋クラスタ横断ルーティング、
  mTLS `/ingress`）✅、`pair-node` 統合デーモン ✅。
- **フェーズ 4 — 検証**：2 ノードでの探索→ペアリング→mTLS→推論ルーティング。参照実装との
  相互運用テスト（フレーミング/暗号のライブ確定）、ROCm エンドツーエンド（実 AMD 機）。

## 動的検証で確認済みの所見
- `/v1/node-info` のワイヤ形状を参照実装（`nvpair-node-info`）と一致確認。✅
- CPU/メモリ検出を移植化（sysinfo：Windows/macOS/Linux）。✅（Windows 実機確認）
- OS レベル GPU 列挙（Windows WMI / macOS system_profiler / Linux amdgpu sysfs）で ROCm
  ツール無しでも AMD を列挙。✅（Windows 実機確認、実 ROCm compute は要 HW 検証）

## 相互運用の姿勢
再現するのはプロトコル契約（識別子・フィールド名・フレーミング・暗号パラメータ）のみで、
相互運用可能なソフトウェアに必要な最小限です。proprietary なコード・バイナリ・逆コンパイル
出力はコミットしません。[`NOTICE`](NOTICE) を参照。
