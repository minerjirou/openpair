# openpair へのコントリビュート

*English: [CONTRIBUTING.md](CONTRIBUTING.md)*

ご関心ありがとうございます。openpair は Apache-2.0 の
[NVIDIA Personal AI Router](https://github.com/NVIDIA/Personal-AI-Router) と互換な
独立 Rust 実装です。

## 基本原則（重要）

- **ライセンス遵守。** 上流は Apache-2.0 です。参照は可能ですが、上流ソース（や第三者コード）を
  ライセンスに従わずに**逐語コピーしない**でください。派生物は著作権表示・NOTICE を保持し帰属を
  明記します。可能な限り、文書化された相互運用契約
  （[`docs/PROTOCOL.ja.md`](docs/PROTOCOL.ja.md)）に対して実装してください。
- コントリビューションは自身のオリジナル、または適切に帰属・Apache-2.0 互換であること。
- プロトコル詳細を上流ソースや自分のテストから確定した場合は、コメントや PR に出典/観測を明記してください。

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
