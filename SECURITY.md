# Security Policy

openpair implements cryptographic pairing and mutual-TLS trust, so we take
security reports seriously.

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Instead, use GitHub's private **"Report a vulnerability"** (Security Advisories)
for this repository, or contact the maintainers privately. Include:

- affected component / version (commit hash),
- a description and impact,
- reproduction steps or a proof of concept.

We aim to acknowledge reports promptly and will coordinate a fix and disclosure.

## Scope notes

- The pairing (EAP-NOOB) implementation has byte-exact serializations still
  marked `TODO(interop)`; until confirmed, do **not** rely on cross-vendor
  pairing for a security boundary.
- Trust is by pinned certificate DER over TLS 1.3 mutual auth. Never point the
  dev-trust directory (`OPENPAIR_TRUST_DIR`) at an untrusted location.
- This is early-stage software provided under Apache-2.0 **without warranty**;
  review before production use.
