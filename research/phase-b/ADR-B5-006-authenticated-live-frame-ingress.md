# ADR B5-006: authenticated bounded RGB ingress

Date: 2026-10-04
Status: Implemented local software increment; broader B5 and qualification remain open

The capture lease owns pixels and identity, but lifecycle control messages alone cannot carry continuous input without producing durable request receipts per frame. Use a distinct strict channel on the existing current-user bounded bulk endpoint. Bind the handshake to the paired actor, engine boot and exact lease identity. Require fresh authorization at frame-header admission and again after the fixed-size body read under the database/registry lock order used by retirement.

Transmit one packed 64x64 RGB image per operation. Derive presentation time from the admitted clock, verify SHA-256 against bytes, reject source/project clock regression and use the existing latest chronological pair owner. Retain only the latest frame header for exact retry; no per-frame database receipt or artifact file is created. The counter and latest header are ephemeral status observations. Older retries and altered identities refuse. Poison client streams on ambiguous outcomes and require explicit status reconciliation and reconnection.

The server shares its sixteen-connection admission limit, has one fixed 12,288-byte read buffer per connection and retains existing I/O deadlines. Pixel-runtime admission is separate. This decision does not admit model/preprocessing memory, calibrate capture clocks, renew leases, acquire a source, execute persistent inference or authorize physical commands. Those remain explicit successor requirements; Handy and GPU fastest-preset six-axis scope is retained.

Local native, authored peer and shipped CLI regressions are retained in the [frame-ingress checkpoint](../../docs/evidence/roadmap-2026-10-01/live-frame-ingress/validation.json). No physical or real-source qualification is inferred from these fixtures. Project schema, portable format and private Stash proof versions are unchanged.
