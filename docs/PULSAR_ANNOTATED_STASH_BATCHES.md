# Source-bound annotated Stash batches

Stash batches now accept a per-source annotated-view catalog with an explicit file-backed CPU or owner-authorized CUDA profile. Each entry binds exact Stash scene/file IDs to an `AnnotatedGenerationView`, including source SHA-256, eye/projection, encoded/intermediate dimensions, target region and target ID. The shared settings are a template with no single-source `annotated_view`. Before dispatch, the scheduler resolves an ordinary validated generation setting from the exact catalog entry and retained source capture. A missing key, changed source hash or mismatched captured identity cannot silently use another view.

The catalog is bounded to 100 unique scene/file keys. Admission requires coverage for every eligible selected source and refuses entries for unselected or ineligible sources before model snapshotting. Shared placements and project intervals refuse; the separate [per-source source-plan catalog](PULSAR_STASH_SOURCE_PLANS.md) now supplies explicit placement/interval intent and source-time publication with matching scene/file keys and source hashes. Omitting source plans retains unplaced full-source generation. [Owner-selected annotated model packs](PULSAR_ANNOTATED_STASH_PACK_BATCHES.md) use the same catalogs; [annotated CUDA file-model batches](PULSAR_ANNOTATED_CUDA_STASH_BATCHES.md) now have native evidence. [Selected annotated CUDA packs, including placed/interval batches](PULSAR_ANNOTATED_CUDA_PACK_BATCHES.md), now have authored native evidence. [File-model CUDA placed/interval batches](PULSAR_ANNOTATED_CUDA_STASH_BATCHES.md) now also have authored native evidence. Ordinary generation validation still requires its explicit single-source binding.

## First-party controls

```sh
pulsar stash generate-missing --url http://127.0.0.1:9999/graphql \
  --mappings MAPPINGS.json --model MODEL.onnx \
  --inference-profile PROFILE.json --annotated-views VIEWS.json \
  --width 64 --height 64 --request-id BATCH_ID --detach
```

`VIEWS.json` is an array:

```json
[
  {
    "scene_id": "SCENE_ID",
    "file_id": "FILE_ID",
    "view": {
      "source_sha256": "EXACT_SHA256_OF_SELECTED_SOURCE",
      "declaration": {
        "schema_version": 1,
        "source_dimensions": [192, 64],
        "analysis_dimensions": [96, 64],
        "target_region": [16, 0, 64, 64],
        "target_id": "EXPLICIT_TARGET_ID",
        "video_view": {
          "layout": "side_by_side_left_right",
          "eye": "right",
          "projection": {"kind": "flat"}
        }
      }
    }
  }
]
```

The example identity strings are placeholders; actual source hashes and declarations are mandatory. CLI grammar requires model/profile and refuses a competing global video view or pack. The catalog loader uses the existing 64 KiB, no-follow, regular-file JSON boundary. Desktop exposes an optional catalog path beside folder mappings and submits the same typed contract. Actual desktop interaction and usability remain unqualified.

## Durable intent and migration

Annotated intent uses batch report/ledger version 5 and atomically requires at least the existing schema-13 pre-recovery ledger reader. Existing schema-23 annotated jobs and the schema-24 portable wrapper are preserved. Preceding readers reject the unknown catalog or report version through their read-only batch validation. Current startup detects a downgraded database header when version-5/catalog metadata requires that validator. Empty legacy catalogs omit the new wire field.

Replacement generation intent includes the complete replacement catalog. Migration remains owner-authorized, bound to the observed ledger digest and paused until a separate resume. It retains previous options/catalogs and accepted jobs; it does not rewrite their immutable settings. Historical annotated catalogs preserve the version-5 reader requirement even if future generation changes to a legacy profile. Future captured sources must match replacement bindings before the migration can commit. Existing request identities, grants, model content checks and uncertain external effects remain governed by the batch ledger.

## Evidence and limits

The authored native campaign uses the shipped binary, actual confined CPU workers, local IPC and its own loopback Stash fixture. Two different encoded sources use different eye/target declarations. Both publish 28 actions; all 28 intended unrounded estimates across those two jobs match the independent Python model fixture exactly. Their schema-3 receipts retain the corresponding native declarations. Restart after a durable generation request preserves one generation request per source; terminal batch replay preserves outcomes.

A third source deliberately receives a wrong hash. It stops in `needs_attention` before a generation request or job and writes no sibling script. Stale-ledger migration refuses. Correct replacement intent stays paused through restart and exact migration replay, then explicit resume generates and publishes successfully. Its previous bad catalog remains in migration history. Across three completed jobs, all 42 intended unrounded Python estimates match exactly and all 42 rounded script positions agree. No remote mutation is requested by this campaign.

An actual preceding executable refuses the new ledger with observed database/WAL bytes unchanged. This is not proof that all earlier startup custody/filesystem preparation was unchanged. Transaction rollback, header/report downgrade refusal, missing/duplicate/global-view/provider refusal, source substitution and legacy omission have focused coverage. Initial failures were fixture errors: default 320x180 analysis in a 64x64 temporal test and treating the batch-status response envelope as a bare report; the strict production checks were preserved.

Final checks: 1,098 workspace tests pass (62 ignored), the native annotated batch/migration campaign passes, and four preceding native Stash campaigns pass. The default binary builds, 284 architecture checks and 36 source guards pass. See [ADR B3-076](../research/phase-b/ADR-B3-076-annotated-stash-batches.md) and the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-stash-batches/checkpoint.json).

This is authored integration, not learned model quality or installed Stash qualification. No fitting, live/sterile input or physical playback occurred. Annotated CUDA preview/desktop workflows, mixed-clock publication without piecewise proof, broader fault/retention campaigns, actual desktop usability, visual/script alignment and useful held-out quality, platform/offline distribution, physical Handy, and human/release acceptance remain open. Roadmap status: PARTIAL.
