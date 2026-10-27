# Explicit annotated training views

Pulsar can now stage a new training dataset from explicit eye/projection and static target-region declarations. It reuses the native view planner used by preview/generation, then crops an integer region in that resolved view and resizes to 64×64 with nearest sampling. Encoded orientation is preserved; display rotation is disabled. Source timestamps, reference offsets and label-support rules retain their existing meanings.

Saved desktop target-view sidecars can first be combined through the [checked saved-view intake bridge](PULSAR_SAVED_VIEW_TRAINING_INTAKE.md). It binds explicit manifest IDs, source hashes, probed encoded dimensions and native plans, and publishes a new aggregate document with provenance without inventing labels or replacing existing work.

The stage-only command creates a new private run and never fits or activates a model:

```sh
python3 scripts/stage-reference-views.py \
  --manifest PRIVATE_TRAINING_MANIFEST.json \
  --input-views PRIVATE_INPUT_VIEWS.json \
  --planner /absolute/path/to/pulsar \
  --output NEW_PRIVATE_RUN
```

The annotations document contains `schema_version: 1` and an `entries` array covering exactly the manifest's IDs. Each entry has `id`, `source_sha256` and `declaration`. A declaration has this shape:

```json
{
  "schema_version": 1,
  "source_dimensions": [192, 64],
  "analysis_dimensions": [96, 64],
  "video_view": {
    "layout": "side_by_side_left_right",
    "eye": "right",
    "projection": {"kind": "flat"}
  },
  "target_region": [8, 4, 32, 24],
  "target_id": "authored-target"
}
```

`source_dimensions` must match the probed encoded source. `analysis_dimensions` are maxima for flat views and the output size for spherical viewports, as in the existing native view adapter. The resolved size is returned by `pulsar training-input-plan JSON`. `target_region` is `[x,y,width,height]` in that resolved view before the final 64×64 resize. Region bounds, stereo dimensions, projection aspect and viewport validity are checked. Full/half equirectangular projection uses the existing `VideoView` viewport fields; fisheye/cubemap are not inferred.

Each annotation must match the snapshotted source digest. The private run retains exact annotation bytes, their hash, native planner identity, per-entry declarations/plans, clip RGB/PTS receipts and dataset identity. Annotated runs use schema-2 intake and the recipe `annotated-temporal-view-v1`. Existing whole-frame staging remains schema 1. Legacy model export refuses annotated intake. [Separate annotated model export](PULSAR_ANNOTATED_MODEL_EXPORT.md) is now implemented. Production profile/runtime/generation binding remains required; this staging feature does not make the saved whole-frame model target-aware.

Authored native tests cover declared flat stereo regions and full/half equirectangular eye selection, exact pixel values, unchanged source PTS and labels, source identity mismatch, output preservation and legacy-export refusal. Geometry tests reject malformed/unknown declarations, odd stereo dimensions and overflowing/out-of-view regions. Native planning does not create Engine state. See [the decision](../research/phase-b/ADR-B2-005-explicit-annotated-training-views.md) and [the evidence checkpoint](evidence/roadmap-2026-10-08/annotated-training-views/checkpoint.json) for the exact final gates and scope.

Final gates: 1,061 serial workspace tests pass (57 ignored), 79 CUDA-feature worker tests pass, the default binary builds, all 275 architecture checks and 36 source guards pass. The three new Python tests include both spherical subcases; eight legacy training and nine pixel/label regressions also pass. One native planning case and six direct CLI refusal cases pass without Engine state creation. An initial test-only Rust JSON assignment compile error is retained and corrected; it is not counted as a passing gate. Source archive/readback covers 376 records: 369 preceding records unchanged, four existing paths changed and three new paths.

The rectangle is a static declared region, not automatic tracking, independently verified anatomy or physical motion. No live annotations were invented and no live dataset was restaged for this checkpoint. No model fitting, sterile evaluation, activation or physical playback occurred. Useful generation quality, real annotations/alignment, dynamic target behavior, broader product/Stash closure, platform, Handy, human and release gates remain open. The roadmap remains PARTIAL.
