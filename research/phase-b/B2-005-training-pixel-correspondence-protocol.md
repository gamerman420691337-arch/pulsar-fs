# B2-005: Existing training pixel and source-clock correspondence

Status: Frozen before measurement; diagnostic only.
Date: 2026-10-08

Use only the hash-bound eight live-reference training snapshots and original intake/manifest in `.pulsar-reference-evidence/live-position-training-2026-10-04/`, and the consumed dataset in `.pulsar-reference-evidence/live-position-holdout-2026-10-04-retry/`. No sterile input, fitting, model selection/activation, label/offset edits or source rewriting.

For all 32 saved clips, independently construct a bounded FFmpeg command from validated numeric intake fields, using stride 4, nearest 64×64 RGB packed-source decoding and disabled display autorotation. Never execute saved command arrays. Use a single native decode with tee raw RGB and SHA-256 framehash outputs; independently parse framehash clock, PTS, byte lengths and hashes against each delivered RGB frame. Compare the complete PTS sequence and raw RGB hash against the saved clip receipt. Compare every accepted previous/current RGB pair, in declared NCHW order, against the stored uint8 tensor and its ordered entry ID and PTS pair. Apply the existing declared supported-label/pair-gap admission only to reconstruct omissions; this audit does not requalify label interpolation.

Verify original manifest/intake/dataset bindings, all source and reference hashes, source sizes and finite bounded windows before decoding. Bound native process time/output and array allocation, prohibit pickle, reject escaping/symlink snapshot paths and malformed receipts. Rehash inputs after processing. Emit only hashes, counts and pass/failure information; no media or tensor bytes in public evidence. Stop rather than repair on identity or decode failure. A mismatch is negative evidence, never permission to overwrite a dataset. Do not change preprocessing or thresholds in response to results.

Authored regressions must detect corrupted frame payloads, incorrect PTS, reversed temporal/channel order and changed snapshot identity. A successful audit proves correspondence for these saved samples and current native decoder only; it does not prove human alignment, target/anatomical identity, adequate resolution/cadence, generalization or release quality. Retain source/tool/runtime/protocol identities and terminal results. The eight source groups remain consumed training/development data.
