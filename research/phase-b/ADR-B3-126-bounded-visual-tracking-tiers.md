# ADR-B3-126: distinct bounded visual tracking plans

Status: implemented software increment; tier/model/release qualification remains open.

The Engine admitted maximum/quality generation, but the worker rejected them. Accepted fast/default/economy names selected identical tracking work. Resolve visual detector generation to explicit bounded plans rather than treating the label as evidence of a different pipeline.

Fast uses eight target and 32 background points with full-resolution forward/backward checking and no pyramid or sparse-grid recovery. Default preserves the existing 16/64 grids, radius six, checked pyramid and conditional 48-point additional grid. Maximum uses 64/64 grids, radius eight and the existing coarse-proposal/full-resolution refinement checks. Thresholds, minimum supported points, missing-evidence gaps, target identity and camera-relative motion checks remain unchanged. Economy aliases Fast and quality aliases Maximum; aliases do not establish additional tiers.

Receipts disclose the resolved visual_tracking_plan. Default retains the v4 recipe identifier; changed Fast/Maximum tracking uses v5. Existing per-sample search summaries and point counts describe executed work separately from the selected plan. Supplied model/runtime/preprocessing identities stay explicit and unchanged; these plans do not choose bundled models or fabricate a model family. Unsupported Maximum temporal/model-free generation refuses instead of substituting another recipe.

Verify bounded Core admission, actual grid/search execution, flat-image refusal and static relative-motion consistency. A separate explicit owned native case must complete each tier using the same pinned eligible detector/runtime, bind each receipt to the published candidate, observe nonempty detections and effective flow search/grid, and preserve master motion. One static fixture does not demonstrate better tracking or useful Maximum quality. Paired natural-media quality, timing, resources, all input families, GPU vendors, editions and physical/human acceptance remain independent.

See [operation scope and evidence](../../docs/PULSAR_VISUAL_TRACKING_TIERS.md).
