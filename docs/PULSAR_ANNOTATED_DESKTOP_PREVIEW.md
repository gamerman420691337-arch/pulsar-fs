# Annotated CPU Preview in the desktop

Status: desktop wiring implemented and software checks verified. Native window rendering and human usability remain open. Roadmap PARTIAL.

The desktop now sends the explicit source-bound annotated Preview request when model analysis uses an annotated CPU profile. File models and currently owner-selected packs use the same source/view and strict profile contract as the CLI. Load an annotated CPU profile and its model file, or select an approved annotated CPU pack; then load the target-view JSON in Generator. Seek in Studio or Cinema. Preview requires the loaded view to match the selected imported source. Placement seeks resolve project time through the active mapping.

The current target image appears in the main preview. The predecessor appears alongside its original source ordinal and PTS below the preview, with the optional unqualified normalized position. The pair stays in original source order even during reverse placement playback. The desktop uses source stride one. First-frame, cut and discontinuity absence remains visible as unavailable analysis; no neutral position or detector box is substituted. Detector profiles retain their existing preview path, and unannotated temporal profiles remain pixels-only.

## Presentation authority and freshness

The shared presentation state constructs the annotated command and binds the expected view, stride, source, placement mapping, seek generation and canonical profile. It rejects mismatched model/runtime/profile identities, missing annotations, changed pairing and qualified claims. File-model profile conflicts and wrong-source bindings refuse before enqueueing. Pack responses additionally pass the existing current-selection, model and profile checks. The Engine still owns source capture, model admission, inference and pack approval.

The RPC thread loads both raw images with the existing bounded regular-file/layout/digest checks. The UI accepts metadata only for the current request; a missing or invalid predecessor clears the result and both textures. Seeks, source/model/profile changes and loaded-target changes clear old presentation. No decoder, inference runtime, database authority or physical-device driver is added to the desktop.

## Verification scope

Three new ordinary tests check request choice retention, conflicting file profiles, changed identities/stride/seek, failed-request clearing and unavailable estimates replacing previous positions. An explicit native controller test replays 21 authored results through desktop admission and raw-pixel loading, including asymmetric placements, cut/gap/first-frame absence and explicit source PTS. It then uses the same command builder and production SessionClient to make 18 fresh file-model Preview calls against the isolated restarted Engine and checks positions, current pixels and acceptance. The separate shipped CLI/native campaign exercises pack and file models, both pixels, source clocks, owner withdrawal and installed model drift.

These are controller and native inference checks. They do not exercise real egui window rendering or qualify human usability, natural-media quality, platform distribution, physical Handy playback or release acceptance. No live/sterile input, fitting, external Stash service or physical device is used. See the [source-bound checkpoint](evidence/roadmap-2026-10-08/annotated-desktop-preview/checkpoint.json) and [underlying Preview contract](PULSAR_ANNOTATED_PREVIEW.md).

Final gates: 1,107 workspace tests pass with 63 ignored; the default binary builds; 286 architecture checks and 36 source guards pass. The authored native Preview and legacy selected-pack detector Preview campaigns pass. The explicitly enabled native desktop-controller test passes separately from the default ignored-test count.

## Actual Linux window workflow and target-pair layout

The [native window campaign](PULSAR_DESKTOP_PAIR_LAYOUT.md) now exercises authored project creation/reopen, media import, profile/view loading, first-frame absence, supported pair display and clear/reload through actual desktop controls. It repairs the predecessor thumbnail clipping found at default window size and verifies both images at default and minimum size with scrollable metadata. This adds limited window evidence to the earlier controller campaign; human usability, broader interactions and platform/release qualification remain open.

The subsequent [Linux desktop CUDA window checkpoint](PULSAR_DESKTOP_CUDA_PREVIEW.md) verifies authored file-model first-frame absence, supported pair rendering, restart and clearing, and corrects CPU-only presentation wording. Desktop generation/commit/export, selected-pack window workflows and sustained performance remain separate open gates.
