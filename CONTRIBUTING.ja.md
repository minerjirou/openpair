# openpair へのコントリビュート

*English: [CONTRIBUTING.md](CONTRIBUTING.md)*

ご関心ありがとうございます。openpair はクリーンルームの相互運用実装であり、その原則を
維持します。

## 基本原則（重要）

- **クリーンルーム限定。** 第三者のソースコード、逆コンパイル/逆アセンブル出力、
  proprietary バイナリ、著作物を貼り付け・流用・アップロードしないでください。
  コントリビューションはあなた自身のオリジナルな成果物である必要があります。
- 実装のコピーではなく、**文書化された相互運用契約**
  （[`docs/PROTOCOL.ja.md`](docs/PROTOCOL.ja.md)：フィールド名・フレーミング・相互運用に
  必要な暗号パラメータ）に対して貢献してください。
- 自分のマシンでの動的テストにより `[live]` 項目を確定した場合は、コードではなく
  *観測した内容*（値・形状）を根拠として示してください。

## 開発

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test  --workspace
```

- 変更は小さくまとめ、新しい振る舞いにはテストを追加してください。
- 既存のモジュール様式と doc コメント規約に従ってください。
- 未確定のプロトコル前提には `TODO(interop)` を付け、確定方法を注記してください。

## コミット / PR

- 明確なコミットメッセージ（何を・なぜ）を書いてください。
- コントリビューション提出により、Apache-2.0 でのライセンスに同意し、
  [Developer Certificate of Origin](https://developercertificate.org/) を証明するものと
  します（`git commit -s` で `Signed-off-by:` を付与）。

## セキュリティ問題の報告

[`SECURITY.ja.md`](SECURITY.ja.md) を参照。脆弱性を公開 issue に書かないでください。
