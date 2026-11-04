# Desktop observed-target picking — 2026-10-09

Status: software implementation and scoped checks PASS; roadmap qualification PARTIAL.

The viewer previously required manually entering an original source-frame ordinal and normalized point. With a detector selected, **Pick observed target in preview** pauses playback and requests an ordinary detector preview. Click a unique observed box inside the actual letterboxed image, review the source ordinal/normalized point, then explicitly **Use picked tracking target**. This sets the existing source/view-bound `AnalysisTargetSeed` controls; generation still uses the established Engine contracts and requires explicit generation/candidate commit. Target-view drawing and target picking are mutually exclusive.

Picking uses the decoded frame association, actual source ordinal, selected view/coordinate map and artifact identity. Missing, overlapping, stale or unmapped boxes refuse. Predicted, inferred and synthesized boxes cannot become an observed target selection. Box containment uses the same half-open edge convention as the native tracker. Preview observations provide no detector class, so selection explicitly leaves the class filter unset; the UI explains how to add one manually. No class or semantic identity is guessed.

The entire pointer gesture is bound to one frame/artifact and image rectangle. Replacement pixels, seek/request changes or layout changes while the pointer is held reject the new click. Invalid clicks preserve an earlier valid draft; an earlier draft can survive a layout-only resize because its normalized point still refers to the same source pixels. Source/artifact/observation changes invalidate that draft. A yellow crosshair labeled **Target seed draft** remains distinct from detector overlays and does not activate generation settings without the explicit use action.

| Check | Result |
| --- | --- |
| Client locked/offline all-target Cargo check | PASS, no warnings |
| Picker geometry, freshness, declared view, actual pointer and whole-gesture tests | PASS: 5, zero ignored |
| Whole client library test binary | PASS: 146, zero failures, 3 default-ignored cases; 31.62 seconds under a 480-second deadline |
| Existing target-view authoring tests | PASS: 4 |
| Default Engine compilation | PASS; 27 existing warnings |
| Neighboring manual-seed tracker tests | PASS: 2 |
| Widget-produced JSON consumed by the actual production tracker | PASS: 1, explicitly run, zero ignored |
| Architecture/source guards | PASS: 299 / 36 |
| New module rustfmt check | PASS |

The actual egui pointer handler produces original source ordinal 4, normalized point (0.5, 0.5) and no class filter. The production tracker consumes that serialized fixture through the existing `AnalysisTargetSeed` schema, waits before ordinal 4, selects the clicked lower-confidence target over a higher-confidence decoy, retains unavailable gaps when only the decoy remains, reacquires the original track and invalidates selection at a cut. Initial and final widget output bytes are identical. This is a controller-to-kernel correspondence test using synthetic frame metadata and a synthetic texture; it is not a native decoder/model/whole-generation attempt or proof of real target identity.

The full client suite passes on this current source snapshot. The preceding checkpoint's 180-second timeout remains historical evidence and is not rewritten into a pass. No performance improvement or cause of that timeout is inferred from the present run. Three default-ignored client integration/process cases remain separate gates. The Engine build and three tracker tests do not establish whole-Engine/workspace/CUDA/release coverage.

Two intermediate failures are retained: the first direct client build used a cached serde variant without the required Arc serialization feature; the corrected dependency family compiles. An initial gesture test incorrectly expected an earlier valid draft to disappear on layout-only reflow; its corrected assertion verifies that the new click refuses while the prior draft survives. No production safeguard was weakened to obtain a pass.

No real media, live/sterile references, model inference, Stash server, secret or device was accessed. Isolated GUI workspace tools remain unavailable; no host desktop was used. Window rendering/usability, genuine source-matched target and script-timing annotations, useful model quality, full workspace/Engine/CUDA/release gates, runtime/crash/power-loss reliability, live Stash, platform/vendor matrices, Handy credentials/firmware/physical qualification and human acceptance remain open. Point selection is an explicit user intent; generation re-detects at the named frame and does not promise semantic identity or qualified motion.

[Retained evidence](evidence/roadmap-2026-10-09/desktop-target-picking/checkpoint.json) includes source, before/current diff, compile identities, complete default client output, failed/corrected tests, widget JSON and production tracker output. Against the previous checkpoint: 426 source records, three existing paths changed, one new module and 422 existing records unchanged. Engine production code remains unchanged; its added fixture consumer is test-only. No commit or push was made. Per-run scratch is removed after retention; shared build caches are preserved.
