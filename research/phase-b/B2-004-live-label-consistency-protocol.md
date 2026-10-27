# B2-004: Live-reference label and sampled-clock consistency

Status: Frozen before measurement; diagnostic only.
Date: 2026-10-08

Use only the existing hash-bound training snapshots in `.pulsar-reference-evidence/live-position-training-2026-10-04/`, the consumed development dataset/split in `.pulsar-reference-evidence/live-position-holdout-2026-10-04-retry/`, and their declared receipts. No sterile inputs, media decoding, model fitting/export/selection, frame-feature selection or label/offset changes.

Independently reconstruct every accepted row from the saved clip PTS, source origin/timebase, explicit reference offset (legacy schema-1 zero) and exact supplied script interpolation. Compare ordered entry IDs, PTS pairs and float32 labels with the dataset. Verify reference snapshots against receipt digests and verify manifest/dataset/split correspondence. Read bounded array headers; do not load image tensors. Source byte separation alone is not source-family identity.

For every accepted pair with both script endpoints supported, compute supplied-script polyline variation through all strictly interior action knots, endpoint displacement, maximum excursion outside the two endpoint positions, and the number of strict local turning knots inside the pair. Count positive hidden excursions using exact rational arithmetic and report normalized magnitudes. Strict turning counts exclude plateau extrema and boundary knots; they are not a complete motion-aliasing classifier. Report aggregate, optimizer/development and per-entry counts without choosing a new sampling rate or fitting a correction.

Reject changed identities, malformed/unbounded inputs, inconsistent clip omission/sample counts, empty/overlapping split declarations or non-monotonic PTS. A reconstructed label discrepancy is a failed bookkeeping audit, not permission to overwrite labels. A successful reconstruction establishes agreement with declared script clocks only, never visual/anatomical/physical alignment. Between-frame excursions describe supplied-label detail absent from endpoint positions; they do not prove the cause of model underfitting or that all temporal methods fail.

Test interpolation, nonzero origin/offset, unsupported endpoints, strict interior turning semantics, plateau handling and deliberate label corruption on authored arrays. Run the fixed audit twice, retain input/runtime/protocol/source identities, and independently check a fixed row per entry with a separate interpolation implementation. The eight groups remain consumed training/development data. Independent acceptance, target/projection annotations, verified alignment and model quality remain open.
