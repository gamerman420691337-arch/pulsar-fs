# Saved desktop views into training intake

The desktop's saved target-view sidecars can now be bound to exact training manifest entries without hand-editing a second annotations document. Supply one explicit `ID=absolute-sidecar-path` assignment for every training entry:

```sh
timeout 300 python3 scripts/assemble-reference-views.py \
  --manifest /absolute/private/training-manifest.json \
  --view one=/absolute/private/one-target-view.json \
  --view two=/absolute/private/two-target-view.json \
  --planner /absolute/path/to/pulsar \
  --ffprobe /absolute/path/to/ffprobe \
  --output /absolute/private/new-view-intake
```

The manifest must retain the existing training-only `Live-Reference` admission and explicit reference clock declarations. The bridge accepts the exact saved `AnnotatedGenerationView` wrapper. It refuses missing, extra or duplicate assignments, duplicate/non-finite JSON, symbolic-link sidecars, stale source digests, changed inputs and incompatible source dimensions. It hashes each bounded source descriptor before and after checking, probes that same inherited read-only descriptor, and verifies the declaration through the actual native `training-input-plan` command. Native planner/probe identities are checked again before publication.

The new directory contains exact `manifest.json` and saved `ID.view.json` bytes, aggregate `input-views.json` and a `receipt.json` binding their hashes to native plans and source bytes. It does not copy media, generate labels, train, activate a model or send data anywhere. The Linux publisher uses private descriptor-anchored staging, fsync and atomic no-replace publication. Existing files, directories and symlink destinations remain unchanged. Unsupported publication platforms refuse explicitly; an error after successful publication retains the output for inspection rather than deleting it.

Use the assembled view document with the existing stage-only path:

```sh
python3 scripts/stage-reference-views.py \
  --manifest /absolute/private/new-view-intake/manifest.json \
  --input-views /absolute/private/new-view-intake/input-views.json \
  --planner /absolute/path/to/pulsar \
  --output /absolute/private/new-staged-run
```

Staging independently snapshots and verifies source identity again. Assembly preserves reference offsets; it cannot establish human visual/script timing or anatomical correctness. Receipts keep `qualified: false` and explicitly unverified human target/alignment status. Static regions do not establish dynamic target tracking. The user has supplied no real annotations, and none were invented or used for fitting.

Exact native test subjects, refusal results, independently predicted fixture labels and unchanged source PTS are recorded in `evidence/roadmap-2026-10-11/saved-view-intake/checkpoint.json`. Existing staging, pixel/label and bounded-process regressions remain separate. No window rendering, native GUI usability, real target/model quality, sterile evaluation, physical Handy behavior or platform/release acceptance is inferred. Full roadmap remains PARTIAL.
