# Linux runtime bundle checkpoint

Date: 2026-10-09
Status: Authored bundle lookup and repaired desktop preview PASS; roadmap PARTIAL.

## Assembly and selection

[Assembler](../scripts/assemble-linux-runtime-candidate.mjs) creates a new Linux x86-64 runtime candidate from the existing release executable and media build output. It validates executable architecture, rejects dynamically dependent media tools, checks the pinned source archive, includes source/licenses/configuration and hashes every copied file. Existing output, dynamic-tool and wrong-source cases each refuse before new output creation; an existing candidate manifest remains unchanged. The media build recipe now includes its source archive. Its compilation flags are unchanged; this source-inclusion edit is syntax checked, not a repeated compiler gate.

The existing Engine lookup already prefers `runtimes/ffmpeg` and `runtimes/ffprobe` beside the executable before PATH. No production Rust change was necessary. The candidate in [evidence](evidence/roadmap-2026-10-09/linux-runtime-bundle/runtime-candidate/) uses that path. The exact worker request independently confirms both bundled paths, without `PULSAR_FFMPEG` or `PULSAR_FFPROBE` overrides. GPU policy remains the explicitly selected diagnostic candidate; no owner configuration is changed.

## Native production workflow

The initial unchanged fixture fails with `Engine startup deadline`, before generation/database creation. Its fence remains version2 with no groups or retired records; it did not reach later staging/creation schema initialization. The repeat uses the same fixture and15-second startup threshold after the private desktop exits; it passes without skips in68.97s. No causal explanation or reliability repair is claimed for the first failure.

The repeat checks production generation, annotated preview, oversized-profile refusal, unchanged CUDA evidence after restart, portable clone import/export equality and clone reopening after another restart. It records27 successful CUDA runs,324 CUDA computation nodes and maximum reference position difference2.9802322387695312e-08. Original/clone exports match byte-for-byte. Optional preceding-reader rejection is not enabled. Final custody is empty. These are authored correspondence checks, not real model-quality or physical qualification.

## Actual desktop placement regression

A private KWin/Xwayland/DBus session runs this bundle with its own state and without media-tool overrides or CUDA policy. Actual UI controls create/import the authored four-second video and add source trim0.5–3.5s, source anchor0.5s, project anchor1s and speed2/1. After snapshot selection, the rebuilt window shows project1.000000s, source0.500000s, source frame4 and source PTS500 at time base1/1000. The former out-of-trim seek error is absent. Screenshots preserve controls before/after placement and the displayed decoded frame. Read-only database metadata independently shows project revision1. No model is selected; the window clearly reports that analysis is unavailable. This closes the sampled automatic seek/preview defect, not desktop CUDA placed-interval generation.

Initial display discovery reaches its15-second cutoff while the session is still starting. The same live session subsequently supplies the window; it is not restarted on observation timeout. A later CLI snapshot check also times out at15 seconds and does not establish snapshot correspondence. Engine/GUI processes are stopped only within this owned session, and the compositor returns143 after termination. Final GUI custody is empty.

## Evidence and remaining work

Evidence binds417 source records:415 unchanged, the media builder source-inclusion edit and one added assembler. Production Rust and the retained release supplier remain unchanged. Architecture/source guards pass289/36; assembler/builder syntax and diff whitespace checks pass. Assembly manifest identities are independently rehashed after retention. Source snapshot, bundle, screenshots, exact commands, successful receipts and startup failure are retained; private databases/tokens are excluded. Scratch/build/campaign/session roots are removed after terminal handles and no scratch executable/working-directory references are confirmed.

Core/Complete model and assistant packaging, full media/platform matrices, startup reliability, actual desktop CUDA placement/interval workflow, cold live readiness/reuse, real Stash/media/model validation, Handy and human acceptance remain open.


## Dedicated worker assembly repair — 2026-10-11

Current launches select a dedicated sibling `pulsar-worker`; the historical assembler copied only `pulsar`. The assembler now requires that companion as a regular executable x86-64 ELF input, copies it with executable permissions and includes its exact hash/length in the manifest. Static media checks and pinned archive verification now use the updated input positions. Existing output is still never overwritten. This repairs the current assembler; the historical retained bundle is not silently updated.

The same explicit current application/worker and retained media inputs produce two failing predecessor tests (missing manifest worker and missing-companion acceptance) and two passing corrected tests. The corrected tests independently rehash all eight output records, check executable worker permissions, preserve existing output and refuse missing workers/dynamic media before output creation. These tests run in a private one-GiB tmpfs namespace. An ordinary-storage predecessor assembly times out at60 seconds with incomplete output; a premature manifest probe also fails because assembly was still pending. Both are retained separately from the verified defect, and do not establish storage repair.

All preceding370 Rust/config/source hashes and supplier binaries remain unchanged. No Rust rebuild/lint/library rerun is needed for this script-only repair; script syntax, architecture376 and source guards36 pass. The [checkpoint](evidence/roadmap-2026-10-11/worker-runtime-bundle/checkpoint.json) retains exact before/after scripts, regression output and supplier identities. Test bundles and incomplete ordinary output are removed; no new durable distribution artifact is claimed. Actual Core/Complete models/assistant/defaults, offline extraction, OS/vendor, storage reliability, media/device and human/release gates remain open. Full roadmap PARTIAL.


## Optional static assistant candidate — 2026-10-11

[Assistant packaging](PULSAR_STATIC_ASSISTANT_BUNDLE.md) extends the assembler with an optional bounded, exact-identity declaration and four assistant payloads. An actual twelve-file bundle is retained separately from historical evidence. Relocated copied-CLI offline host setup and refusal tests PASS2; the three-argument worker/media tests PASS2. Independent retained-file hashing and final manifest correspondence pass. The static runtime has no dynamic entries, but its ELF requires x86-64-v3 and records Linux6.1.0; no broader portability, license qualification, useful model selection or Core/Complete completion is claimed.


## CPU runtime discovery and packaging — 2026-10-11

[Bundled CPU runtime](PULSAR_BUNDLED_CPU_RUNTIME.md) adds independent optional exact-pinned runtime/license/notices assembly and executable-relative inference discovery. The actual fifteen-file combined candidate supports copied relocated CLI/Engine/worker authored generation and explicitly selected offline packs without runtime/media overrides. Discovery does not activate a model or load inference; manifests retain unqualified status and actual standard-system dependency names. Useful generation model assets, edition/clean-machine/platform/license qualification and the remaining roadmap gates are open.
