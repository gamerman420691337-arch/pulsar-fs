# Bounded visual tracking tiers

2026-10-11. Detector-based stroke generation has distinct Fast, Default and Maximum image-flow plans. Full M3 model/tier qualification remains open.

| Plan | Initial target/background points | Search | Sparse support recovery |
| --- | --- | --- | --- |
| Fast | 8 / 32 | Radius 6, checked single-scale | None |
| Default | 16 / 64 | Radius 6, checked pyramid | Up to 48 additional distinct target points when fewer than three are supported in a larger view |
| Maximum | 64 / 64 | Radius 8, checked pyramid | None; dense initial grid |

Default preserves the existing algorithm. Fast reduces point work and removes pyramid recovery; Maximum changes both initial target density and search range. More work is not proof of better output. Forward/backward cycles, texture/photometric limits, full-resolution refinement, minimum support, detection identity, scene cuts, background-relative separation and missing-evidence gaps remain mandatory. No missing result is promoted to observed motion or a measured pause.

The Engine already accepted Maximum, but its worker previously rejected it. Maximum now executes the visual detector pipeline. Maximum temporal or model-free generation still refuses explicitly; all-input tier coverage is unfinished. Legacy model-free and temporal Fast/Default paths retain their existing recipes and do not acquire a visual tracking-plan claim. Economy aliases Fast; quality aliases Maximum. Model and runtime selection remain explicit; no useful pretrained/default/bundled model selection follows from these algorithms.

Visual receipts add `visual_tracking_plan` with a versioned identifier, grids, radius and search mode. Default keeps `tracked-relative-image-flow-stroke-v4`; changed Fast/Maximum use v5. Existing per-sample point counts and flow summaries separately record executed work. The model/runtime/preprocessing and preset/settings identities remain captured in the immutable manifest and candidate lineage.

The [source-bound checkpoint](evidence/roadmap-2026-10-11/visual-tracking-tiers/checkpoint.json) records final gate results and distinct failed/limited attempts. Authored tests exercise actual Core search modes, exact tracking grids, static relative motion, flat evidence refusal and malformed buffers. Native verification requires actual confined model execution, candidate-bound receipt integrity, nonempty detections and recorded flow support for every plan, with master/revision unchanged.

This increment does not establish Default accuracy, a Maximum quality gain, Fast realtime performance, end-to-end speed, aggregate RAM/VRAM, GPU-vendor coverage, faithful six-axis projection, other-input model families, Core/Complete offline model defaults, platform/Handy or human/release acceptance. Native timing is post-acceptance wait/read elapsed; it excludes earlier request capture/admission and is not a complete performance benchmark. The public static image is an execution fixture, not an annotated motion-quality corpus. Training/sterile references and annotations are unchanged.


The final explicit native case passes in 2.53 seconds, using four repeated frames derived from the previously pinned public bus image. Each tier completes four CPU model runs, observes five boxes on every frame, and records three relative-flow samples. Fast/Default/Maximum use 8/16/64 target points and record component bounds 8.5/26/34 pixels, respectively. On this static fixture, 4/10/37 target points pass the unchanged checks and every inferred relative translation is zero. These support counts do not demonstrate a natural-media quality improvement. Each receipt matches exactly one identity in its published candidate lineage.

The earlier test-pattern clip completed all three models/plans but had no detections or flow samples; that passing limited run is retained separately and does not establish native tracking-plan execution. The first native attempt also failed by reading worker-local `lineage.json` after the Engine published it as `receipt.json`; the corrected test verifies the published candidate-bound bytes. Initial compilation hit the existing large receipt JSON macro's recursion limit; adding the new field after object construction repaired compilation without raising the crate-wide limit.


Final default Engine library: 816 PASS, 63 ignored. The explicit native flow case passes separately. Strict default/all-feature workspace all-target Clippy, formatting, 399 architecture checks and 36 guards pass. All-feature application/worker binaries build. Full protocol/client libraries, all-feature runtime suites, GUI and hardware qualification are not inferred from compilation. The retained all-feature companion is executed with an explicitly CPU inference profile.


A later [desktop/Stash admission increment](PULSAR_TIER_ADMISSION.md) exposes Maximum to desktop preparation and batch/migration requests and shares fresh execution checks with the worker. Unsupported requests refuse without creating generation jobs or replacing a retained migration ledger. The earlier native evidence above retains its own source/binary subject.


Learned CUDA execution update: [native visual tier evidence](PULSAR_LEARNED_CUDA_TIERS.md) verifies the pinned YOLO11 Fast plan against original CUDA artifacts and candidate lineage on the owner's RTX 4090. The missing convolution cuDNN components are explicitly pinned in the test policy; All three rebuilt scoped native checks pass, each with four learned inferences, original CUDA artifacts, candidate binding, unchanged master and terminal retirement. This is not model-quality, default-model, speed/resource, vendor, media, physical or human/release qualification. Full roadmap PARTIAL; goal active.
