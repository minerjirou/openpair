# Contributing to openpair

Thanks for your interest! openpair is a clean-room, interoperable implementation
and we intend to keep it that way.

## Ground rules (important)

- **Clean-room only.** Do **not** paste, adapt, or upload third-party source
  code, decompiler/disassembler output, proprietary binaries, or copyrighted
  assets. Contributions must be your own original work.
- Contribute against the **documented interoperability contract**
  ([`docs/PROTOCOL.md`](docs/PROTOCOL.md)) — field names, framing, and crypto
  parameters needed to interoperate — not by copying an implementation.
- If you confirm a `[live]` protocol detail via your own dynamic testing on your
  own machines, cite *what you observed* (values/shape), not any code.

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
