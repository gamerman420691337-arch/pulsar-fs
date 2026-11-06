# Manual observed-target selection

Finite detector generation accepts an explicit initial target seed. It overrides confidence-based selection and follows only the selected tracker identity. This does not establish anatomical correctness or qualified stroke motion.

## Declare and generate

Create a bounded JSON file:

```json
{"source_frame_index":0,"x":0.3,"y":0.5,"class_id":0}
```

Then use the same declared detector/model profile as ordinary generation:

```sh
pulsar generate VIDEO --model MODEL.onnx --inference-profile PROFILE.json --target-seed TARGET.json --output OUTPUT.funscript
```

`source_frame_index` is an original indexed source ordinal, not seconds or an index within a trimmed clip. `x` and `y` are normalized coordinates in the decoded analysis view: finite values in `[0,1)`. A selected-eye or spherical view uses its transformed analysis image, not packed-source coordinates. `class_id` may be `null` to allow any class; a provided class must fit the declared model contract. The settings and exact source/model/runtime/view dependencies are retained by the engine.

Desktop generation provides original-frame, point and optional class controls. Bind the declaration to the current source and view before generation. A different source/view requires rebinding. The existing preview can show actual detector observations; these form controls do not create decorative boxes or infer a selected entity without observations. Compilation and CLI checks do not establish rendered usability.

At the exact selected frame, the point must be inside exactly one observed class-compatible box. No match or multiple matches consume the seed and leave unavailable motion. The point is never reused to choose a later target. Frames before a later seed remain unavailable. Standard export refuses unresolved gaps; there is no implied partial export.

## Identity and support

Following the seed, only observations with the selected issued track identity can contribute. Missing observations clear flow continuity and remain gaps. Reappearance of the same surviving identity resumes with a reviewed synthetic anchor. Track expiration, cut/context invalidation or a new entity at the same location does not authorize automatic replacement. Existing ambiguity, flow support, confidence and calibration reviews remain visible.

A seed requires an indexed-video detector. Global translation, temporal script-position regression and raw frame-pair recipes refuse it. The frame must lie on the sampling lattice and inside actual selected decoded support. Forward source placement can scope the candidate; reverse placement is currently unsupported for this policy. The authenticated project API accepts the same settings and returns an uncommitted candidate; selective merge and protection retain their existing semantics. The standalone generation CLI retains its existing explicit commit/export workflow.

The optional field is omitted for legacy automatic selection. It does not alter the device-neutral master during candidate generation or authorize physical playback. In-attempt reselection, richer target interactions, source-bound per-scene batch UI declarations, assistant actions and full natural-media/model/device/platform/human/release qualification remain open.

## Evidence

[The checkpoint](evidence/roadmap-2026-10-01/target-seed/validation.json) records exact source identities, local gates and the authored native trace. Tracker tests cover lower-confidence selection, decoys, missing observations, surviving-ID reacquisition, expired-ID refusal, delayed/ambiguous/missing seeds and cuts. Strict protocol tests cover point/class/lattice bounds and omitted fields. Native CLI generation with an authored two-box model checks the selected target ordinal, retained settings/receipt, missing-seed gaps/export refusal, outside-support failure, unchanged source/model hashes and actual restart. Authored model outputs are application wiring evidence, not pretrained quality or independent physical target annotations.

See [ADR B2-002](../research/phase-b/ADR-B2-002-explicit-observed-target-seeds.md).
