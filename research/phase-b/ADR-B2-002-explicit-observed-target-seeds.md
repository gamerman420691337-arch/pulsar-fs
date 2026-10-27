# ADR B2-002: explicit source-frame observed-target seeds

Status: implemented locally; natural-media, anatomy and physical qualification remain open.

## Requirement and decision

EDIT-03 requires overriding automatic target selection. Existing finite detector generation retains target identities but can choose the highest-confidence observed box when its previous target is unavailable. An explicit override must not silently substitute another detected entity.

Add an optional `AnalysisTargetSeed` to the existing immutable generation settings. It names one original source-frame ordinal, a normalized point in the decoded analysis view, and an optional declared detector class. At that exact sampled frame, select **exactly one** observed class-compatible box containing the point. Ambiguous/missing seeds are unavailable. Consume the seed once; never reuse its spatial location to select a later entity.

After selection, follow only the issued tracker identity. Missing observations remain gaps and clear flow continuity. A short absence can resume the same surviving observed identity with a reviewed synthetic anchor. An expired identity cannot be replaced by a new track at the same location. Cuts/context changes invalidate the override for the remainder of the attempt. The existing association uncertainty remains visible and does not become physical ground truth.

## Boundaries

The source artifact, view, model contract, placement, sampled ordinal and settings are part of the engine-captured request/configuration identity. Points use normalized analysis-view image edges, not original packed-source pixels or anatomy. A projected view has its own retained transform/map. Detector contracts and known selected source support are mandatory; global motion, temporal script-position regression, raw frame pairs and reverse placement refuse this declaration. The worker rejects an unsampled, out-of-range or trimmed-away source ordinal.

The CLI reads one bounded strict JSON declaration. Desktop form controls require binding to the current source/view and refuse a stale binding. Existing job authority, revision checks, candidate review, protected selective merge, durable history, confinement and dependency validation remain in force. The standalone CLI continues its existing generation/commit/export workflow; a gapped candidate does not authorize standard export.

This is finite generation support. In-attempt reselection, source-bound per-scene batch UI selection, assistant selection actions, rendered usability and independently qualified natural-media target identity remain owed. No whole B2/B4/Good-phase closure follows.

## Evidence and rollback

[The checkpoint](../../docs/evidence/roadmap-2026-10-01/target-seed/validation.json) binds exact source archives and local gate results. Pure tracker regressions cover lower-confidence override, decoys, missing observations, same-ID reappearance, expiration, exact one-shot timing, ambiguous/missing seeds and cuts. Protocol tests check optional legacy settings, point/class/lattice bounds and detector admission. A shipped native CLI scenario uses an authored two-box ONNX graph, independently known boxes/scores and stationary source frames; it checks retained configuration, differing selected track ordinals, missing-seed gaps/export refusal, unsupported source ordinals, unchanged input hashes and actual engine restart. These are wiring/identity fixtures, not trained perception or physical annotations.

Omit `target_seed` to retain the previous automatic policy. Existing captured attempts keep their original declarations; never rewrite history or convert missing support to a guessed replacement target.
