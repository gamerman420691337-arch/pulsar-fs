# Desktop target-view authoring — 2026-10-09

Status: software implemented and focused checks PASS; roadmap qualification remains PARTIAL.

The user confirmed there is no existing annotation manifest and requested continued software work. The desktop now offers **Draw target view in preview** after an explicit eye/projection declaration. Author mode pauses preview playback and requests ordinary pixels without model inference; it preserves the selected model/profile settings. Drag inside the actual letterboxed image, choose a lowercase token target ID, then explicitly **Use drawn target view** or **Save new drawn target view** at the entered target-view path. Loading existing declarations remains available.

The saved `AnnotatedGenerationView` carries the accepted source content hash, packed-source dimensions, selected eye/projection and an integer crop in the resolved native view. Reverse drags are supported. Out-of-image, nonfinite and zero-area rectangles refuse. A draft cannot be used or saved after its source, frame, request, view or artifact binding changes. A viewer layout change during a gesture cancels that gesture. Drafts do not silently replace the active target view.

Ordinary preview limits are now one shared protocol constant consumed by Engine and authoring. Declarations retain the original 640×480 maxima instead of fitting the displayed dimensions again. The actual native planner reproduces a 640×360 fit from a 1920×1081 flat source; using 640×360 as fresh maxima instead produces smaller dimensions and correctly refuses the full rectangle. This prevents authoring geometry from drifting through a second aspect-ratio fit. The protocol wire shape and ordinary preview values are unchanged.

Saving uses the existing descriptor-anchored publisher with exclusive staging and atomic no-overwrite publication. Regular destinations and symlinks refuse without modifying their bytes. Publication errors retain outcome/recovery information with target-view wording. The shared safe publisher currently supports Unix; non-Unix save refuses explicitly. In-memory authoring does not qualify other platforms.

| Check | Result |
| --- | --- |
| Client all-target Cargo check, locked/offline | PASS, no warnings |
| Actual egui pointer-event authoring, freshness/reflow, geometry and publication tests | PASS: 4, zero ignored |
| Default Engine compilation with aligned cached dependencies | PASS; 27 existing warnings |
| Engine native view-planner tests | PASS: 3, zero ignored |
| Widget-produced JSON through actual native planner/view adapter modules | PASS: exact right-eye crop and 96×64 resolved view |
| Flat aspect-ratio control, double-fit refusal and spherical selected-eye control | PASS |
| Architecture/source guards | PASS: 297 / 36 |
| New module rustfmt check | PASS |
| Whole client test suite | INCOMPLETE: `timeout 180s` returned exit 124 |

The whole client suite stopped at `evidence_recovery::tests::full_journal_preserves_unknown_requests_and_reclaims_only_acknowledged_entries`, without a failure assertion or final suite summary. It is neither a complete pass nor a demonstrated product failure. The focused final authoring run completed in 1.38 seconds; this is functional evidence, not a performance claim. An initial direct Engine build assembled incompatible cached serde variants and failed; the retained corrected build uses a consistent dependency family. Initial malformed spherical-control input and its explicit schema rejection are also retained.

The adapter harness imports the actual native planner/view modules, with verbatim numeric frame-size/fit helpers and a synthetic media-index shape. The full default Engine build and its three planner tests provide separate production compilation evidence. The authoring pointer tests use synthetic frame metadata and a synthetic content hash; they do not decode real media or demonstrate source/label alignment.

The isolated GUI tools were unavailable. No host desktop/window, live references, sterile media, device, secret or Stash server was accessed. Window rendering, human usability, genuine source-matched annotations, checked script timing, useful model quality, the incomplete whole-client suite, full workspace/CUDA/release, runtime reliability, live Stash, platform matrix and Handy/physical/human qualification remain open. A fixed rectangle is a user geometry declaration, not tracking, anatomical identification, timing evidence or physical ground truth.

[Retained evidence](evidence/roadmap-2026-10-09/desktop-target-view-authoring/checkpoint.json) includes source, before/current diff, build receipts, pointer tests, original/final widget JSON, native plans and the incomplete suite log. Inventory against the preceding checkpoint: 425 records, five existing source paths changed, one new source module and 419 existing records unchanged. No commit or push was made. Per-run scratch binaries/libraries are removed after the retained packet is sealed; reused project build caches are preserved.


Training intake follow-up: [checked saved-view assembly](PULSAR_SAVED_VIEW_TRAINING_INTAKE.md) consumes the saved wrapper through explicit manifest-ID assignments, verifies source identity and native planning, and retains exact bytes in a private aggregate intake. This does not change the drawing/saving UI or establish human annotation/alignment quality.
