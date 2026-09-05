# Dynamic-analysis plan — closing the `[live]` items

*日本語: [DYNAMIC_ANALYSIS_PLAN.ja.md](DYNAMIC_ANALYSIS_PLAN.ja.md)*

Static analysis pinned the protocol down to a handful of byte-exact
serializations that can only be confirmed by observing the running reference.
This plan captures exactly what to observe and how, so byte-exact interop can be
finished with a small, targeted effort.

## What still needs confirmation
1. **EAP-NOOB MACs/MACp association array** — the ordered element list HMAC'd
   with Kms/Kmp (RFC 9140 §3.3.2): element order, raw-vs-JSON-quoted per field,
   and the leading `Dir` value.
2. **KDF FixedInfo ordering** — `Np ‖ Ns` vs `Ns ‖ Np`, and any length prefixes.
3. **320-byte KDF output split** — exact offsets for MSK/EMSK/AMSK/MethodId/
   Kms/Kmp/Kz.
4. **Node advertisement JSON** — the exact object a node publishes to peers
   (`nodeUuid`/`hostUuid`/`clusterUuid`/`certPem`/…) and mDNS TXT key spellings.
5. **Endorsement / tombstone payload** — field order in the signed ASCII blob.
6. **Certificate Subject + validity** — O/OU/CN and the not-before/after window.
7. **Reconnect (Types 7–9)** — the Kz-based fast-reconnect exchange.

## How to capture (two independent reference nodes on a LAN)
1. **Instrument the loopback IPC.** The supervisor↔worker channel is
   newline-delimited JSON-RPC over stdio; wrap each worker with a tee (or a
   local stdio proxy) to log every line. This yields the method catalog with
   real params and the node-advertisement JSON (#4) with zero crypto in the way.
2. **Pair two nodes and record `/pairing` + `/invite*`.** Because pairing runs
   over the mesh before mTLS is established for that peer, a LAN capture (or a
   localhost man-in-the-middle on the pairing port) yields the EAP-NOOB
   messages: the JWKs, nonces, `Noob`/`NoobId`, and the MACs — enough to fix
   #1–#3 by matching our KDF+MAC output to the observed `MACs`/`MACp`.
3. **Diff derived keys.** Reproduce the handshake offline with the captured
   inputs; sweep the small space of orderings/offsets (#2/#3) until our `Kms`
   reproduces the captured `MACs`. This is a finite search once the transcript
   is in hand.
4. **Dump one membership blob.** Capture a single endorsement + tombstone from
   `/clustertrust/*` to fix #5; verify our Ed25519 verify accepts it.
5. **Read one leaf cert.** `openssl x509 -text` on a live node cert fixes #6.

## Where each result lands in the code
- #1–#3 → `pair-pairing::{kdf,mac}` (replace the `TODO(interop)` seams).
- #4 → `pair-proto::telemetry` / `pair-discovery` TXT keys + a node-advert type.
- #5 → a `pair-trust::membership` module (endorse/tombstone verify).
- #6 → `pair-trust::identity` Subject/validity.
- #7 → `pair-pairing` reconnect states.

## ROCm end-to-end
The AMD path (`pair-nodeinfo::amd`) is implemented against `amd-smi`/`rocm-smi`
JSON but needs a real AMD/ROCm host to validate field spellings across tool
versions. Validation step: run `openpair-node` on a Radeon/Instinct box, confirm
`/v1/node-info` reports non-null `vram_bytes`/`vram_used_bytes`/
`utilization_percent`, and confirm a peer routes inference to it.

## Safety / scope
Capture only your own two nodes on your own LAN. This is interoperability
testing of software you run; do not capture third-party traffic.
