# Contributing to openpair

*日本語: [CONTRIBUTING.ja.md](CONTRIBUTING.ja.md)*

Thanks for your interest! openpair is an independent Rust implementation
compatible with the Apache-2.0 [NVIDIA Personal AI Router](https://github.com/NVIDIA/Personal-AI-Router).

## Ground rules (important)

- **Respect licenses.** The upstream project is Apache-2.0. You may reference it,
  but do **not** copy upstream source (or any third-party code) verbatim without
  complying with its license — retain copyright/NOTICE and attribute derived
  work. Prefer implementing against the documented interoperability contract
  ([`docs/PROTOCOL.md`](docs/PROTOCOL.md)).
- Keep contributions your own original work, or properly attributed and
  license-compatible (Apache-2.0).
- When you pin down a protocol detail from the upstream source or your own
  testing, note the source/observation in the code comment or PR.

## Development

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test  --workspace
```

- Keep changes focused; add tests for new behavior.
- Follow the existing module style and doc-comment conventions.
- New protocol assumptions that aren't yet confirmed must be marked
  `TODO(interop)` with a note on how to confirm them.

## Commit / PR

- Write clear commit messages (what + why).
- By submitting a contribution you agree to license it under Apache-2.0 and
  certify the [Developer Certificate of Origin](https://developercertificate.org/)
  (add a `Signed-off-by:` line via `git commit -s`).

## Reporting security issues

See [`SECURITY.md`](SECURITY.md) — please do not open public issues for
vulnerabilities.
