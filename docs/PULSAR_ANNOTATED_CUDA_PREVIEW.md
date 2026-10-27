# Annotated CUDA preview

Implementation status: implemented and authored native campaigns pass. Roadmap PARTIAL.

Source-bound annotated preview now admits explicit CPU or version-2 CUDA profiles. CUDA still requires the Linux CUDA-enabled Engine, owner policy, pinned runtime/model, supervised native capture, fresh interactive resource admission and private verified worker bootstrap. The preview retains the same exact target pixels, source-clock pair, immutable model contract and unqualified position semantics as CPU.

## Inference and no-inference evidence

A supported temporal pair produces exactly one real CUDA model run and the existing strict computation-provider trace. The parent independently validates pair support from original transferred pixels, binds preview context/clock/model/profile/view/runtime to the request, then checks that the original receipt and node trace report exactly one run.

First-frame, scene-cut and unsupported-clock pairs produce no estimate. Their native session still initializes with CPU fallback disabled and rechecks the selected device, but it never runs inference. A distinct `cuda-session-initialized-without-inference` receipt records zero successful runs and a schema-2 initialization-only trace summary. That summary claims no computation assignment. The bounded original trace must contain session initialization and only known session loading/initialization events; any node, model run, executor event or unknown event refuses.

The parent admits this receipt only through its annotated-preview gate after validating original pixels and absence of continuous pair support. Generation, historical/portable CUDA readers, live sessions and ordinary positive-run verification keep their existing positive-run requirement. The worker's generic session finish remains strict; its separate no-inference finalizer is reachable only from an annotated preview with no successful run. Failed/expired/preprocessing/decoding attempts cannot publish either successful receipt.

The worker protocol envelope and artifact-reference shape are unchanged. The new initialization summary is confined to read-only preview artifacts, with no authoritative generation attempt, candidate membership, storage migration or qualification claim. Old positive-run readers refuse the distinct evidence marker. Preview manifests are checked live but are not retained as authoritative jobs; this does not establish a new durable preview-evidence browsing API.

## Verification

The final default workspace passes 1,129 tests (63 ignored); the CUDA Engine suite passes 604 (68 ignored). Syntax, 288 architecture checks and 36 source guards pass. Negative cases distinguish initialized sessions from positive runs, refuse altered receipt/request/profile/device/summary identities, and prove the parent rejects a supported pair attempting to claim no inference. Extra annotated-preview runs refuse. The existing intermittent worker-fence lease reacquisition issue remains unresolved; these current suites pass without a retry.

The native CUDA campaign passes 21 previews: 18 supported pairs each retain exactly one model run and 12 CUDA-only computation nodes; first-frame, long-gap and scene-cut previews each retain zero runs and initialization-only original evidence. Fourteen exact previous/current RGB pairs, nonzero source origin, source-PTS local IPC, forward/reverse placements, restart, model drift and owner-approval withdrawal pass. No new authoritative generation jobs/attempts or master-motion changes occur. Maximum Python-reference position difference is 2.9802322387695312e-8.

CPU preview and CUDA generation/restart/portable-clone/export regressions pass. The initial generation regression correctly refused an offline profile demanding 512 MiB memory and 64 MiB output in interactive preview; the corrected test preserves that refusal and supplies a separate explicit interactive profile. The original failure and repeat are retained. Production admission limits were unchanged. The [source-bound checkpoint](evidence/roadmap-2026-10-09/annotated-cuda-preview/checkpoint.json) binds 406 source records and the compiled CUDA executable, with original preview receipts/traces retained. All private campaigns are terminal and worker custody is empty.

## Remaining qualification

Actual desktop CUDA preview interaction, sustained performance and the 200 ms obligation, useful held-out quality/real annotations, mixed-clock piecewise publication, installed Stash, broader concurrency/fault/retention and offline/platform qualification, physical Handy playback and human/release acceptance remain open. No training, live/sterile media access, user Stash contact or physical actuation occurred.

The subsequent [Linux desktop CUDA window checkpoint](PULSAR_DESKTOP_CUDA_PREVIEW.md) verifies authored file-model first-frame absence, supported pair rendering, restart and clearing, and corrects CPU-only presentation wording. Desktop generation/commit/export, selected-pack window workflows and sustained performance remain separate open gates.
