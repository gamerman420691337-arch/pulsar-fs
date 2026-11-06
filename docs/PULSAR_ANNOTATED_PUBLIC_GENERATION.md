# Public source-bound annotated generation

Direct file-backed CPU generation now admits an explicit annotated temporal profile and source-content view binding. Before snapshotting the supplied model, the Engine requires an imported media source, current project revision, matching source SHA-256 and an explicit CPU model/profile. Worker preparation still validates profile/runtime/model pins, the active placement and full immutable request; it retains schema-23 annotated membership in the job transaction. Completion validates schema-3 receipt/model correspondence before publishing a candidate. Output remains unqualified.

The CLI exposes a dedicated profile declaration and target-view input:

```sh
pulsar model profile MODEL.onnx --annotated-temporal-position \
  --width 64 --height 64 --class-count 1 > PROFILE.json
pulsar generate SOURCE.mkv --model MODEL.onnx \
  --inference-profile PROFILE.json --annotated-view VIEW.json \
  --output NEW_OUTPUT.funscript
```

`VIEW.json` contains `source_sha256` and an explicit `declaration`: schema version, encoded source dimensions, intermediate analysis dimensions, video view, integer target region and target ID. It uses the [shared annotated declaration](../crates/pulsar-protocol/src/temporal_recipe.rs). It does not infer annotations from model metadata. The bounded loader refuses unknown fields, excessive bytes, symlinks and nonregular files. CLI grammar requires the profile/model and refuses competing raw, tracking-target, video-view or model-pack inputs.

Desktop has load/clear controls for that same declaration and includes the retained binding in generation settings. Existing settings validation rejects incompatible profiles or competing declarations; changing source cannot silently rebind the target. These controls compile and use the shared loader, but an actual desktop window and human usability have not been qualified.

The authored public Engine campaign submits to a real confined worker, replays the same request without a new attempt, waits for completion, commits, restarts with the original persisted session, queries the completed job and uses production capture to create format-11 bytes. Actual clone import, restart, archived-clock status, re-export and second clone then pass. The original authored frame/model comparison and source-clock corruption checks remain in that campaign. A changed source hash refuses before model snapshotting. Initial harness failures assumed source import advanced motion revision, used a newly paired session without project grants, and retained one stale capture revision; those fixtures were corrected without weakening authorization or revision checks.

A separate shipped CLI campaign uses a private Engine over local IPC, the current binary and pinned native CPU runtime. Profile creation, generation, commit and funscript export pass. All 14 independent Python positions match the retained unrounded worker program exactly (maximum difference 0); all 14 rounded exported positions agree. The exact source-bound settings, 28 decoded samples, native-plan declaration, schema-3 receipt, schema-23 membership and unqualified status are read back. The owned server is terminal after the campaign. This is authored correspondence, not natural-media quality.

Generic Stash batches, model packs and annotated CUDA admission remain guarded. This change does not qualify them or automatically activate a model. Real target annotations, visual/script alignment, useful learned quality, broad fault/retry campaigns, Stash/product closure, platform/offline packaging, physical Handy playback and human/release acceptance remain open. No fitting, live/sterile input use or physical actuation occurred.

See [ADR B2-010](../research/phase-b/ADR-B2-010-public-annotated-generation.md) and the [checkpoint](evidence/roadmap-2026-10-08/annotated-public-generation/checkpoint.json). Roadmap status: PARTIAL.

Final checks: 1,092 workspace tests pass (62 ignored), 25 CUDA-feature annotated tests pass (five native cases ignored by default), and fresh public Engine and original file-worker native cases pass separately. The default binary builds; 283 architecture checks and 36 source guards pass. The extended native test and new Engine harness pass formatting checks. The shipped CLI also passes after restarting its private Engine with the final binary hashed before and after execution. All 387 source records match the saved archive: 380 preceding unchanged, six existing paths changed and one added.
