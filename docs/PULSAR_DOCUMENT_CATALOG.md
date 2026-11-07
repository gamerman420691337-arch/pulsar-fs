# Pulsar document catalog

Consolidation inventory: 2026-11-06. Start with the [project specification](PULSAR_PROJECT.md), which owns feature scope, stack, API and delivery order. This catalog makes preserved work discoverable; it is not another roadmap or acceptance ledger.

Coverage: every tracked Markdown/reStructuredText/AsciiDoc document in the repository, plus all tracked files under `docs/`, `research/` and `assurance/`, including new consolidation records. Ignored/private evidence, data and build caches remain separately preserved. Asset links retain their original paths.

Classification routes readers; it does not accept proposals or erase confirmed technical obligations. Historical means scheduling/status is superseded, not that source or counterevidence is disposable. Family tags are discovery hints; the canonical feature catalog defines scope. Legacy implementation references are excluded from the production dependency graph.

Maintain this index when adding or relocating documentation. Use `git ls-files` and `rg --files docs research assurance` to find current records. This inventory is not an additional build/commit gate.

## Inventory summary

| Role | Records |
| --- | --- |
| Canonical project specification | 1 |
| Current execution data | 1 |
| Generated execution views | 2 |
| Entry points, usage and integration register | 8 |
| Supporting contracts, research and technical notes | 867 |
| Historical plans and observations | 46 |
| Supporting assets, models, schemas and receipts | 118 |

**Total: 1043 records.**

## Feature navigation

Use the canonical [feature catalog](PULSAR_PROJECT.md#one-feature-set), [API contract](PULSAR_PROJECT.md#one-ergonomic-api-and-one-runtime-owner), [delivery roadmap](PULSAR_PROJECT.md#delivery-roadmap) and [branch/worktree register](PULSAR_CONSOLIDATION.md). Search this page by family or original filename for detailed contracts and source findings.

## Canonical project specification

| Document / original path | Feature family |
| --- | --- |
| [Pulsar: product specification and execution roadmap](../docs/PULSAR_PROJECT.md)<br>`docs/PULSAR_PROJECT.md` | Projects and interoperability |

## Current execution data

| Document / original path | Feature family |
| --- | --- |
| [PULSAR_EXECUTION.json](../docs/PULSAR_EXECUTION.json)<br>`docs/PULSAR_EXECUTION.json` | Cross-project or reference |

## Generated execution views

| Document / original path | Feature family |
| --- | --- |
| [Pulsar current status](../docs/PULSAR_CURRENT_STATUS.md)<br>`docs/PULSAR_CURRENT_STATUS.md` | Cross-project or reference |
| [Pulsar execution dashboard](../docs/PULSAR_MILESTONES.md)<br>`docs/PULSAR_MILESTONES.md` | Cross-project or reference |

## Entry points, usage and integration register

| Document / original path | Feature family |
| --- | --- |
| [Pulsar work routing](../AGENTS.md)<br>`AGENTS.md` | Cross-project or reference |
| [Pulsar](../CONTEXT.md)<br>`CONTEXT.md` | Cross-project or reference |
| [⚡ Pulsar — Neural Kinematic Workstation & Funscript Generator](../README.md)<br>`README.md` | Cross-project or reference |
| [Use the current Pulsar app](../docs/PULSAR_APP_QUICKSTART.md)<br>`docs/PULSAR_APP_QUICKSTART.md` | Cross-project or reference |
| [Pulsar consolidation register](../docs/PULSAR_CONSOLIDATION.md)<br>`docs/PULSAR_CONSOLIDATION.md` | Cross-project or reference |
| [Pulsar document catalog](../docs/PULSAR_DOCUMENT_CATALOG.md)<br>`docs/PULSAR_DOCUMENT_CATALOG.md` | Cross-project or reference |
| [Pulsar software specification — consolidated](../docs/PULSAR_SOFTWARE_SPECIFICATION.md)<br>`docs/PULSAR_SOFTWARE_SPECIFICATION.md` | Cross-project or reference |
| [Pulsar documentation](../docs/README.md)<br>`docs/README.md` | Cross-project or reference |

## Supporting contracts, research and technical notes

| Document / original path | Feature family |
| --- | --- |
| [Motion Transfer Model and Implementation Correspondence](../assurance/MOTION_TRANSFERS.md)<br>`assurance/MOTION_TRANSFERS.md` | Models and generation |
| [Portable export lifecycle model](../assurance/PulsarPackageExport.md)<br>`assurance/PulsarPackageExport.md` | Projects and interoperability |
| [Clone import lifecycle model and correspondence](../assurance/PulsarPackageImport.md)<br>`assurance/PulsarPackageImport.md` | Projects and interoperability |
| [Phase A lifecycle model and correspondence obligations](../assurance/README.md)<br>`assurance/README.md` | Architecture and assurance |
| [WASM start-resolution model and correspondence](../assurance/WASM_START_RESOLUTION.md)<br>`assurance/WASM_START_RESOLUTION.md` | Assistant and extensions |
| [Pulsar clients](../crates/pulsar-clients/README.md)<br>`crates/pulsar-clients/README.md` | Cross-project or reference |
| [Authored H.264 orientation fixture](../crates/pulsar-engine/tests/fixtures/rotation-h264.md)<br>`crates/pulsar-engine/tests/fixtures/rotation-h264.md` | Cross-project or reference |
| [Pulsar local protocol v1](../crates/pulsar-protocol/PROTOCOL.md)<br>`crates/pulsar-protocol/PROTOCOL.md` | Architecture and assurance |
| [Engine owner death during admitted creation](../docs/PULSAR_ADMITTED_ENGINE_CRASH.md)<br>`docs/PULSAR_ADMITTED_ENGINE_CRASH.md` | Cross-project or reference |
| [Generation analysis dimensions](../docs/PULSAR_ANALYSIS_DIMENSIONS.md)<br>`docs/PULSAR_ANALYSIS_DIMENSIONS.md` | Cross-project or reference |
| [Annotated file generation on CUDA](../docs/PULSAR_ANNOTATED_CUDA_GENERATION.md)<br>`docs/PULSAR_ANNOTATED_CUDA_GENERATION.md` | Models and generation |
| [Annotated CUDA model-pack Stash batches](../docs/PULSAR_ANNOTATED_CUDA_PACK_BATCHES.md)<br>`docs/PULSAR_ANNOTATED_CUDA_PACK_BATCHES.md` | Library and Stash |
| [Annotated CUDA preview](../docs/PULSAR_ANNOTATED_CUDA_PREVIEW.md)<br>`docs/PULSAR_ANNOTATED_CUDA_PREVIEW.md` | Models and generation |
| [Annotated CUDA Stash batches](../docs/PULSAR_ANNOTATED_CUDA_STASH_BATCHES.md)<br>`docs/PULSAR_ANNOTATED_CUDA_STASH_BATCHES.md` | Library and Stash |
| [Annotated CPU Preview in the desktop](../docs/PULSAR_ANNOTATED_DESKTOP_PREVIEW.md)<br>`docs/PULSAR_ANNOTATED_DESKTOP_PREVIEW.md` | Multimodal inputs |
| [Source-bound annotated file generation worker](../docs/PULSAR_ANNOTATED_FILE_WORKER.md)<br>`docs/PULSAR_ANNOTATED_FILE_WORKER.md` | Cross-project or reference |
| [Annotated generation receipt correspondence](../docs/PULSAR_ANNOTATED_GENERATION_RECEIPTS.md)<br>`docs/PULSAR_ANNOTATED_GENERATION_RECEIPTS.md` | Models and generation |
| [Annotated generation storage and reader barrier](../docs/PULSAR_ANNOTATED_GENERATION_STORAGE.md)<br>`docs/PULSAR_ANNOTATED_GENERATION_STORAGE.md` | Models and generation |
| [Separately bound annotated temporal model export](../docs/PULSAR_ANNOTATED_MODEL_EXPORT.md)<br>`docs/PULSAR_ANNOTATED_MODEL_EXPORT.md` | Projects and interoperability |
| [Owner-selected annotated CPU model packs](../docs/PULSAR_ANNOTATED_MODEL_PACKS.md)<br>`docs/PULSAR_ANNOTATED_MODEL_PACKS.md` | Models and generation |
| [Native annotated model binding](../docs/PULSAR_ANNOTATED_NATIVE_BINDING.md)<br>`docs/PULSAR_ANNOTATED_NATIVE_BINDING.md` | Cross-project or reference |
| [Portable annotated generation and source clocks](../docs/PULSAR_ANNOTATED_PORTABLE_GENERATION.md)<br>`docs/PULSAR_ANNOTATED_PORTABLE_GENERATION.md` | Projects and interoperability |
| [Source-bound annotated CPU Preview](../docs/PULSAR_ANNOTATED_PREVIEW.md)<br>`docs/PULSAR_ANNOTATED_PREVIEW.md` | Multimodal inputs |
| [Annotated preview capture deadline diagnosis](../docs/PULSAR_ANNOTATED_PREVIEW_DEADLINE_DIAGNOSIS.md)<br>`docs/PULSAR_ANNOTATED_PREVIEW_DEADLINE_DIAGNOSIS.md` | Multimodal inputs |
| [Public source-bound annotated generation](../docs/PULSAR_ANNOTATED_PUBLIC_GENERATION.md)<br>`docs/PULSAR_ANNOTATED_PUBLIC_GENERATION.md` | Models and generation |
| [Source-bound annotated Stash batches](../docs/PULSAR_ANNOTATED_STASH_BATCHES.md)<br>`docs/PULSAR_ANNOTATED_STASH_BATCHES.md` | Library and Stash |
| [Annotated Stash batches with owner-selected model packs](../docs/PULSAR_ANNOTATED_STASH_PACK_BATCHES.md)<br>`docs/PULSAR_ANNOTATED_STASH_PACK_BATCHES.md` | Library and Stash |
| [Annotated generation through Stash publication and portable download](../docs/PULSAR_ANNOTATED_STASH_PUBLICATION.md)<br>`docs/PULSAR_ANNOTATED_STASH_PUBLICATION.md` | Library and Stash |
| [Explicit annotated training views](../docs/PULSAR_ANNOTATED_TRAINING_VIEWS.md)<br>`docs/PULSAR_ANNOTATED_TRAINING_VIEWS.md` | Multimodal inputs |
| [Actual cleanup after AppImage extract-and-run](../docs/PULSAR_APPIMAGE_CLEANUP.md)<br>`docs/PULSAR_APPIMAGE_CLEANUP.md` | Cross-project or reference |
| [Persistent runtime custody in a Linux AppImage](../docs/PULSAR_APPIMAGE_RUNTIME_CUSTODY.md)<br>`docs/PULSAR_APPIMAGE_RUNTIME_CUSTODY.md` | Models and generation |
| [Pulsar Architecture Decisions and Design Continuation](../docs/PULSAR_ARCHITECTURE_DECISIONS.md)<br>`docs/PULSAR_ARCHITECTURE_DECISIONS.md` | Architecture and assurance |
| [Retained generation origin inspection](../docs/PULSAR_ARCHIVED_GENERATION_ORIGINS.md)<br>`docs/PULSAR_ARCHIVED_GENERATION_ORIGINS.md` | Models and generation |
| [Current master ancestry from retained generation evidence](../docs/PULSAR_ARCHIVED_MASTER_ANCESTRY.md)<br>`docs/PULSAR_ARCHIVED_MASTER_ANCESTRY.md` | Cross-project or reference |
| [Bounded archival master reference decoding](../docs/PULSAR_ARCHIVED_MASTER_HEAD_BOUND.md)<br>`docs/PULSAR_ARCHIVED_MASTER_HEAD_BOUND.md` | Cross-project or reference |
| [Archived mixed-clock carriers](../docs/PULSAR_ARCHIVED_MIXED_CLOCK_CARRIERS.md)<br>`docs/PULSAR_ARCHIVED_MIXED_CLOCK_CARRIERS.md` | Cross-project or reference |
| [Archived mixed-clock retiming regression](../docs/PULSAR_ARCHIVED_MIXED_CLOCK_RETIMING.md)<br>`docs/PULSAR_ARCHIVED_MIXED_CLOCK_RETIMING.md` | Cross-project or reference |
| [Stash publication from a retained generation origin](../docs/PULSAR_ARCHIVED_STASH_PUBLICATION.md)<br>`docs/PULSAR_ARCHIVED_STASH_PUBLICATION.md` | Library and Stash |
| [Assistant capacity-release recovery](../docs/PULSAR_ASSISTANT_CAPACITY_RECOVERY.md)<br>`docs/PULSAR_ASSISTANT_CAPACITY_RECOVERY.md` | Assistant and extensions |
| [Local assistant in the desktop](../docs/PULSAR_ASSISTANT_DESKTOP.md)<br>`docs/PULSAR_ASSISTANT_DESKTOP.md` | Assistant and extensions |
| [Assistant edit recipes](../docs/PULSAR_ASSISTANT_EDIT_RECIPES.md)<br>`docs/PULSAR_ASSISTANT_EDIT_RECIPES.md` | Projects and interoperability |
| [Saved assistant edit recovery](../docs/PULSAR_ASSISTANT_EDIT_RECOVERY.md)<br>`docs/PULSAR_ASSISTANT_EDIT_RECOVERY.md` | Projects and interoperability |
| [Applying an assistant proposal to generation](../docs/PULSAR_ASSISTANT_GENERATION_APPROVAL.md)<br>`docs/PULSAR_ASSISTANT_GENERATION_APPROVAL.md` | Models and generation |
| [Saved assistant generation approval and desktop controls](../docs/PULSAR_ASSISTANT_GENERATION_INTENTS.md)<br>`docs/PULSAR_ASSISTANT_GENERATION_INTENTS.md` | Models and generation |
| [Assistant host setup and first-party CLI](../docs/PULSAR_ASSISTANT_HOST_AND_CLI.md)<br>`docs/PULSAR_ASSISTANT_HOST_AND_CLI.md` | Assistant and extensions |
| [Offline assistant host setup](../docs/PULSAR_ASSISTANT_HOST_SETUP.md)<br>`docs/PULSAR_ASSISTANT_HOST_SETUP.md` | Assistant and extensions |
| [Assistant journal capture admission](../docs/PULSAR_ASSISTANT_JOURNAL_ADMISSION.md)<br>`docs/PULSAR_ASSISTANT_JOURNAL_ADMISSION.md` | Projects and interoperability |
| [Assistant retirement diagnosis](../docs/PULSAR_ASSISTANT_RETIREMENT_DIAGNOSIS.md)<br>`docs/PULSAR_ASSISTANT_RETIREMENT_DIAGNOSIS.md` | Assistant and extensions |
| [Assistant selection of recorded review evidence](../docs/PULSAR_ASSISTANT_REVIEW_EXPLANATIONS.md)<br>`docs/PULSAR_ASSISTANT_REVIEW_EXPLANATIONS.md` | Assistant and extensions |
| [Recovering an assistant start](../docs/PULSAR_ASSISTANT_START_RESOLUTION.md)<br>`docs/PULSAR_ASSISTANT_START_RESOLUTION.md` | Assistant and extensions |
| [Assistant target application](../docs/PULSAR_ASSISTANT_TARGET_APPLICATION.md)<br>`docs/PULSAR_ASSISTANT_TARGET_APPLICATION.md` | Assistant and extensions |
| [Checked assistant target catalogue](../docs/PULSAR_ASSISTANT_TARGET_CATALOG.md)<br>`docs/PULSAR_ASSISTANT_TARGET_CATALOG.md` | Assistant and extensions |
| [Assistant tool surface: MCP server, movement API and funscript authoring](../docs/PULSAR_ASSISTANT_TOOL_SURFACE.md)<br>`docs/PULSAR_ASSISTANT_TOOL_SURFACE.md` | Assistant and extensions |
| [Atomic auxiliary startup schema](../docs/PULSAR_ATOMIC_AUXILIARY_SCHEMA.md)<br>`docs/PULSAR_ATOMIC_AUXILIARY_SCHEMA.md` | Cross-project or reference |
| [Atomic Linux recovery receipts](../docs/PULSAR_ATOMIC_LINUX_RECEIPTS.md)<br>`docs/PULSAR_ATOMIC_LINUX_RECEIPTS.md` | Cross-project or reference |
| [Explicit six-axis audio synthesis](../docs/PULSAR_AUDIO_BUNDLE.md)<br>`docs/PULSAR_AUDIO_BUNDLE.md` | Projects and interoperability |
| [Audio-modulated oscillation](../docs/PULSAR_AUDIO_OSCILLATION.md)<br>`docs/PULSAR_AUDIO_OSCILLATION.md` | Multimodal inputs |
| [Authenticated local-assistant proposals](../docs/PULSAR_AUTHENTICATED_ASSISTANT.md)<br>`docs/PULSAR_AUTHENTICATED_ASSISTANT.md` | Assistant and extensions |
| [Authenticated encoded source ingress](../docs/PULSAR_AUTHENTICATED_ENCODED_INGRESS.md)<br>`docs/PULSAR_AUTHENTICATED_ENCODED_INGRESS.md` | Architecture and assurance |
| [Authenticated prepared-image assistant](../docs/PULSAR_AUTHENTICATED_IMAGE_ASSISTANT.md)<br>`docs/PULSAR_AUTHENTICATED_IMAGE_ASSISTANT.md` | Assistant and extensions |
| [Authenticated automatic live-pair inference](../docs/PULSAR_AUTHENTICATED_LIVE_INFERENCE.md)<br>`docs/PULSAR_AUTHENTICATED_LIVE_INFERENCE.md` | Live sources and capture |
| [Authenticated WASM editing](../docs/PULSAR_AUTHENTICATED_WASM_EDITING.md)<br>`docs/PULSAR_AUTHENTICATED_WASM_EDITING.md` | Projects and interoperability |
| [Shared authority and archival input contracts](../docs/PULSAR_AUTHORITY_INPUTS.md)<br>`docs/PULSAR_AUTHORITY_INPUTS.md` | Architecture and assurance |
| [Explicit axis-pattern candidates](../docs/PULSAR_AXIS_PATTERN_CANDIDATES.md)<br>`docs/PULSAR_AXIS_PATTERN_CANDIDATES.md` | Cross-project or reference |
| [Experimental browser camera input](../docs/PULSAR_BROWSER_CAMERA.md)<br>`docs/PULSAR_BROWSER_CAMERA.md` | Live sources and capture |
| [Durable browser script-export recovery](../docs/PULSAR_BROWSER_EXPORT_RECOVERY.md)<br>`docs/PULSAR_BROWSER_EXPORT_RECOVERY.md` | Live sources and capture |
| [Confirmed browser export retirement and retained history](../docs/PULSAR_BROWSER_EXPORT_RETIREMENT.md)<br>`docs/PULSAR_BROWSER_EXPORT_RETIREMENT.md` | Live sources and capture |
| [Durable original browser gap commits](../docs/PULSAR_BROWSER_GAP_COMMIT_RECOVERY.md)<br>`docs/PULSAR_BROWSER_GAP_COMMIT_RECOVERY.md` | Live sources and capture |
| [Durable browser gap-upload intents](../docs/PULSAR_BROWSER_GAP_JOURNAL.md)<br>`docs/PULSAR_BROWSER_GAP_JOURNAL.md` | Live sources and capture |
| [Browser authored gap proposals](../docs/PULSAR_BROWSER_GAP_PROPOSALS.md)<br>`docs/PULSAR_BROWSER_GAP_PROPOSALS.md` | Live sources and capture |
| [Browser original edit-upload recovery](../docs/PULSAR_BROWSER_GAP_RECOVERY.md)<br>`docs/PULSAR_BROWSER_GAP_RECOVERY.md` | Live sources and capture |
| [Confirmed browser gap-proposal retirement](../docs/PULSAR_BROWSER_GAP_RETIREMENT.md)<br>`docs/PULSAR_BROWSER_GAP_RETIREMENT.md` | Live sources and capture |
| [Browser-observed Handy playback](../docs/PULSAR_BROWSER_HANDY_START.md)<br>`docs/PULSAR_BROWSER_HANDY_START.md` | Device playback |
| [Browser analysis into a live Handy HSP stream](../docs/PULSAR_BROWSER_HSP_LIVE.md)<br>`docs/PULSAR_BROWSER_HSP_LIVE.md` | Device playback |
| [Automatic browser live-capture continuation](../docs/PULSAR_BROWSER_LIVE_CONTINUATION.md)<br>`docs/PULSAR_BROWSER_LIVE_CONTINUATION.md` | Live sources and capture |
| [Local browser live-motion preview](../docs/PULSAR_BROWSER_LIVE_PREVIEW.md)<br>`docs/PULSAR_BROWSER_LIVE_PREVIEW.md` | Live sources and capture |
| [Browser player control and an optional Handy stop guard](../docs/PULSAR_BROWSER_PLAYER_CONTROL.md)<br>`docs/PULSAR_BROWSER_PLAYER_CONTROL.md` | Live sources and capture |
| [Browser preview freshness](../docs/PULSAR_BROWSER_PREVIEW_FRESHNESS.md)<br>`docs/PULSAR_BROWSER_PREVIEW_FRESHNESS.md` | Live sources and capture |
| [Browser recording candidate controls](../docs/PULSAR_BROWSER_RECORDING_CANDIDATES.md)<br>`docs/PULSAR_BROWSER_RECORDING_CANDIDATES.md` | Live sources and capture |
| [Browser recording-candidate staging recovery](../docs/PULSAR_BROWSER_RECORDING_CANDIDATE_RECOVERY.md)<br>`docs/PULSAR_BROWSER_RECORDING_CANDIDATE_RECOVERY.md` | Live sources and capture |
| [Confirmed browser recording-candidate retirement](../docs/PULSAR_BROWSER_RECORDING_CANDIDATE_RETIREMENT.md)<br>`docs/PULSAR_BROWSER_RECORDING_CANDIDATE_RETIREMENT.md` | Live sources and capture |
| [Explicit browser recording commitment](../docs/PULSAR_BROWSER_RECORDING_COMMIT.md)<br>`docs/PULSAR_BROWSER_RECORDING_COMMIT.md` | Live sources and capture |
| [Original browser recording-commit recovery](../docs/PULSAR_BROWSER_RECORDING_COMMIT_RECOVERY.md)<br>`docs/PULSAR_BROWSER_RECORDING_COMMIT_RECOVERY.md` | Live sources and capture |
| [Browser accepted-estimate recording controls](../docs/PULSAR_BROWSER_RECORDING_CONTROLS.md)<br>`docs/PULSAR_BROWSER_RECORDING_CONTROLS.md` | Live sources and capture |
| [Browser recording index recovery](../docs/PULSAR_BROWSER_RECORDING_INDEX.md)<br>`docs/PULSAR_BROWSER_RECORDING_INDEX.md` | Live sources and capture |
| [Stopped playback recording-index retirement](../docs/PULSAR_BROWSER_RECORDING_INDEX_RETIREMENT.md)<br>`docs/PULSAR_BROWSER_RECORDING_INDEX_RETIREMENT.md` | Live sources and capture |
| [Rendered local browser preview QA](../docs/PULSAR_BROWSER_RENDERED_QA.md)<br>`docs/PULSAR_BROWSER_RENDERED_QA.md` | Live sources and capture |
| [Browser committed-script download](../docs/PULSAR_BROWSER_SCRIPT_DOWNLOAD.md)<br>`docs/PULSAR_BROWSER_SCRIPT_DOWNLOAD.md` | Live sources and capture |
| [Browser playback of an imported source](../docs/PULSAR_BROWSER_SOURCE_MEDIA.md)<br>`docs/PULSAR_BROWSER_SOURCE_MEDIA.md` | Live sources and capture |
| [Experimental consent-based browser-tab input](../docs/PULSAR_BROWSER_TAB_CAPTURE.md)<br>`docs/PULSAR_BROWSER_TAB_CAPTURE.md` | Live sources and capture |
| [Experimental tab-capture loss and playback-failure handling](../docs/PULSAR_BROWSER_TAB_LIFECYCLE.md)<br>`docs/PULSAR_BROWSER_TAB_LIFECYCLE.md` | Live sources and capture |
| [Browser frame timing observations](../docs/PULSAR_BROWSER_TIMING.md)<br>`docs/PULSAR_BROWSER_TIMING.md` | Live sources and capture |
| [Bundled assistant workflow evidence — 2026-10-12](../docs/PULSAR_BUNDLED_ASSISTANT_WORKFLOWS.md)<br>`docs/PULSAR_BUNDLED_ASSISTANT_WORKFLOWS.md` | Projects and interoperability |
| [Bundled CPU inference runtime](../docs/PULSAR_BUNDLED_CPU_RUNTIME.md)<br>`docs/PULSAR_BUNDLED_CPU_RUNTIME.md` | Projects and interoperability |
| [Desktop bundled model preparation](../docs/PULSAR_BUNDLED_DESKTOP_PREPARATION.md)<br>`docs/PULSAR_BUNDLED_DESKTOP_PREPARATION.md` | Projects and interoperability |
| [Inert bundled generation pack delivery](../docs/PULSAR_BUNDLED_GENERATION_PACKS.md)<br>`docs/PULSAR_BUNDLED_GENERATION_PACKS.md` | Projects and interoperability |
| [Bundled media fixture compatibility](../docs/PULSAR_BUNDLED_MEDIA_FIXTURES.md)<br>`docs/PULSAR_BUNDLED_MEDIA_FIXTURES.md` | Projects and interoperability |
| [Verified bundled model discovery](../docs/PULSAR_BUNDLED_MODEL_DISCOVERY.md)<br>`docs/PULSAR_BUNDLED_MODEL_DISCOVERY.md` | Projects and interoperability |
| [Generation flow diagnostics and camera-motion fixture](../docs/PULSAR_CAMERA_MOTION_EVIDENCE.md)<br>`docs/PULSAR_CAMERA_MOTION_EVIDENCE.md` | Live sources and capture |
| [Worker cancellation after durable commit](../docs/PULSAR_CANCELLATION_COMMIT_ORDER.md)<br>`docs/PULSAR_CANCELLATION_COMMIT_ORDER.md` | Cross-project or reference |
| [Candidate image generation origin inspection](../docs/PULSAR_CANDIDATE_IMAGE_ORIGIN.md)<br>`docs/PULSAR_CANDIDATE_IMAGE_ORIGIN.md` | Cross-project or reference |
| [Candidate motion evidence presentation](../docs/PULSAR_CANDIDATE_MOTION_EVIDENCE.md)<br>`docs/PULSAR_CANDIDATE_MOTION_EVIDENCE.md` | Models and generation |
| [Captured CPU runtime execution](../docs/PULSAR_CAPTURED_CPU_RUNTIME.md)<br>`docs/PULSAR_CAPTURED_CPU_RUNTIME.md` | Live sources and capture |
| [Captured RGB pairs and bounded scheduling](../docs/PULSAR_CAPTURED_FRAME_PAIRS.md)<br>`docs/PULSAR_CAPTURED_FRAME_PAIRS.md` | Live sources and capture |
| [Capture admission receipts after Engine restart](../docs/PULSAR_CAPTURE_RESTART_RECEIPTS.md)<br>`docs/PULSAR_CAPTURE_RESTART_RECEIPTS.md` | Live sources and capture |
| [Catalog-bound owner approval for future pack admission](../docs/PULSAR_CATALOG_BOUND_APPROVAL.md)<br>`docs/PULSAR_CATALOG_BOUND_APPROVAL.md` | Cross-project or reference |
| [Cgroup configuration through held directory identity](../docs/PULSAR_CGROUP_CONFIGURATION_IDENTITY.md)<br>`docs/PULSAR_CGROUP_CONFIGURATION_IDENTITY.md` | Cross-project or reference |
| [Admitted cgroup creation custody](../docs/PULSAR_CGROUP_CREATION_CUSTODY.md)<br>`docs/PULSAR_CGROUP_CREATION_CUSTODY.md` | Cross-project or reference |
| [Format-4 cgroup creation journal primitive](../docs/PULSAR_CGROUP_CREATION_JOURNAL.md)<br>`docs/PULSAR_CGROUP_CREATION_JOURNAL.md` | Projects and interoperability |
| [Production admitted cgroup creation](../docs/PULSAR_CGROUP_CREATION_PRODUCTION.md)<br>`docs/PULSAR_CGROUP_CREATION_PRODUCTION.md` | Cross-project or reference |
| [Reader requirement for durable cgroup creation](../docs/PULSAR_CGROUP_CREATION_READER.md)<br>`docs/PULSAR_CGROUP_CREATION_READER.md` | Cross-project or reference |
| [Journaled reclamation of recorded Linux cgroups](../docs/PULSAR_CGROUP_RECLAMATION.md)<br>`docs/PULSAR_CGROUP_RECLAMATION.md` | Cross-project or reference |
| [Held directory identity at worker registration](../docs/PULSAR_CGROUP_REGISTRATION_IDENTITY.md)<br>`docs/PULSAR_CGROUP_REGISTRATION_IDENTITY.md` | Cross-project or reference |
| [Staging cleanup and storage-delay diagnosis — 2026-10-09](../docs/PULSAR_CLEANUP_IO_DIAGNOSIS.md)<br>`docs/PULSAR_CLEANUP_IO_DIAGNOSIS.md` | Cross-project or reference |
| [Cleanup receipt lock owner lifetime](../docs/PULSAR_CLEANUP_LOCK_OWNERSHIP.md)<br>`docs/PULSAR_CLEANUP_LOCK_OWNERSHIP.md` | Cross-project or reference |
| [Client journal contention reproduction and contract checks — 2026-10-10](../docs/PULSAR_CLIENT_JOURNAL_CONTENTION.md)<br>`docs/PULSAR_CLIENT_JOURNAL_CONTENTION.md` | Projects and interoperability |
| [Client storage and quality verification](../docs/PULSAR_CLIENT_STORAGE.md)<br>`docs/PULSAR_CLIENT_STORAGE.md` | Cross-project or reference |
| [CLI visual tier parity and bounded background recovery](../docs/PULSAR_CLI_VISUAL_TIERS.md)<br>`docs/PULSAR_CLI_VISUAL_TIERS.md` | Cross-project or reference |
| [Closed CUDA runtime checkpoint](../docs/PULSAR_CLOSED_CUDA_RUNTIME.md)<br>`docs/PULSAR_CLOSED_CUDA_RUNTIME.md` | Models and generation |
| [Compact creative completion experiment](../docs/PULSAR_COMPACT_CREATIVE_COMPLETIONS.md)<br>`docs/PULSAR_COMPACT_CREATIVE_COMPLETIONS.md` | Cross-project or reference |
| [Shared worker confinement interface](../docs/PULSAR_CONFINEMENT_INTERFACE.md)<br>`docs/PULSAR_CONFINEMENT_INTERFACE.md` | Cross-project or reference |
| [Native contributor origin verification](../docs/PULSAR_CONTRIBUTOR_CLOCK_VERIFICATION.md)<br>`docs/PULSAR_CONTRIBUTOR_CLOCK_VERIFICATION.md` | Cross-project or reference |
| [Corrupt audio decoding failures](../docs/PULSAR_CORRUPT_AUDIO_FAILURES.md)<br>`docs/PULSAR_CORRUPT_AUDIO_FAILURES.md` | Multimodal inputs |
| [Corrupt video decoding failures](../docs/PULSAR_CORRUPT_MEDIA_FAILURES.md)<br>`docs/PULSAR_CORRUPT_MEDIA_FAILURES.md` | Multimodal inputs |
| [Aggregate worker CPU admission](../docs/PULSAR_CPU_ADMISSION.md)<br>`docs/PULSAR_CPU_ADMISSION.md` | Models and generation |
| [Creative generation approval capabilities](../docs/PULSAR_CREATIVE_APPROVAL_CAPABILITIES.md)<br>`docs/PULSAR_CREATIVE_APPROVAL_CAPABILITIES.md` | Cross-project or reference |
| [Desktop synthetic proposal review](../docs/PULSAR_CREATIVE_DESKTOP.md)<br>`docs/PULSAR_CREATIVE_DESKTOP.md` | Cross-project or reference |
| [Creative model candidate comparison](../docs/PULSAR_CREATIVE_MODEL_COMPARISON.md)<br>`docs/PULSAR_CREATIVE_MODEL_COMPARISON.md` | Models and generation |
| [Bounded learned creative proposals](../docs/PULSAR_CREATIVE_PATTERN_PROPOSALS.md)<br>`docs/PULSAR_CREATIVE_PATTERN_PROPOSALS.md` | Cross-project or reference |
| [Creative pulse duties and declared channels](../docs/PULSAR_CREATIVE_PULSE_TUPLES.md)<br>`docs/PULSAR_CREATIVE_PULSE_TUPLES.md` | Cross-project or reference |
| [CUDA mounted-alias byte proof, 2026-10-09](../docs/PULSAR_CUDA_ALIAS_BYTE_PROOF.md)<br>`docs/PULSAR_CUDA_ALIAS_BYTE_PROOF.md` | Models and generation |
| [CUDA archived producer identity — 2026-10-09](../docs/PULSAR_CUDA_ARCHIVED_CLOCK_ORIGINS.md)<br>`docs/PULSAR_CUDA_ARCHIVED_CLOCK_ORIGINS.md` | Models and generation |
| [Durable CUDA candidate evidence](../docs/PULSAR_CUDA_CANDIDATE_EVIDENCE.md)<br>`docs/PULSAR_CUDA_CANDIDATE_EVIDENCE.md` | Models and generation |
| [Shipped CUDA CLI cancellation during frozen-worker Finish](../docs/PULSAR_CUDA_CLI_FINISH_CANCELLATION.md)<br>`docs/PULSAR_CUDA_CLI_FINISH_CANCELLATION.md` | Models and generation |
| [CUDA capture owner-death recovery through the shipped CLI](../docs/PULSAR_CUDA_CLI_RECOVERY.md)<br>`docs/PULSAR_CUDA_CLI_RECOVERY.md` | Models and generation |
| [Held CUDA device bindings](../docs/PULSAR_CUDA_DEVICE_BINDINGS.md)<br>`docs/PULSAR_CUDA_DEVICE_BINDINGS.md` | Device playback |
| [Durable CUDA attempt intent and schema-15 barrier](../docs/PULSAR_CUDA_DURABLE_INTENT.md)<br>`docs/PULSAR_CUDA_DURABLE_INTENT.md` | Models and generation |
| [Supervised CUDA memory broker](../docs/PULSAR_CUDA_MEMORY_BROKER.md)<br>`docs/PULSAR_CUDA_MEMORY_BROKER.md` | Models and generation |
| [Request-bound CUDA model adapters](../docs/PULSAR_CUDA_MODEL_ADAPTERS.md)<br>`docs/PULSAR_CUDA_MODEL_ADAPTERS.md` | Models and generation |
| [Request-bound captured-library and CUDA device mounts](../docs/PULSAR_CUDA_MOUNT_CUSTODY.md)<br>`docs/PULSAR_CUDA_MOUNT_CUSTODY.md` | Models and generation |
| [Owner CUDA policy in the real engine](../docs/PULSAR_CUDA_OWNER_POLICY.md)<br>`docs/PULSAR_CUDA_OWNER_POLICY.md` | Models and generation |
| [CUDA preparation I/O diagnosis, 2026-10-09](../docs/PULSAR_CUDA_PREPARATION_IO.md)<br>`docs/PULSAR_CUDA_PREPARATION_IO.md` | Models and generation |
| [CUDA resource declarations and launch limits](../docs/PULSAR_CUDA_RESOURCE_CONTRACT.md)<br>`docs/PULSAR_CUDA_RESOURCE_CONTRACT.md` | Models and generation |
| [Original CUDA evidence egress and parent verification](../docs/PULSAR_CUDA_RESULT_EGRESS.md)<br>`docs/PULSAR_CUDA_RESULT_EGRESS.md` | Models and generation |
| [Worker CUDA SDK identity checkpoint](../docs/PULSAR_CUDA_SDK_IDENTITY.md)<br>`docs/PULSAR_CUDA_SDK_IDENTITY.md` | Models and generation |
| [Selected model packs on CUDA](../docs/PULSAR_CUDA_SELECTED_MODEL_PACK.md)<br>`docs/PULSAR_CUDA_SELECTED_MODEL_PACK.md` | Models and generation |
| [Confined CUDA worker bootstrap](../docs/PULSAR_CUDA_WORKER_BOOTSTRAP.md)<br>`docs/PULSAR_CUDA_WORKER_BOOTSTRAP.md` | Models and generation |
| [CUDA worker request contract](../docs/PULSAR_CUDA_WORKER_CONTRACT.md)<br>`docs/PULSAR_CUDA_WORKER_CONTRACT.md` | Models and generation |
| [cuDNN sublibrary closure diagnosis, 2026-10-09](../docs/PULSAR_CUDNN_SUBLIBRARY_CLOSURE.md)<br>`docs/PULSAR_CUDNN_SUBLIBRARY_CLOSURE.md` | Library and Stash |
| [Dedicated worker composition](../docs/PULSAR_DEDICATED_WORKER.md)<br>`docs/PULSAR_DEDICATED_WORKER.md` | Cross-project or reference |
| [Desktop assistant host settings — 2026-10-12](../docs/PULSAR_DESKTOP_ASSISTANT_SETTINGS.md)<br>`docs/PULSAR_DESKTOP_ASSISTANT_SETTINGS.md` | Assistant and extensions |
| [Desktop six-axis audio authoring](../docs/PULSAR_DESKTOP_AUDIO_BUNDLE.md)<br>`docs/PULSAR_DESKTOP_AUDIO_BUNDLE.md` | Projects and interoperability |
| [Desktop launch of the shared browser player](../docs/PULSAR_DESKTOP_BROWSER_LIVE.md)<br>`docs/PULSAR_DESKTOP_BROWSER_LIVE.md` | Live sources and capture |
| [Desktop capture evidence](../docs/PULSAR_DESKTOP_CAPTURE_EVIDENCE.md)<br>`docs/PULSAR_DESKTOP_CAPTURE_EVIDENCE.md` | Live sources and capture |
| [Desktop catalog controls and persistent recovery](../docs/PULSAR_DESKTOP_CATALOGS.md)<br>`docs/PULSAR_DESKTOP_CATALOGS.md` | Cross-project or reference |
| [Desktop retained CUDA evidence removal](../docs/PULSAR_DESKTOP_CUDA_EVIDENCE_REMOVAL.md)<br>`docs/PULSAR_DESKTOP_CUDA_EVIDENCE_REMOVAL.md` | Models and generation |
| [Annotated CUDA generation through the Linux desktop](../docs/PULSAR_DESKTOP_CUDA_GENERATION.md)<br>`docs/PULSAR_DESKTOP_CUDA_GENERATION.md` | Models and generation |
| [Desktop CUDA placed-interval workflow](../docs/PULSAR_DESKTOP_CUDA_INTERVAL_COMPLETION.md)<br>`docs/PULSAR_DESKTOP_CUDA_INTERVAL_COMPLETION.md` | Models and generation |
| [Selected annotated CUDA packs through the Linux desktop](../docs/PULSAR_DESKTOP_CUDA_PACK.md)<br>`docs/PULSAR_DESKTOP_CUDA_PACK.md` | Models and generation |
| [Annotated CUDA preview in the Linux desktop](../docs/PULSAR_DESKTOP_CUDA_PREVIEW.md)<br>`docs/PULSAR_DESKTOP_CUDA_PREVIEW.md` | Models and generation |
| [Durable desktop removal recovery](../docs/PULSAR_DESKTOP_CUDA_REMOVAL_RECOVERY.md)<br>`docs/PULSAR_DESKTOP_CUDA_REMOVAL_RECOVERY.md` | Models and generation |
| [Desktop generation, edge trimming and export](../docs/PULSAR_DESKTOP_EDGE_TRIM.md)<br>`docs/PULSAR_DESKTOP_EDGE_TRIM.md` | Cross-project or reference |
| [Evidence panel worker failure](../docs/PULSAR_DESKTOP_EVIDENCE_WORKER_FAILURE.md)<br>`docs/PULSAR_DESKTOP_EVIDENCE_WORKER_FAILURE.md` | Cross-project or reference |
| [Desktop Handy inspection](../docs/PULSAR_DESKTOP_HANDY_INSPECTION.md)<br>`docs/PULSAR_DESKTOP_HANDY_INSPECTION.md` | Device playback |
| [Saved Handy references in Pulsar Desktop](../docs/PULSAR_DESKTOP_HANDY_REGISTRATIONS.md)<br>`docs/PULSAR_DESKTOP_HANDY_REGISTRATIONS.md` | Device playback |
| [Desktop image assistant preparation](../docs/PULSAR_DESKTOP_IMAGE_ASSISTANT.md)<br>`docs/PULSAR_DESKTOP_IMAGE_ASSISTANT.md` | Assistant and extensions |
| [Desktop retained library batches](../docs/PULSAR_DESKTOP_LIBRARY_BATCHES.md)<br>`docs/PULSAR_DESKTOP_LIBRARY_BATCHES.md` | Library and Stash |
| [Author browser live options in the desktop](../docs/PULSAR_DESKTOP_LIVE_OPTIONS.md)<br>`docs/PULSAR_DESKTOP_LIVE_OPTIONS.md` | Live sources and capture |
| [Desktop owner-selected model packs](../docs/PULSAR_DESKTOP_MODEL_PACKS.md)<br>`docs/PULSAR_DESKTOP_MODEL_PACKS.md` | Models and generation |
| [Desktop atomic neutral bundle export](../docs/PULSAR_DESKTOP_NEUTRAL_BUNDLE.md)<br>`docs/PULSAR_DESKTOP_NEUTRAL_BUNDLE.md` | Projects and interoperability |
| [Persistent desktop pack recovery](../docs/PULSAR_DESKTOP_PACK_RECOVERY.md)<br>`docs/PULSAR_DESKTOP_PACK_RECOVERY.md` | Models and generation |
| [Desktop target-pair layout and native workflow](../docs/PULSAR_DESKTOP_PAIR_LAYOUT.md)<br>`docs/PULSAR_DESKTOP_PAIR_LAYOUT.md` | Cross-project or reference |
| [Desktop placed interval generation and snapshot seek, 2026-10-09](../docs/PULSAR_DESKTOP_PLACED_INTERVAL.md)<br>`docs/PULSAR_DESKTOP_PLACED_INTERVAL.md` | Cross-project or reference |
| [Desktop interrupted preparation recovery](../docs/PULSAR_DESKTOP_PREPARATION_RECOVERY.md)<br>`docs/PULSAR_DESKTOP_PREPARATION_RECOVERY.md` | Cross-project or reference |
| [Desktop rendering evidence](../docs/PULSAR_DESKTOP_RENDERING.md)<br>`docs/PULSAR_DESKTOP_RENDERING.md` | Cross-project or reference |
| [Desktop script repair](../docs/PULSAR_DESKTOP_SCRIPT_REPAIR.md)<br>`docs/PULSAR_DESKTOP_SCRIPT_REPAIR.md` | Projects and interoperability |
| [Desktop startup recovery — 2026-10-10](../docs/PULSAR_DESKTOP_STARTUP_RECOVERY.md)<br>`docs/PULSAR_DESKTOP_STARTUP_RECOVERY.md` | Cross-project or reference |
| [Desktop observed-target picking — 2026-10-09](../docs/PULSAR_DESKTOP_TARGET_PICKING.md)<br>`docs/PULSAR_DESKTOP_TARGET_PICKING.md` | Multimodal inputs |
| [Desktop target-view authoring — 2026-10-09](../docs/PULSAR_DESKTOP_TARGET_VIEW_AUTHORING.md)<br>`docs/PULSAR_DESKTOP_TARGET_VIEW_AUTHORING.md` | Multimodal inputs |
| [Desktop completed WASM archival recovery](../docs/PULSAR_DESKTOP_WASM_ARCHIVAL.md)<br>`docs/PULSAR_DESKTOP_WASM_ARCHIVAL.md` | Assistant and extensions |
| [Desktop unsuccessful WASM attempt cleanup](../docs/PULSAR_DESKTOP_WASM_CLEANUP.md)<br>`docs/PULSAR_DESKTOP_WASM_CLEANUP.md` | Assistant and extensions |
| [Durable desktop WASM cleanup recovery](../docs/PULSAR_DESKTOP_WASM_CLEANUP_RECOVERY.md)<br>`docs/PULSAR_DESKTOP_WASM_CLEANUP_RECOVERY.md` | Assistant and extensions |
| [Desktop WASM editor workflow](../docs/PULSAR_DESKTOP_WASM_EDITOR.md)<br>`docs/PULSAR_DESKTOP_WASM_EDITOR.md` | Projects and interoperability |
| [Desktop WASM invocation recovery](../docs/PULSAR_DESKTOP_WASM_RECOVERY.md)<br>`docs/PULSAR_DESKTOP_WASM_RECOVERY.md` | Assistant and extensions |
| [Explicit detector letterbox](../docs/PULSAR_DETECTOR_LETTERBOX.md)<br>`docs/PULSAR_DETECTOR_LETTERBOX.md` | Cross-project or reference |
| [Pinned detector numerical comparison](../docs/PULSAR_DETECTOR_NUMERICAL_COMPARISON.md)<br>`docs/PULSAR_DETECTOR_NUMERICAL_COMPARISON.md` | Cross-project or reference |
| [Checked generation detector score](../docs/PULSAR_DETECTOR_SCORE_SETTINGS.md)<br>`docs/PULSAR_DETECTOR_SCORE_SETTINGS.md` | Cross-project or reference |
| [Durable device-effect metadata and restart recovery](../docs/PULSAR_DEVICE_EFFECT_JOURNAL.md)<br>`docs/PULSAR_DEVICE_EFFECT_JOURNAL.md` | Device playback |
| [Archiving analytic device-motion exports](../docs/PULSAR_DEVICE_EXPORT_ARCHIVES.md)<br>`docs/PULSAR_DEVICE_EXPORT_ARCHIVES.md` | Device playback |
| [Durable native action-origin timing proofs](../docs/PULSAR_DURABLE_NATIVE_ACTION_PROOFS.md)<br>`docs/PULSAR_DURABLE_NATIVE_ACTION_PROOFS.md` | Cross-project or reference |
| [Empty stopped recording scopes remain unavailable evidence](../docs/PULSAR_EMPTY_RECORDING_SCOPES.md)<br>`docs/PULSAR_EMPTY_RECORDING_SCOPES.md` | Cross-project or reference |
| [Engine CUDA generation](../docs/PULSAR_ENGINE_CUDA_GENERATION.md)<br>`docs/PULSAR_ENGINE_CUDA_GENERATION.md` | Models and generation |
| [Engine CUDA detector frame Preview](../docs/PULSAR_ENGINE_CUDA_PREVIEW.md)<br>`docs/PULSAR_ENGINE_CUDA_PREVIEW.md` | Models and generation |
| [Engine feature boundaries and completion custody](../docs/PULSAR_ENGINE_FEATURE_BOUNDARIES.md)<br>`docs/PULSAR_ENGINE_FEATURE_BOUNDARIES.md` | Cross-project or reference |
| [Engine lint gate progress](../docs/PULSAR_ENGINE_LINT_BATCH.md)<br>`docs/PULSAR_ENGINE_LINT_BATCH.md` | Library and Stash |
| [Engine-owned private live CUDA startup](../docs/PULSAR_ENGINE_LIVE_CUDA_START.md)<br>`docs/PULSAR_ENGINE_LIVE_CUDA_START.md` | Live sources and capture |
| [Engine quality and process-gate follow-up](../docs/PULSAR_ENGINE_QUALITY_FOLLOWUP.md)<br>`docs/PULSAR_ENGINE_QUALITY_FOLLOWUP.md` | Models and generation |
| [Engine and client regression checkpoint](../docs/PULSAR_ENGINE_REGRESSION_2026-10-11.md)<br>`docs/PULSAR_ENGINE_REGRESSION_2026-10-11.md` | Cross-project or reference |
| [Engine shutdown and worker retirement](../docs/PULSAR_ENGINE_SHUTDOWN.md)<br>`docs/PULSAR_ENGINE_SHUTDOWN.md` | Cross-project or reference |
| [Engine ownership for ordinary Linux workers](../docs/PULSAR_ENGINE_WORKER_FENCE.md)<br>`docs/PULSAR_ENGINE_WORKER_FENCE.md` | Cross-project or reference |
| [Source snapshots and retained evidence](../docs/PULSAR_EVIDENCE_STORAGE.md)<br>`docs/PULSAR_EVIDENCE_STORAGE.md` | Cross-project or reference |
| [Exact live project phase across epochs](../docs/PULSAR_EXACT_LIVE_PROJECT_PHASE.md)<br>`docs/PULSAR_EXACT_LIVE_PROJECT_PHASE.md` | Live sources and capture |
| [Exact merge action origins](../docs/PULSAR_EXACT_MERGE_ACTION_ORIGINS.md)<br>`docs/PULSAR_EXACT_MERGE_ACTION_ORIGINS.md` | Cross-project or reference |
| [Explicit existing-motion retiming](../docs/PULSAR_EXISTING_MOTION_RETIMING.md)<br>`docs/PULSAR_EXISTING_MOTION_RETIMING.md` | Models and generation |
| [Paired export cleanup interruption](../docs/PULSAR_EXPORT_CLEANUP_RECOVERY.md)<br>`docs/PULSAR_EXPORT_CLEANUP_RECOVERY.md` | Projects and interoperability |
| [Paired export after client process death](../docs/PULSAR_EXPORT_CLIENT_DEATH.md)<br>`docs/PULSAR_EXPORT_CLIENT_DEATH.md` | Projects and interoperability |
| [Export corruption refusal assurance](../docs/PULSAR_EXPORT_REFUSAL_ASSURANCE.md)<br>`docs/PULSAR_EXPORT_REFUSAL_ASSURANCE.md` | Projects and interoperability |
| [Bound worker-fence reopen, 2026-10-09](../docs/PULSAR_FENCE_BOUND_REOPEN.md)<br>`docs/PULSAR_FENCE_BOUND_REOPEN.md` | Cross-project or reference |
| [Worker-fence refusal diagnostic, 2026-10-09](../docs/PULSAR_FENCE_REFUSAL_DIAGNOSTIC.md)<br>`docs/PULSAR_FENCE_REFUSAL_DIAGNOSTIC.md` | Cross-project or reference |
| [Confirming fixture Engine retirement](../docs/PULSAR_FIXTURE_ENGINE_RETIREMENT.md)<br>`docs/PULSAR_FIXTURE_ENGINE_RETIREMENT.md` | Cross-project or reference |
| [Workspace formatting and export/restart diagnosis](../docs/PULSAR_FORMATTING_AND_EXPORT_DIAGNOSIS.md)<br>`docs/PULSAR_FORMATTING_AND_EXPORT_DIAGNOSIS.md` | Projects and interoperability |
| [Explicit authored gap-resolution proposals](../docs/PULSAR_GAP_RESOLUTION.md)<br>`docs/PULSAR_GAP_RESOLUTION.md` | Cross-project or reference |
| [CPU generation progress recovery](../docs/PULSAR_GENERATION_PROGRESS.md)<br>`docs/PULSAR_GENERATION_PROGRESS.md` | Models and generation |
| [Frozen generation quality diagnosis](../docs/PULSAR_GENERATION_QUALITY_DIAGNOSIS.md)<br>`docs/PULSAR_GENERATION_QUALITY_DIAGNOSIS.md` | Models and generation |
| [Cooperative GPU resource accounting](../docs/PULSAR_GPU_RESOURCE_ACCOUNTING.md)<br>`docs/PULSAR_GPU_RESOURCE_ACCOUNTING.md` | Multimodal inputs |
| [Explicit Handy clock synchronization](../docs/PULSAR_HANDY_CLOCK_SYNC.md)<br>`docs/PULSAR_HANDY_CLOCK_SYNC.md` | Device playback |
| [Handy connection sessions](../docs/PULSAR_HANDY_CONNECTION_SESSIONS.md)<br>`docs/PULSAR_HANDY_CONNECTION_SESSIONS.md` | Device playback |
| [Continuous timing for an existing Handy play](../docs/PULSAR_HANDY_CONTINUOUS_TIMING.md)<br>`docs/PULSAR_HANDY_CONTINUOUS_TIMING.md` | Device playback |
| [Acquire and maintain a Handy controller lease](../docs/PULSAR_HANDY_CONTROLLER_LEASES.md)<br>`docs/PULSAR_HANDY_CONTROLLER_LEASES.md` | Device playback |
| [Read-only Handy device timing inspection](../docs/PULSAR_HANDY_DEVICE_TIMING.md)<br>`docs/PULSAR_HANDY_DEVICE_TIMING.md` | Device playback |
| [Owned Handy HSSP transport](../docs/PULSAR_HANDY_DISPATCH.md)<br>`docs/PULSAR_HANDY_DISPATCH.md` | Device playback |
| [Handy stop fallback and incomplete history](../docs/PULSAR_HANDY_EMERGENCY_STOP.md)<br>`docs/PULSAR_HANDY_EMERGENCY_STOP.md` | Device playback |
| [Generated Handy point streams](../docs/PULSAR_HANDY_GENERATED_STREAMS.md)<br>`docs/PULSAR_HANDY_GENERATED_STREAMS.md` | Device playback |
| [Explicit Handy temporary script hosting](../docs/PULSAR_HANDY_HOSTING.md)<br>`docs/PULSAR_HANDY_HOSTING.md` | Device playback |
| [Durable HSP commands and native dispatch](../docs/PULSAR_HANDY_HSP_DISPATCH.md)<br>`docs/PULSAR_HANDY_HSP_DISPATCH.md` | Device playback |
| [HSP Stop during unavailable journal writes](../docs/PULSAR_HANDY_HSP_EMERGENCY_STOP.md)<br>`docs/PULSAR_HANDY_HSP_EMERGENCY_STOP.md` | Device playback |
| [Engine-owned HSP streaming lifecycle](../docs/PULSAR_HANDY_HSP_OWNER.md)<br>`docs/PULSAR_HANDY_HSP_OWNER.md` | Device playback |
| [Prepare Handy HSP synchronization](../docs/PULSAR_HANDY_HSP_PREPARATION.md)<br>`docs/PULSAR_HANDY_HSP_PREPARATION.md` | Device playback |
| [HSP qualification, emergency history and recovery](../docs/PULSAR_HANDY_HSP_RECOVERY.md)<br>`docs/PULSAR_HANDY_HSP_RECOVERY.md` | Device playback |
| [Handy online inspection](../docs/PULSAR_HANDY_INSPECTION.md)<br>`docs/PULSAR_HANDY_INSPECTION.md` | Device playback |
| [Handy operative settings and qualification subjects](../docs/PULSAR_HANDY_OPERATIVE_SETTINGS.md)<br>`docs/PULSAR_HANDY_OPERATIVE_SETTINGS.md` | Device playback |
| [Combined native playback timing](../docs/PULSAR_HANDY_PLAYBACK_CLOCK.md)<br>`docs/PULSAR_HANDY_PLAYBACK_CLOCK.md` | Device playback |
| [Handy playback coordinator](../docs/PULSAR_HANDY_PLAYBACK_COORDINATOR.md)<br>`docs/PULSAR_HANDY_PLAYBACK_COORDINATOR.md` | Device playback |
| [Prepare native Handy playback inputs](../docs/PULSAR_HANDY_PLAYBACK_PREPARATION.md)<br>`docs/PULSAR_HANDY_PLAYBACK_PREPARATION.md` | Device playback |
| [Durable original play-operation custody](../docs/PULSAR_HANDY_PLAY_INTENTS.md)<br>`docs/PULSAR_HANDY_PLAY_INTENTS.md` | Device playback |
| [Guarded native Handy playback submission](../docs/PULSAR_HANDY_PLAY_SUBMISSION.md)<br>`docs/PULSAR_HANDY_PLAY_SUBMISSION.md` | Device playback |
| [Inspect the stream associated with a Handy setup](../docs/PULSAR_HANDY_PROGRAM_CORRELATION.md)<br>`docs/PULSAR_HANDY_PROGRAM_CORRELATION.md` | Device playback |
| [Handy qualification dossiers and operator review](../docs/PULSAR_HANDY_QUALIFICATION.md)<br>`docs/PULSAR_HANDY_QUALIFICATION.md` | Device playback |
| [Handy command recovery and explicit resumption](../docs/PULSAR_HANDY_RECONCILIATION.md)<br>`docs/PULSAR_HANDY_RECONCILIATION.md` | Device playback |
| [Owner-controlled Handy registrations](../docs/PULSAR_HANDY_REGISTRATIONS.md)<br>`docs/PULSAR_HANDY_REGISTRATIONS.md` | Device playback |
| [Handy RPC error correlation](../docs/PULSAR_HANDY_RPC_ERROR_CORRELATION.md)<br>`docs/PULSAR_HANDY_RPC_ERROR_CORRELATION.md` | Device playback |
| [Handy public RPC stop codec](../docs/PULSAR_HANDY_RPC_STOP_CODEC.md)<br>`docs/PULSAR_HANDY_RPC_STOP_CODEC.md` | Device playback |
| [Local Handy Stroke script preparation](../docs/PULSAR_HANDY_SCRIPT_PREPARATION.md)<br>`docs/PULSAR_HANDY_SCRIPT_PREPARATION.md` | Device playback |
| [Native Handy server-clock observations](../docs/PULSAR_HANDY_SERVER_CLOCK.md)<br>`docs/PULSAR_HANDY_SERVER_CLOCK.md` | Device playback |
| [Cancel an unclaimed Handy setup](../docs/PULSAR_HANDY_SETUP_CANCELLATION.md)<br>`docs/PULSAR_HANDY_SETUP_CANCELLATION.md` | Device playback |
| [Read-only inspection before Handy setup](../docs/PULSAR_HANDY_SETUP_INSPECTION.md)<br>`docs/PULSAR_HANDY_SETUP_INSPECTION.md` | Device playback |
| [Durable setup intents and single-use claims](../docs/PULSAR_HANDY_SETUP_JOURNAL.md)<br>`docs/PULSAR_HANDY_SETUP_JOURNAL.md` | Device playback |
| [Retained Handy setup observations](../docs/PULSAR_HANDY_SETUP_OBSERVATIONS.md)<br>`docs/PULSAR_HANDY_SETUP_OBSERVATIONS.md` | Device playback |
| [Setup history recovery and private route identity](../docs/PULSAR_HANDY_SETUP_RECOVERY.md)<br>`docs/PULSAR_HANDY_SETUP_RECOVERY.md` | Device playback |
| [Guarded Handy setup](../docs/PULSAR_HANDY_SETUP_SEND.md)<br>`docs/PULSAR_HANDY_SETUP_SEND.md` | Device playback |
| [Engine-owned Handy stop and completion work](../docs/PULSAR_HANDY_STOP_OWNER.md)<br>`docs/PULSAR_HANDY_STOP_OWNER.md` | Device playback |
| [Handy REST v3 wire codecs](../docs/PULSAR_HANDY_WIRE_CODECS.md)<br>`docs/PULSAR_HANDY_WIRE_CODECS.md` | Device playback |
| [Image-conditioned generation origin](../docs/PULSAR_IMAGE_GENERATION_ORIGIN.md)<br>`docs/PULSAR_IMAGE_GENERATION_ORIGIN.md` | Models and generation |
| [Image-origin portable lifecycle](../docs/PULSAR_IMAGE_PORTABLE_LIFECYCLE.md)<br>`docs/PULSAR_IMAGE_PORTABLE_LIFECYCLE.md` | Projects and interoperability |
| [Preceding image-origin reader qualification](../docs/PULSAR_IMAGE_PRECEDING_READER.md)<br>`docs/PULSAR_IMAGE_PRECEDING_READER.md` | Cross-project or reference |
| [Portable clone publication binding](../docs/PULSAR_IMPORT_CLONE_BINDING.md)<br>`docs/PULSAR_IMPORT_CLONE_BINDING.md` | Projects and interoperability |
| [Import concurrency and release supplier comparison, 2026-10-09](../docs/PULSAR_IMPORT_CONCURRENCY.md)<br>`docs/PULSAR_IMPORT_CONCURRENCY.md` | Projects and interoperability |
| [Import and startup diagnosis, 2026-10-09](../docs/PULSAR_IMPORT_STARTUP_DIAGNOSIS.md)<br>`docs/PULSAR_IMPORT_STARTUP_DIAGNOSIS.md` | Projects and interoperability |
| [Separately captured audio with declared clock alignment](../docs/PULSAR_INDEPENDENT_AUDIO_ALIGNMENT.md)<br>`docs/PULSAR_INDEPENDENT_AUDIO_ALIGNMENT.md` | Multimodal inputs |
| [Declared independent audio clock rates](../docs/PULSAR_INDEPENDENT_AUDIO_RATE.md)<br>`docs/PULSAR_INDEPENDENT_AUDIO_RATE.md` | Multimodal inputs |
| [Inspect the current inference environment](../docs/PULSAR_INFERENCE_ENVIRONMENT.md)<br>`docs/PULSAR_INFERENCE_ENVIRONMENT.md` | Models and generation |
| [Inference equivalence records](../docs/PULSAR_INFERENCE_EQUIVALENCE.md)<br>`docs/PULSAR_INFERENCE_EQUIVALENCE.md` | Models and generation |
| [Installed pack association with an accepted offline catalog](../docs/PULSAR_INSTALLED_CATALOG_ASSOCIATION.md)<br>`docs/PULSAR_INSTALLED_CATALOG_ASSOCIATION.md` | Distribution and resources |
| [Additional pinned instruction-model candidate](../docs/PULSAR_INSTRUCT_MODEL_CANDIDATE.md)<br>`docs/PULSAR_INSTRUCT_MODEL_CANDIDATE.md` | Models and generation |
| [Project-time interval regeneration](../docs/PULSAR_INTERVAL_REGENERATION.md)<br>`docs/PULSAR_INTERVAL_REGENERATION.md` | Models and generation |
| [Item 1 closure: portable-project checkpoint](../docs/PULSAR_ITEM1_CLOSURE.md)<br>`docs/PULSAR_ITEM1_CLOSURE.md` | Cross-project or reference |
| [Pulsar items 2–3: resumed implementation](../docs/PULSAR_ITEMS_2_3_PROGRESS_2026-09-28.md)<br>`docs/PULSAR_ITEMS_2_3_PROGRESS_2026-09-28.md` | Cross-project or reference |
| [Private journal lock release](../docs/PULSAR_JOURNAL_LOCK_RELEASE.md)<br>`docs/PULSAR_JOURNAL_LOCK_RELEASE.md` | Projects and interoperability |
| [Learned CUDA visual tier execution](../docs/PULSAR_LEARNED_CUDA_TIERS.md)<br>`docs/PULSAR_LEARNED_CUDA_TIERS.md` | Models and generation |
| [LiquidAI compact assistant candidate boundaries](../docs/PULSAR_LFM_CANDIDATES.md)<br>`docs/PULSAR_LFM_CANDIDATES.md` | Cross-project or reference |
| [Explicit six-axis audio folder synthesis](../docs/PULSAR_LIBRARY_AUDIO_BUNDLES.md)<br>`docs/PULSAR_LIBRARY_AUDIO_BUNDLES.md` | Projects and interoperability |
| [Local library companion-axis inventory](../docs/PULSAR_LIBRARY_AXIS_INVENTORY.md)<br>`docs/PULSAR_LIBRARY_AXIS_INVENTORY.md` | Library and Stash |
| [Explicit migration of stopped local library batches](../docs/PULSAR_LIBRARY_MIGRATION.md)<br>`docs/PULSAR_LIBRARY_MIGRATION.md` | Library and Stash |
| [Explicit models for folder generation](../docs/PULSAR_LIBRARY_MODEL_GENERATION.md)<br>`docs/PULSAR_LIBRARY_MODEL_GENERATION.md` | Library and Stash |
| [Selected model packs in retained local library batches](../docs/PULSAR_LIBRARY_MODEL_PACKS.md)<br>`docs/PULSAR_LIBRARY_MODEL_PACKS.md` | Library and Stash |
| [Selected model packs for folder generation](../docs/PULSAR_LIBRARY_PACKS.md)<br>`docs/PULSAR_LIBRARY_PACKS.md` | Library and Stash |
| [Desktop selected-pack library controls](../docs/PULSAR_LIBRARY_PACK_CONTROLS.md)<br>`docs/PULSAR_LIBRARY_PACK_CONTROLS.md` | Library and Stash |
| [Stale selected-pack library admission](../docs/PULSAR_LIBRARY_PACK_PREFLIGHT.md)<br>`docs/PULSAR_LIBRARY_PACK_PREFLIGHT.md` | Library and Stash |
| [Fill missing channels in existing local audio bundles](../docs/PULSAR_LIBRARY_PARTIAL_BUNDLES.md)<br>`docs/PULSAR_LIBRARY_PARTIAL_BUNDLES.md` | Projects and interoperability |
| [Actual client-death recovery for existing audio-bundle channels](../docs/PULSAR_LIBRARY_PARTIAL_CLIENT_DEATH.md)<br>`docs/PULSAR_LIBRARY_PARTIAL_CLIENT_DEATH.md` | Library and Stash |
| [Acknowledgement-covered pending journal retirement](../docs/PULSAR_LIBRARY_PENDING_RETIREMENT.md)<br>`docs/PULSAR_LIBRARY_PENDING_RETIREMENT.md` | Library and Stash |
| [Retained batch preparation recovery](../docs/PULSAR_LIBRARY_PREPARATION_RECOVERY.md)<br>`docs/PULSAR_LIBRARY_PREPARATION_RECOVERY.md` | Library and Stash |
| [Desktop library audio recipe authoring](../docs/PULSAR_LIBRARY_RECIPE_AUTHORING.md)<br>`docs/PULSAR_LIBRARY_RECIPE_AUTHORING.md` | Library and Stash |
| [Actual client death during pending metadata retirement](../docs/PULSAR_LIBRARY_RETIREMENT_CLIENT_DEATH.md)<br>`docs/PULSAR_LIBRARY_RETIREMENT_CLIENT_DEATH.md` | Library and Stash |
| [Selected audio axes in retained local batches](../docs/PULSAR_LIBRARY_SELECTED_AUDIO.md)<br>`docs/PULSAR_LIBRARY_SELECTED_AUDIO.md` | Library and Stash |
| [Direct source-view authoring for retained visual library batches](../docs/PULSAR_LIBRARY_VIEW_CONTROLS.md)<br>`docs/PULSAR_LIBRARY_VIEW_CONTROLS.md` | Library and Stash |
| [Desktop visual library recipe controls](../docs/PULSAR_LIBRARY_VISUAL_CONTROLS.md)<br>`docs/PULSAR_LIBRARY_VISUAL_CONTROLS.md` | Library and Stash |
| [Linux runtime bundle checkpoint](../docs/PULSAR_LINUX_RUNTIME_BUNDLE.md)<br>`docs/PULSAR_LINUX_RUNTIME_BUNDLE.md` | Projects and interoperability |
| [Live capture receipt replay through the CLI](../docs/PULSAR_LIVE_CAPTURE_CLI_REPLAY.md)<br>`docs/PULSAR_LIVE_CAPTURE_CLI_REPLAY.md` | Live sources and capture |
| [Native timing for accepted live capture](../docs/PULSAR_LIVE_CAPTURE_CLOCK.md)<br>`docs/PULSAR_LIVE_CAPTURE_CLOCK.md` | Live sources and capture |
| [Authenticated capture-session lifecycle](../docs/PULSAR_LIVE_CAPTURE_SESSIONS.md)<br>`docs/PULSAR_LIVE_CAPTURE_SESSIONS.md` | Live sources and capture |
| [Local monotonic capture-clock observations](../docs/PULSAR_LIVE_CLOCK_OBSERVATIONS.md)<br>`docs/PULSAR_LIVE_CLOCK_OBSERVATIONS.md` | Live sources and capture |
| [Stop during a blocked native CUDA worker Finish](../docs/PULSAR_LIVE_CUDA_BLOCKED_FINISH.md)<br>`docs/PULSAR_LIVE_CUDA_BLOCKED_FINISH.md` | Live sources and capture |
| [Native public live CUDA continuation checks](../docs/PULSAR_LIVE_CUDA_CONTINUATION.md)<br>`docs/PULSAR_LIVE_CUDA_CONTINUATION.md` | Live sources and capture |
| [Paced live CUDA ingress and handoff, 2026-10-09](../docs/PULSAR_LIVE_CUDA_CONTINUOUS.md)<br>`docs/PULSAR_LIVE_CUDA_CONTINUOUS.md` | Live sources and capture |
| [Private CUDA live-stream contract](../docs/PULSAR_LIVE_CUDA_CONTRACT.md)<br>`docs/PULSAR_LIVE_CUDA_CONTRACT.md` | Live sources and capture |
| [Durable live CUDA evidence store](../docs/PULSAR_LIVE_CUDA_DURABLE_STORE.md)<br>`docs/PULSAR_LIVE_CUDA_DURABLE_STORE.md` | Live sources and capture |
| [Live CUDA final evidence admission](../docs/PULSAR_LIVE_CUDA_EVIDENCE_ADMISSION.md)<br>`docs/PULSAR_LIVE_CUDA_EVIDENCE_ADMISSION.md` | Live sources and capture |
| [Engine-owned live CUDA finalization](../docs/PULSAR_LIVE_CUDA_FINALIZATION.md)<br>`docs/PULSAR_LIVE_CUDA_FINALIZATION.md` | Live sources and capture |
| [Native CUDA consuming-finalizer boundary tests](../docs/PULSAR_LIVE_CUDA_FINALIZER_RACES.md)<br>`docs/PULSAR_LIVE_CUDA_FINALIZER_RACES.md` | Live sources and capture |
| [Issued-work binding for private live CUDA](../docs/PULSAR_LIVE_CUDA_ISSUED_WORK.md)<br>`docs/PULSAR_LIVE_CUDA_ISSUED_WORK.md` | Live sources and capture |
| [Public live CUDA measurement, 2026-10-09](../docs/PULSAR_LIVE_CUDA_MEASUREMENT.md)<br>`docs/PULSAR_LIVE_CUDA_MEASUREMENT.md` | Live sources and capture |
| [Live CUDA memory admission](../docs/PULSAR_LIVE_CUDA_MEMORY.md)<br>`docs/PULSAR_LIVE_CUDA_MEMORY.md` | Live sources and capture |
| [Live CUDA memory-age diagnosis](../docs/PULSAR_LIVE_CUDA_MEMORY_AGE_DIAGNOSIS.md)<br>`docs/PULSAR_LIVE_CUDA_MEMORY_AGE_DIAGNOSIS.md` | Live sources and capture |
| [Owned CUDA live mount custody](../docs/PULSAR_LIVE_CUDA_MOUNTS.md)<br>`docs/PULSAR_LIVE_CUDA_MOUNTS.md` | Live sources and capture |
| [Authored native persistent CUDA stream](../docs/PULSAR_LIVE_CUDA_NATIVE_STREAM.md)<br>`docs/PULSAR_LIVE_CUDA_NATIVE_STREAM.md` | Live sources and capture |
| [Live CUDA provisional reporting and Engine completion](../docs/PULSAR_LIVE_CUDA_PROVISIONAL_REPORT.md)<br>`docs/PULSAR_LIVE_CUDA_PROVISIONAL_REPORT.md` | Live sources and capture |
| [Release live CUDA measurement, 2026-10-09](../docs/PULSAR_LIVE_CUDA_RELEASE_MEASUREMENT.md)<br>`docs/PULSAR_LIVE_CUDA_RELEASE_MEASUREMENT.md` | Live sources and capture |
| [Native CUDA lease expiry inside SQLite WAL sync](../docs/PULSAR_LIVE_CUDA_SQLITE_SYNC.md)<br>`docs/PULSAR_LIVE_CUDA_SQLITE_SYNC.md` | Live sources and capture |
| [Live CUDA cold-start diagnosis](../docs/PULSAR_LIVE_CUDA_STARTUP_DIAGNOSIS.md)<br>`docs/PULSAR_LIVE_CUDA_STARTUP_DIAGNOSIS.md` | Live sources and capture |
| [Live CUDA startup process trace, 2026-10-09](../docs/PULSAR_LIVE_CUDA_STARTUP_TRACE.md)<br>`docs/PULSAR_LIVE_CUDA_STARTUP_TRACE.md` | Live sources and capture |
| [Persistent live CUDA stream implementation](../docs/PULSAR_LIVE_CUDA_STREAM.md)<br>`docs/PULSAR_LIVE_CUDA_STREAM.md` | Live sources and capture |
| [Live-development position signal baseline](../docs/PULSAR_LIVE_DEVELOPMENT_SIGNAL.md)<br>`docs/PULSAR_LIVE_DEVELOPMENT_SIGNAL.md` | Live sources and capture |
| [Explicit live epoch continuation](../docs/PULSAR_LIVE_EPOCH_CONTINUATION.md)<br>`docs/PULSAR_LIVE_EPOCH_CONTINUATION.md` | Live sources and capture |
| [Browser/live fallback implementation](../docs/PULSAR_LIVE_FALLBACK.md)<br>`docs/PULSAR_LIVE_FALLBACK.md` | Live sources and capture |
| [Authenticated live RGB frame ingress](../docs/PULSAR_LIVE_FRAME_INGRESS.md)<br>`docs/PULSAR_LIVE_FRAME_INGRESS.md` | Live sources and capture |
| [Live-reference label consistency and sampled motion](../docs/PULSAR_LIVE_LABEL_CONSISTENCY.md)<br>`docs/PULSAR_LIVE_LABEL_CONSISTENCY.md` | Live sources and capture |
| [Named live lifecycle inputs](../docs/PULSAR_LIVE_LIFECYCLE_INPUTS.md)<br>`docs/PULSAR_LIVE_LIFECYCLE_INPUTS.md` | Live sources and capture |
| [Owner-selected packs for live capture](../docs/PULSAR_LIVE_MODEL_PACKS.md)<br>`docs/PULSAR_LIVE_MODEL_PACKS.md` | Live sources and capture |
| [Living native image reuse, 2026-10-09](../docs/PULSAR_LIVE_NATIVE_IMAGE_REUSE.md)<br>`docs/PULSAR_LIVE_NATIVE_IMAGE_REUSE.md` | Live sources and capture |
| [Bounded captured-pixel ownership](../docs/PULSAR_LIVE_PIXEL_OWNERSHIP.md)<br>`docs/PULSAR_LIVE_PIXEL_OWNERSHIP.md` | Live sources and capture |
| [Declared live presentation correspondence](../docs/PULSAR_LIVE_PRESENTATION_CLOCKS.md)<br>`docs/PULSAR_LIVE_PRESENTATION_CLOCKS.md` | Live sources and capture |
| [Checked live-recording motion assembly](../docs/PULSAR_LIVE_RECORDING_ASSEMBLY.md)<br>`docs/PULSAR_LIVE_RECORDING_ASSEMBLY.md` | Live sources and capture |
| [Engine-owned live inference recording buffer](../docs/PULSAR_LIVE_RECORDING_BUFFER.md)<br>`docs/PULSAR_LIVE_RECORDING_BUFFER.md` | Live sources and capture |
| [Engine-owned live recording candidates](../docs/PULSAR_LIVE_RECORDING_CANDIDATE.md)<br>`docs/PULSAR_LIVE_RECORDING_CANDIDATE.md` | Live sources and capture |
| [Live preparation: verified blocking-read deadline gap](../docs/PULSAR_LIVE_STAGING_IO.md)<br>`docs/PULSAR_LIVE_STAGING_IO.md` | Live sources and capture |
| [B3: bounded live snapshot preparation](../docs/PULSAR_LIVE_STAGING_IO_PLAN.md)<br>`docs/PULSAR_LIVE_STAGING_IO_PLAN.md` | Live sources and capture |
| [Declared live timestamp domains](../docs/PULSAR_LIVE_TIME_BASIS.md)<br>`docs/PULSAR_LIVE_TIME_BASIS.md` | Live sources and capture |
| [Durable custody for live Linux workers](../docs/PULSAR_LIVE_WORKER_CUSTODY.md)<br>`docs/PULSAR_LIVE_WORKER_CUSTODY.md` | Live sources and capture |
| [Local assistant candidate: real inference and proposal validation](../docs/PULSAR_LOCAL_ASSISTANT_CANDIDATE.md)<br>`docs/PULSAR_LOCAL_ASSISTANT_CANDIDATE.md` | Assistant and extensions |
| [Engine-owned local assistant execution](../docs/PULSAR_LOCAL_ASSISTANT_PARENT.md)<br>`docs/PULSAR_LOCAL_ASSISTANT_PARENT.md` | Assistant and extensions |
| [Local assistant: learned worker and second candidate](../docs/PULSAR_LOCAL_ASSISTANT_WORKER.md)<br>`docs/PULSAR_LOCAL_ASSISTANT_WORKER.md` | Assistant and extensions |
| [Retained local batch cancellation](../docs/PULSAR_LOCAL_BATCH_CANCELLATION.md)<br>`docs/PULSAR_LOCAL_BATCH_CANCELLATION.md` | Library and Stash |
| [Retained local audio batch generation](../docs/PULSAR_LOCAL_BATCH_GENERATION.md)<br>`docs/PULSAR_LOCAL_BATCH_GENERATION.md` | Library and Stash |
| [Retained local batch publication handoffs](../docs/PULSAR_LOCAL_BATCH_PUBLICATION.md)<br>`docs/PULSAR_LOCAL_BATCH_PUBLICATION.md` | Library and Stash |
| [Bounded retained local batch scheduling](../docs/PULSAR_LOCAL_BATCH_SCHEDULER.md)<br>`docs/PULSAR_LOCAL_BATCH_SCHEDULER.md` | Library and Stash |
| [Automatic native manual-edit protection](../docs/PULSAR_MANUAL_EDIT_PROTECTION.md)<br>`docs/PULSAR_MANUAL_EDIT_PROTECTION.md` | Projects and interoperability |
| [Manual observed-target selection](../docs/PULSAR_MANUAL_TARGET_SELECTION.md)<br>`docs/PULSAR_MANUAL_TARGET_SELECTION.md` | Multimodal inputs |
| [Standalone media inspection](../docs/PULSAR_MEDIA_INSPECTION.md)<br>`docs/PULSAR_MEDIA_INSPECTION.md` | Multimodal inputs |
| [Audio and video generation](../docs/PULSAR_MIXED_AUDIO_VIDEO.md)<br>`docs/PULSAR_MIXED_AUDIO_VIDEO.md` | Multimodal inputs |
| [Inherited model attempt deadlines](../docs/PULSAR_MODEL_ATTEMPT_DEADLINE.md)<br>`docs/PULSAR_MODEL_ATTEMPT_DEADLINE.md` | Models and generation |
| [Generation model delivery readiness](../docs/PULSAR_MODEL_DELIVERY_READINESS.md)<br>`docs/PULSAR_MODEL_DELIVERY_READINESS.md` | Live sources and capture |
| [Owner-selected detector preview](../docs/PULSAR_MODEL_PACK_PREVIEW.md)<br>`docs/PULSAR_MODEL_PACK_PREVIEW.md` | Models and generation |
| [Model-pack approval withdrawal](../docs/PULSAR_MODEL_PACK_WITHDRAWAL.md)<br>`docs/PULSAR_MODEL_PACK_WITHDRAWAL.md` | Models and generation |
| [Neutral motion and full project evidence](../docs/PULSAR_MOTION_EVIDENCE.md)<br>`docs/PULSAR_MOTION_EVIDENCE.md` | Models and generation |
| [Paired neutral playback and full project export](../docs/PULSAR_MOTION_EVIDENCE_EXPORT.md)<br>`docs/PULSAR_MOTION_EVIDENCE_EXPORT.md` | Projects and interoperability |
| [Paired export: real lease acknowledgement loss](../docs/PULSAR_MOTION_EVIDENCE_INTERRUPTION.md)<br>`docs/PULSAR_MOTION_EVIDENCE_INTERRUPTION.md` | Models and generation |
| [Structured authored motion-retiming receipts](../docs/PULSAR_MOTION_RETIME_RECEIPTS.md)<br>`docs/PULSAR_MOTION_RETIME_RECEIPTS.md` | Models and generation |
| [Bounded larger image-motion support](../docs/PULSAR_MULTISCALE_FLOW.md)<br>`docs/PULSAR_MULTISCALE_FLOW.md` | Models and generation |
| [Native action-clock ancestry and ordinary Stash publication](../docs/PULSAR_NATIVE_ACTION_CLOCK_ANCESTRY.md)<br>`docs/PULSAR_NATIVE_ACTION_CLOCK_ANCESTRY.md` | Cross-project or reference |
| [Native candidate verification experiment, 2026-10-09](../docs/PULSAR_NATIVE_CANDIDATE_EXPERIMENT.md)<br>`docs/PULSAR_NATIVE_CANDIDATE_EXPERIMENT.md` | Cross-project or reference |
| [Coalesced native CUDA preparation](../docs/PULSAR_NATIVE_CAPTURE_COALESCING.md)<br>`docs/PULSAR_NATIVE_CAPTURE_COALESCING.md` | Live sources and capture |
| [Native capture copy candidates](../docs/PULSAR_NATIVE_CAPTURE_COPY_CANDIDATES.md)<br>`docs/PULSAR_NATIVE_CAPTURE_COPY_CANDIDATES.md` | Live sources and capture |
| [Shared native capture custody and image handoff](../docs/PULSAR_NATIVE_CAPTURE_CUSTODY.md)<br>`docs/PULSAR_NATIVE_CAPTURE_CUSTODY.md` | Live sources and capture |
| [Native capture limits and custody interface](../docs/PULSAR_NATIVE_CAPTURE_INTERFACE.md)<br>`docs/PULSAR_NATIVE_CAPTURE_INTERFACE.md` | Live sources and capture |
| [Native capture under an inherited live lease](../docs/PULSAR_NATIVE_CAPTURE_LEASE.md)<br>`docs/PULSAR_NATIVE_CAPTURE_LEASE.md` | Live sources and capture |
| [Native capture pressure, 2026-10-09](../docs/PULSAR_NATIVE_CAPTURE_PRESSURE.md)<br>`docs/PULSAR_NATIVE_CAPTURE_PRESSURE.md` | Live sources and capture |
| [Native clock origin verification and read admission](../docs/PULSAR_NATIVE_CLOCK_ORIGINS.md)<br>`docs/PULSAR_NATIVE_CLOCK_ORIGINS.md` | Cross-project or reference |
| [Native creative completion choices](../docs/PULSAR_NATIVE_CREATIVE_CAPABILITIES.md)<br>`docs/PULSAR_NATIVE_CREATIVE_CAPABILITIES.md` | Cross-project or reference |
| [Native source delivery clocks and absolute lifetime](../docs/PULSAR_NATIVE_DELIVERY_CLOCKS.md)<br>`docs/PULSAR_NATIVE_DELIVERY_CLOCKS.md` | Live sources and capture |
| [Captured native dependency bytes](../docs/PULSAR_NATIVE_DEPENDENCY_CAPTURE.md)<br>`docs/PULSAR_NATIVE_DEPENDENCY_CAPTURE.md` | Live sources and capture |
| [Native authored-edit clock origins](../docs/PULSAR_NATIVE_EDIT_CLOCK_ORIGINS.md)<br>`docs/PULSAR_NATIVE_EDIT_CLOCK_ORIGINS.md` | Projects and interoperability |
| [Owned native image preparation](../docs/PULSAR_NATIVE_IMAGE_PREPARATION.md)<br>`docs/PULSAR_NATIVE_IMAGE_PREPARATION.md` | Cross-project or reference |
| [Native image reuse runtime checkpoint, 2026-10-09](../docs/PULSAR_NATIVE_IMAGE_REUSE_RUNTIME.md)<br>`docs/PULSAR_NATIVE_IMAGE_REUSE_RUNTIME.md` | Models and generation |
| [Native multi-carrier source-time export candidate — 2026-10-09](../docs/PULSAR_NATIVE_MIXED_CLOCK_EXPORT.md)<br>`docs/PULSAR_NATIVE_MIXED_CLOCK_EXPORT.md` | Projects and interoperability |
| [Native mixed-clock runtime follow-up](../docs/PULSAR_NATIVE_MIXED_CLOCK_RUNTIME.md)<br>`docs/PULSAR_NATIVE_MIXED_CLOCK_RUNTIME.md` | Models and generation |
| [Native preparation budget checkpoint, 2026-10-09](../docs/PULSAR_NATIVE_PREPARATION_BUDGET.md)<br>`docs/PULSAR_NATIVE_PREPARATION_BUDGET.md` | Cross-project or reference |
| [Native preparation client recovery](../docs/PULSAR_NATIVE_PREPARATION_CLIENT_RECOVERY.md)<br>`docs/PULSAR_NATIVE_PREPARATION_CLIENT_RECOVERY.md` | Cross-project or reference |
| [Authenticated native preparation control](../docs/PULSAR_NATIVE_PREPARATION_CONTROL.md)<br>`docs/PULSAR_NATIVE_PREPARATION_CONTROL.md` | Cross-project or reference |
| [Native preparation I/O profile](../docs/PULSAR_NATIVE_PREPARATION_IO_PROFILE.md)<br>`docs/PULSAR_NATIVE_PREPARATION_IO_PROFILE.md` | Cross-project or reference |
| [Native preparation intent custody](../docs/PULSAR_NATIVE_PREPARATION_JOURNAL.md)<br>`docs/PULSAR_NATIVE_PREPARATION_JOURNAL.md` | Projects and interoperability |
| [Native preparation over the production transport](../docs/PULSAR_NATIVE_PREPARATION_PROCESS.md)<br>`docs/PULSAR_NATIVE_PREPARATION_PROCESS.md` | Cross-project or reference |
| [Native prepared capture authority, 2026-10-09](../docs/PULSAR_NATIVE_PREPARED_AUTHORITY.md)<br>`docs/PULSAR_NATIVE_PREPARED_AUTHORITY.md` | Architecture and assurance |
| [Native prepared CUDA handoff, 2026-10-09](../docs/PULSAR_NATIVE_PREPARED_CONTINUATION.md)<br>`docs/PULSAR_NATIVE_PREPARED_CONTINUATION.md` | Cross-project or reference |
| [Bounded encoded-source worker role](../docs/PULSAR_NATIVE_SOURCE_WORKER.md)<br>`docs/PULSAR_NATIVE_SOURCE_WORKER.md` | Multimodal inputs |
| [Native startup stage follow-up, 2026-10-09](../docs/PULSAR_NATIVE_STARTUP_STAGE_FOLLOWUP.md)<br>`docs/PULSAR_NATIVE_STARTUP_STAGE_FOLLOWUP.md` | Cross-project or reference |
| [Inspect retained neutral bundles before recovery](../docs/PULSAR_NEUTRAL_BUNDLE_INSPECTION.md)<br>`docs/PULSAR_NEUTRAL_BUNDLE_INSPECTION.md` | Projects and interoperability |
| [Atomic neutral bundle publication](../docs/PULSAR_NEUTRAL_BUNDLE_PUBLICATION.md)<br>`docs/PULSAR_NEUTRAL_BUNDLE_PUBLICATION.md` | Projects and interoperability |
| [Owner-authorized offline catalog intake](../docs/PULSAR_OFFLINE_CATALOG_INTAKE.md)<br>`docs/PULSAR_OFFLINE_CATALOG_INTAKE.md` | Distribution and resources |
| [Offline publisher-catalog verification](../docs/PULSAR_OFFLINE_CATALOG_VERIFIER.md)<br>`docs/PULSAR_OFFLINE_CATALOG_VERIFIER.md` | Distribution and resources |
| [Offline authored input workflows](../docs/PULSAR_OFFLINE_INPUT_WORKFLOWS.md)<br>`docs/PULSAR_OFFLINE_INPUT_WORKFLOWS.md` | Models and generation |
| [Offline inert model-pack installation](../docs/PULSAR_OFFLINE_MODEL_PACKS.md)<br>`docs/PULSAR_OFFLINE_MODEL_PACKS.md` | Models and generation |
| [Single-file offline runtime candidate](../docs/PULSAR_OFFLINE_RUNTIME_ARCHIVE.md)<br>`docs/PULSAR_OFFLINE_RUNTIME_ARCHIVE.md` | Models and generation |
| [First-party offline runtime installation](../docs/PULSAR_OFFLINE_RUNTIME_INSTALLATION.md)<br>`docs/PULSAR_OFFLINE_RUNTIME_INSTALLATION.md` | Models and generation |
| [Owned CUDA inference launch](../docs/PULSAR_OWNED_CUDA_LAUNCH.md)<br>`docs/PULSAR_OWNED_CUDA_LAUNCH.md` | Models and generation |
| [Owned CUDA mount custody](../docs/PULSAR_OWNED_CUDA_MOUNTS.md)<br>`docs/PULSAR_OWNED_CUDA_MOUNTS.md` | Models and generation |
| [P1D-008: preserve unresolved export recovery identity](../docs/PULSAR_P1D008_REPAIR.md)<br>`docs/PULSAR_P1D008_REPAIR.md` | Cross-project or reference |
| [Explicit pack update and rollback policy](../docs/PULSAR_PACK_UPDATE_ROLLBACK.md)<br>`docs/PULSAR_PACK_UPDATE_ROLLBACK.md` | Models and generation |
| [Parent-owned confined source launch](../docs/PULSAR_PARENT_SOURCE_LAUNCH.md)<br>`docs/PULSAR_PARENT_SOURCE_LAUNCH.md` | Multimodal inputs |
| [Synthetic proposal sample review](../docs/PULSAR_PATTERN_REVIEW.md)<br>`docs/PULSAR_PATTERN_REVIEW.md` | Multimodal inputs |
| [Persistent assistant host selection — 2026-10-12](../docs/PULSAR_PERSISTENT_ASSISTANT_SELECTION.md)<br>`docs/PULSAR_PERSISTENT_ASSISTANT_SELECTION.md` | Assistant and extensions |
| [Persistent confined temporal inference](../docs/PULSAR_PERSISTENT_LIVE_WORKER.md)<br>`docs/PULSAR_PERSISTENT_LIVE_WORKER.md` | Live sources and capture |
| [Pulsar Phase A migration and capability ledger](../docs/PULSAR_PHASE_A_MIGRATION.md)<br>`docs/PULSAR_PHASE_A_MIGRATION.md` | Cross-project or reference |
| [Pulsar Phase B defect and evidence inventory](../docs/PULSAR_PHASE_B_DEFECTS.md)<br>`docs/PULSAR_PHASE_B_DEFECTS.md` | Cross-project or reference |
| [Bounded piped encoded streams](../docs/PULSAR_PIPED_ENCODED_STREAMS.md)<br>`docs/PULSAR_PIPED_ENCODED_STREAMS.md` | Cross-project or reference |
| [Receipt-bound playback observations](../docs/PULSAR_PLAY_COMPLETION_RECONCILIATION.md)<br>`docs/PULSAR_PLAY_COMPLETION_RECONCILIATION.md` | Cross-project or reference |
| [Portable action-origin timing proofs — 2026-10-09](../docs/PULSAR_PORTABLE_ACTION_CLOCK_PROOFS.md)<br>`docs/PULSAR_PORTABLE_ACTION_CLOCK_PROOFS.md` | Projects and interoperability |
| [Portable approved assistant policy](../docs/PULSAR_PORTABLE_ASSISTANT_POLICY.md)<br>`docs/PULSAR_PORTABLE_ASSISTANT_POLICY.md` | Projects and interoperability |
| [Portable retained clock provider — 2026-10-09](../docs/PULSAR_PORTABLE_CLOCK_PROVIDER.md)<br>`docs/PULSAR_PORTABLE_CLOCK_PROVIDER.md` | Projects and interoperability |
| [Portable accepted CUDA evidence](../docs/PULSAR_PORTABLE_CUDA_EVIDENCE.md)<br>`docs/PULSAR_PORTABLE_CUDA_EVIDENCE.md` | Projects and interoperability |
| [Portable interval candidate scope](../docs/PULSAR_PORTABLE_INTERVAL_SCOPE.md)<br>`docs/PULSAR_PORTABLE_INTERVAL_SCOPE.md` | Projects and interoperability |
| [Portable unchanged-motion rebase ancestry](../docs/PULSAR_PORTABLE_REBASE_ANCESTRY.md)<br>`docs/PULSAR_PORTABLE_REBASE_ANCESTRY.md` | Projects and interoperability |
| [Portable authored retiming facts](../docs/PULSAR_PORTABLE_RETIMING_RECEIPTS.md)<br>`docs/PULSAR_PORTABLE_RETIMING_RECEIPTS.md` | Projects and interoperability |
| [Portable-project runtime checks on the current Linux build](../docs/PULSAR_PORTABLE_RUNTIME_QUALIFICATION.md)<br>`docs/PULSAR_PORTABLE_RUNTIME_QUALIFICATION.md` | Projects and interoperability |
| [Portable Stash source associations and explicit rebinding](../docs/PULSAR_PORTABLE_STASH_ASSOCIATIONS.md)<br>`docs/PULSAR_PORTABLE_STASH_ASSOCIATIONS.md` | Projects and interoperability |
| [Portable WASM invocation evidence](../docs/PULSAR_PORTABLE_WASM_INVOCATIONS.md)<br>`docs/PULSAR_PORTABLE_WASM_INVOCATIONS.md` | Projects and interoperability |
| [Prepared actor and grant boundaries, 2026-10-09](../docs/PULSAR_PREPARED_ACTORS.md)<br>`docs/PULSAR_PREPARED_ACTORS.md` | Cross-project or reference |
| [Public prepared capture capacity, 2026-10-09](../docs/PULSAR_PREPARED_CAPACITY.md)<br>`docs/PULSAR_PREPARED_CAPACITY.md` | Cross-project or reference |
| [Prepared live continuation implementation, 2026-10-09](../docs/PULSAR_PREPARED_CONTINUATION.md)<br>`docs/PULSAR_PREPARED_CONTINUATION.md` | Cross-project or reference |
| [Prepared CPU overlap and public receipt/restart, 2026-10-09](../docs/PULSAR_PREPARED_REPLAY.md)<br>`docs/PULSAR_PREPARED_REPLAY.md` | Cross-project or reference |
| [Preview artifact open-race checkpoint](../docs/PULSAR_PREVIEW_ARTIFACT_OPEN_RACE.md)<br>`docs/PULSAR_PREVIEW_ARTIFACT_OPEN_RACE.md` | Multimodal inputs |
| [Preview timestamps and clip-relative seeks](../docs/PULSAR_PREVIEW_CLOCKS.md)<br>`docs/PULSAR_PREVIEW_CLOCKS.md` | Multimodal inputs |
| [Preview pixels and decoded source-clock correspondence](../docs/PULSAR_PREVIEW_DECODED_CLOCK.md)<br>`docs/PULSAR_PREVIEW_DECODED_CLOCK.md` | Multimodal inputs |
| [Profile RAM admission](../docs/PULSAR_PROFILE_RAM_ADMISSION.md)<br>`docs/PULSAR_PROFILE_RAM_ADMISSION.md` | Cross-project or reference |
| [Protocol storage verification](../docs/PULSAR_PROTOCOL_STORAGE.md)<br>`docs/PULSAR_PROTOCOL_STORAGE.md` | Architecture and assurance |
| [Public CUDA capture Stop during blocked Finish](../docs/PULSAR_PUBLIC_CUDA_BLOCKED_FINISH.md)<br>`docs/PULSAR_PUBLIC_CUDA_BLOCKED_FINISH.md` | Models and generation |
| [Public live CUDA capture](../docs/PULSAR_PUBLIC_LIVE_CUDA.md)<br>`docs/PULSAR_PUBLIC_LIVE_CUDA.md` | Live sources and capture |
| [Qwen3 1.7B assistant candidate](../docs/PULSAR_QWEN3_17B_CANDIDATE.md)<br>`docs/PULSAR_QWEN3_17B_CANDIDATE.md` | Cross-project or reference |
| [Qwen3 4B assistant candidate boundaries](../docs/PULSAR_QWEN3_4B_CANDIDATES.md)<br>`docs/PULSAR_QWEN3_4B_CANDIDATES.md` | Cross-project or reference |
| [Real Stash authored compatibility checkpoint](../docs/PULSAR_REAL_STASH_COMPATIBILITY.md)<br>`docs/PULSAR_REAL_STASH_COMPATIBILITY.md` | Library and Stash |
| [Rebase regression executable attribution](../docs/PULSAR_REBASE_ATTRIBUTION_AUDIT.md)<br>`docs/PULSAR_REBASE_ATTRIBUTION_AUDIT.md` | Projects and interoperability |
| [Handled receipt-write failure recovery](../docs/PULSAR_RECEIPT_WRITE_FAILURES.md)<br>`docs/PULSAR_RECEIPT_WRITE_FAILURES.md` | Cross-project or reference |
| [Reference-model capacity and fixed longer-training experiment](../docs/PULSAR_REFERENCE_CAPACITY.md)<br>`docs/PULSAR_REFERENCE_CAPACITY.md` | Cross-project or reference |
| [Reference clock declarations](../docs/PULSAR_REFERENCE_CLOCK_DECLARATIONS.md)<br>`docs/PULSAR_REFERENCE_CLOCK_DECLARATIONS.md` | Cross-project or reference |
| [Prepare a reference set](../docs/PULSAR_REFERENCE_SET_BUILDER.md)<br>`docs/PULSAR_REFERENCE_SET_BUILDER.md` | Distribution and resources |
| [Independent image-relative motion evidence](../docs/PULSAR_RELATIVE_MOTION_EVIDENCE.md)<br>`docs/PULSAR_RELATIVE_MOTION_EVIDENCE.md` | Models and generation |
| [Downloading original retained CUDA evidence](../docs/PULSAR_RETAINED_LIVE_CUDA_DOWNLOAD.md)<br>`docs/PULSAR_RETAINED_LIVE_CUDA_DOWNLOAD.md` | Live sources and capture |
| [Retained live CUDA evidence inspection](../docs/PULSAR_RETAINED_LIVE_CUDA_INSPECTION.md)<br>`docs/PULSAR_RETAINED_LIVE_CUDA_INSPECTION.md` | Live sources and capture |
| [Browsing retained CUDA captures](../docs/PULSAR_RETAINED_LIVE_CUDA_LISTING.md)<br>`docs/PULSAR_RETAINED_LIVE_CUDA_LISTING.md` | Live sources and capture |
| [Removing retained CUDA evidence](../docs/PULSAR_RETAINED_LIVE_CUDA_REMOVAL.md)<br>`docs/PULSAR_RETAINED_LIVE_CUDA_REMOVAL.md` | Live sources and capture |
| [Explicit retained-runtime selection](../docs/PULSAR_RETAINED_RUNTIME_SELECTION.md)<br>`docs/PULSAR_RETAINED_RUNTIME_SELECTION.md` | Models and generation |
| [Retained visual/model library batches](../docs/PULSAR_RETAINED_VISUAL_BATCHES.md)<br>`docs/PULSAR_RETAINED_VISUAL_BATCHES.md` | Library and Stash |
| [Source-time Stash publication after authored retiming](../docs/PULSAR_RETIMED_STASH_PUBLICATION.md)<br>`docs/PULSAR_RETIMED_STASH_PUBLICATION.md` | Library and Stash |
| [Explicit review decisions](../docs/PULSAR_REVIEW_DECISIONS.md)<br>`docs/PULSAR_REVIEW_DECISIONS.md` | Multimodal inputs |
| [Grant revocation commits before worker signals](../docs/PULSAR_REVOCATION_COMMIT_ORDER.md)<br>`docs/PULSAR_REVOCATION_COMMIT_ORDER.md` | Cross-project or reference |
| [Descriptor-bound benchmark runner capture](../docs/PULSAR_RUNNER_REFLINK_CAPTURE.md)<br>`docs/PULSAR_RUNNER_REFLINK_CAPTURE.md` | Live sources and capture |
| [Explicit archive construction-stage recovery](../docs/PULSAR_RUNTIME_ARCHIVE_STAGE_RECOVERY.md)<br>`docs/PULSAR_RUNTIME_ARCHIVE_STAGE_RECOVERY.md` | Models and generation |
| [Graphical first-use preparation](../docs/PULSAR_RUNTIME_BOOTSTRAP_GUI.md)<br>`docs/PULSAR_RUNTIME_BOOTSTRAP_GUI.md` | Models and generation |
| [Offline component runtime updates](../docs/PULSAR_RUNTIME_COMPONENT_UPDATES.md)<br>`docs/PULSAR_RUNTIME_COMPONENT_UPDATES.md` | Models and generation |
| [Runtime installation stage recovery](../docs/PULSAR_RUNTIME_STAGE_RECOVERY.md)<br>`docs/PULSAR_RUNTIME_STAGE_RECOVERY.md` | Models and generation |
| [Offline changed-payload runtime archive](../docs/PULSAR_RUNTIME_UPDATE_ARCHIVE.md)<br>`docs/PULSAR_RUNTIME_UPDATE_ARCHIVE.md` | Models and generation |
| [Strict Rust CUDA worker sessions](../docs/PULSAR_RUST_CUDA_SESSIONS.md)<br>`docs/PULSAR_RUST_CUDA_SESSIONS.md` | Models and generation |
| [Saved desktop views into training intake](../docs/PULSAR_SAVED_VIEW_TRAINING_INTAKE.md)<br>`docs/PULSAR_SAVED_VIEW_TRAINING_INTAKE.md` | Multimodal inputs |
| [Standalone script repair](../docs/PULSAR_SCRIPT_REPAIR.md)<br>`docs/PULSAR_SCRIPT_REPAIR.md` | Projects and interoperability |
| [Average segment-speed diagnostics](../docs/PULSAR_SEGMENT_SPEED_DIAGNOSTICS.md)<br>`docs/PULSAR_SEGMENT_SPEED_DIAGNOSTICS.md` | Cross-project or reference |
| [Selected neutral-axis publication](../docs/PULSAR_SELECTED_AXIS_PUBLICATION.md)<br>`docs/PULSAR_SELECTED_AXIS_PUBLICATION.md` | Cross-project or reference |
| [Owner-selected model packs for offline generation](../docs/PULSAR_SELECTED_MODEL_PACKS.md)<br>`docs/PULSAR_SELECTED_MODEL_PACKS.md` | Models and generation |
| [Explicit selected source clocks](../docs/PULSAR_SELECTED_SOURCE_CLOCKS.md)<br>`docs/PULSAR_SELECTED_SOURCE_CLOCKS.md` | Multimodal inputs |
| [Self-contained Linux media tools](../docs/PULSAR_SELF_CONTAINED_MEDIA_TOOLS.md)<br>`docs/PULSAR_SELF_CONTAINED_MEDIA_TOOLS.md` | Multimodal inputs |
| [Semantic image runtime candidate](../docs/PULSAR_SEMANTIC_IMAGE_RUNTIME.md)<br>`docs/PULSAR_SEMANTIC_IMAGE_RUNTIME.md` | Models and generation |
| [Bound semantic image assistant worker](../docs/PULSAR_SEMANTIC_IMAGE_WORKER.md)<br>`docs/PULSAR_SEMANTIC_IMAGE_WORKER.md` | Cross-project or reference |
| [Snapshot capture with a restored-mtime source rewrite](../docs/PULSAR_SNAPSHOT_CAPTURE_RACE.md)<br>`docs/PULSAR_SNAPSHOT_CAPTURE_RACE.md` | Live sources and capture |
| [Source changes with restored modification time](../docs/PULSAR_SNAPSHOT_CHANGE_DETECTION.md)<br>`docs/PULSAR_SNAPSHOT_CHANGE_DETECTION.md` | Cross-project or reference |
| [Executable snapshot copy diagnosis, 2026-10-09](../docs/PULSAR_SNAPSHOT_COPY_DIAGNOSIS.md)<br>`docs/PULSAR_SNAPSHOT_COPY_DIAGNOSIS.md` | Cross-project or reference |
| [Persistent source placements](../docs/PULSAR_SOURCE_PLACEMENTS.md)<br>`docs/PULSAR_SOURCE_PLACEMENTS.md` | Multimodal inputs |
| [Spectral audio synthesis](../docs/PULSAR_SPECTRAL_AUDIO.md)<br>`docs/PULSAR_SPECTRAL_AUDIO.md` | Multimodal inputs |
| [Checked staging cleanup custody](../docs/PULSAR_STAGING_CLEANUP_CUSTODY.md)<br>`docs/PULSAR_STAGING_CLEANUP_CUSTODY.md` | Live sources and capture |
| [Staging custody after owner death](../docs/PULSAR_STAGING_CRASH_REGRESSION.md)<br>`docs/PULSAR_STAGING_CRASH_REGRESSION.md` | Live sources and capture |
| [Creation-error staging custody](../docs/PULSAR_STAGING_CREATION_CUSTODY.md)<br>`docs/PULSAR_STAGING_CREATION_CUSTODY.md` | Live sources and capture |
| [Shipped-engine staging crash recovery](../docs/PULSAR_STAGING_ENGINE_CRASH.md)<br>`docs/PULSAR_STAGING_ENGINE_CRASH.md` | Live sources and capture |
| [Focused staging diagnosis and desktop startup inspection](../docs/PULSAR_STAGING_FOCUSED_DIAGNOSIS.md)<br>`docs/PULSAR_STAGING_FOCUSED_DIAGNOSIS.md` | Live sources and capture |
| [Durable staging in the production worker fence](../docs/PULSAR_STAGING_FORMAT3.md)<br>`docs/PULSAR_STAGING_FORMAT3.md` | Live sources and capture |
| [Durable staging and creation ownership](../docs/PULSAR_STAGING_JOURNAL_PLAN.md)<br>`docs/PULSAR_STAGING_JOURNAL_PLAN.md` | Live sources and capture |
| [Staging ownership lifecycle prototype](../docs/PULSAR_STAGING_OBLIGATION_PROTOTYPE.md)<br>`docs/PULSAR_STAGING_OBLIGATION_PROTOTYPE.md` | Live sources and capture |
| [Portable projects from a format-3 host](../docs/PULSAR_STAGING_PORTABLE.md)<br>`docs/PULSAR_STAGING_PORTABLE.md` | Live sources and capture |
| [Reader requirement for durable staging](../docs/PULSAR_STAGING_READER_REQUIREMENT.md)<br>`docs/PULSAR_STAGING_READER_REQUIREMENT.md` | Live sources and capture |
| [Explicit Stash batch generation migration](../docs/PULSAR_STASH_BATCH_MIGRATION.md)<br>`docs/PULSAR_STASH_BATCH_MIGRATION.md` | Library and Stash |
| [Captured Stash batch model identity](../docs/PULSAR_STASH_BATCH_MODEL_IDENTITY.md)<br>`docs/PULSAR_STASH_BATCH_MODEL_IDENTITY.md` | Library and Stash |
| [Live Stash generation clock guard](../docs/PULSAR_STASH_GENERATION_CLOCK_GUARD.md)<br>`docs/PULSAR_STASH_GENERATION_CLOCK_GUARD.md` | Library and Stash |
| [Stash integration in the current engine](../docs/PULSAR_STASH_INTEGRATION.md)<br>`docs/PULSAR_STASH_INTEGRATION.md` | Library and Stash |
| [Owner-selected model packs in Stash batches](../docs/PULSAR_STASH_MODEL_PACKS.md)<br>`docs/PULSAR_STASH_MODEL_PACKS.md` | Library and Stash |
| [Stash publication observations](../docs/PULSAR_STASH_PUBLICATION_OBSERVATIONS.md)<br>`docs/PULSAR_STASH_PUBLICATION_OBSERVATIONS.md` | Library and Stash |
| [Source-bound placement and interval batch contract](../docs/PULSAR_STASH_SOURCE_PLANS.md)<br>`docs/PULSAR_STASH_SOURCE_PLANS.md` | Library and Stash |
| [Linux assistant bundle candidate](../docs/PULSAR_STATIC_ASSISTANT_BUNDLE.md)<br>`docs/PULSAR_STATIC_ASSISTANT_BUNDLE.md` | Projects and interoperability |
| [Observed stop completion metadata reconciliation — 2026-10-10](../docs/PULSAR_STOP_COMPLETION_RECONCILIATION.md)<br>`docs/PULSAR_STOP_COMPLETION_RECONCILIATION.md` | Cross-project or reference |
| [Ordinary-storage synchronization diagnosis](../docs/PULSAR_STORAGE_SYNCHRONIZATION_DIAGNOSIS.md)<br>`docs/PULSAR_STORAGE_SYNCHRONIZATION_DIAGNOSIS.md` | Cross-project or reference |
| [Strict native inference trace consistency](../docs/PULSAR_STRICT_INFERENCE_TRACES.md)<br>`docs/PULSAR_STRICT_INFERENCE_TRACES.md` | Models and generation |
| [Supervised CUDA policy binding](../docs/PULSAR_SUPERVISED_CUDA_POLICY.md)<br>`docs/PULSAR_SUPERVISED_CUDA_POLICY.md` | Models and generation |
| [Supervised native dependency capture](../docs/PULSAR_SUPERVISED_NATIVE_CAPTURE.md)<br>`docs/PULSAR_SUPERVISED_NATIVE_CAPTURE.md` | Live sources and capture |
| [Training and temporal script-position generation](../docs/PULSAR_TEMPORAL_POSITION_TRAINING.md)<br>`docs/PULSAR_TEMPORAL_POSITION_TRAINING.md` | Cross-project or reference |
| [Temporal training source orientation](../docs/PULSAR_TEMPORAL_TRAINING_ORIENTATION.md)<br>`docs/PULSAR_TEMPORAL_TRAINING_ORIENTATION.md` | Cross-project or reference |
| [Desktop and Stash tier admission](../docs/PULSAR_TIER_ADMISSION.md)<br>`docs/PULSAR_TIER_ADMISSION.md` | Cross-project or reference |
| [Timestamp-bound single-pass native RGB decoding](../docs/PULSAR_TIMESTAMPED_NATIVE_DECODE.md)<br>`docs/PULSAR_TIMESTAMPED_NATIVE_DECODE.md` | Cross-project or reference |
| [Checked tracker state snapshots](../docs/PULSAR_TRACKER_CHECKPOINTS.md)<br>`docs/PULSAR_TRACKER_CHECKPOINTS.md` | Models and generation |
| [Existing training pixels and source timestamps](../docs/PULSAR_TRAINING_PIXEL_CORRESPONDENCE.md)<br>`docs/PULSAR_TRAINING_PIXEL_CORRESPONDENCE.md` | Architecture and assurance |
| [Unresolved relative-flow diagnosis and publication diagnostics](../docs/PULSAR_UNRESOLVED_FLOW_DIAGNOSTICS.md)<br>`docs/PULSAR_UNRESOLVED_FLOW_DIAGNOSTICS.md` | Models and generation |
| [Declared stereo and spherical video views](../docs/PULSAR_VIDEO_VIEWS.md)<br>`docs/PULSAR_VIDEO_VIEWS.md` | Multimodal inputs |
| [Vision owner policy and CLI preparation](../docs/PULSAR_VISION_OWNER_POLICY_CLI.md)<br>`docs/PULSAR_VISION_OWNER_POLICY_CLI.md` | Cross-project or reference |
| [Bounded visual tracking tiers](../docs/PULSAR_VISUAL_TRACKING_TIERS.md)<br>`docs/PULSAR_VISUAL_TRACKING_TIERS.md` | Models and generation |
| [Direct WASM cancellation commits before signalling](../docs/PULSAR_WASM_CANCELLATION_COMMIT_ORDER.md)<br>`docs/PULSAR_WASM_CANCELLATION_COMMIT_ORDER.md` | Assistant and extensions |
| [WASM catalog recovery and unsuccessful-attempt removal](../docs/PULSAR_WASM_CATALOG_RECOVERY.md)<br>`docs/PULSAR_WASM_CATALOG_RECOVERY.md` | Assistant and extensions |
| [Completed WASM invocation archival](../docs/PULSAR_WASM_COMPLETED_ARCHIVAL.md)<br>`docs/PULSAR_WASM_COMPLETED_ARCHIVAL.md` | Assistant and extensions |
| [Data-only WASM worker](../docs/PULSAR_WASM_DATA_WORKER.md)<br>`docs/PULSAR_WASM_DATA_WORKER.md` | Assistant and extensions |
| [Authoritative resolution of unknown WASM starts](../docs/PULSAR_WASM_START_RESOLUTION.md)<br>`docs/PULSAR_WASM_START_RESOLUTION.md` | Assistant and extensions |
| [Linux WASM worker supervision](../docs/PULSAR_WASM_WORKER_SUPERVISION.md)<br>`docs/PULSAR_WASM_WORKER_SUPERVISION.md` | Assistant and extensions |
| [Checked Windows artifact boundaries](../docs/PULSAR_WINDOWS_ARTIFACT_BOUNDARIES.md)<br>`docs/PULSAR_WINDOWS_ARTIFACT_BOUNDARIES.md` | Cross-project or reference |
| [Windows retained package input](../docs/PULSAR_WINDOWS_PACKAGE_INPUT.md)<br>`docs/PULSAR_WINDOWS_PACKAGE_INPUT.md` | Models and generation |
| [Host and guest path verification](../docs/PULSAR_WINDOWS_PORTABLE_FIXTURES.md)<br>`docs/PULSAR_WINDOWS_PORTABLE_FIXTURES.md` | Projects and interoperability |
| [Windows source and portable test checkpoint](../docs/PULSAR_WINDOWS_SOURCE_CHECK.md)<br>`docs/PULSAR_WINDOWS_SOURCE_CHECK.md` | Multimodal inputs |
| [Durable worker fence primitive](../docs/PULSAR_WORKER_FENCE_PRIMITIVE.md)<br>`docs/PULSAR_WORKER_FENCE_PRIMITIVE.md` | Cross-project or reference |
| [Worker pipe deadlines](../docs/PULSAR_WORKER_PIPE_DEADLINES.md)<br>`docs/PULSAR_WORKER_PIPE_DEADLINES.md` | Cross-project or reference |
| [Worker custody before registration and spawn](../docs/PULSAR_WORKER_PRE_SPAWN_CUSTODY.md)<br>`docs/PULSAR_WORKER_PRE_SPAWN_CUSTODY.md` | Cross-project or reference |
| [Ordinary worker retirement custody](../docs/PULSAR_WORKER_RETIREMENT.md)<br>`docs/PULSAR_WORKER_RETIREMENT.md` | Cross-project or reference |
| [Verified worker snapshot reuse, 2026-10-09](../docs/PULSAR_WORKER_SNAPSHOT_REUSE.md)<br>`docs/PULSAR_WORKER_SNAPSHOT_REUSE.md` | Cross-project or reference |
| [Recorded workspace gates and request-resolution assurance](../docs/PULSAR_WORKSPACE_GATES.md)<br>`docs/PULSAR_WORKSPACE_GATES.md` | Cross-project or reference |
| [Workspace strict lint gate](../docs/PULSAR_WORKSPACE_LINT_CLOSURE.md)<br>`docs/PULSAR_WORKSPACE_LINT_CLOSURE.md` | Cross-project or reference |
| [Official YOLO11 candidate integration probe](../docs/PULSAR_YOLO11_CANDIDATE_PROBE.md)<br>`docs/PULSAR_YOLO11_CANDIDATE_PROBE.md` | Cross-project or reference |
| [ADR 0001: Phase A executable boundaries and fail-closed migration](../docs/adr/0001-phase-a-boundaries.md)<br>`docs/adr/0001-phase-a-boundaries.md` | Cross-project or reference |
| [ADR 0002: Checked perception and bounded temporal tracking](../docs/adr/0002-perception-tracking.md)<br>`docs/adr/0002-perception-tracking.md` | Models and generation |
| [ADR 0003: Constrained multimodal synthesis, not implied semantic inference](../docs/adr/0003-synthesis-modalities.md)<br>`docs/adr/0003-synthesis-modalities.md` | Cross-project or reference |
| [ADR 0004: Runtime and device research boundaries](../docs/adr/0004-runtime-device-research.md)<br>`docs/adr/0004-runtime-device-research.md` | Device playback |
| [ADR 0005: Admit the audio source clock before extracting motion features](../docs/adr/0005-audio-source-clock-admission.md)<br>`docs/adr/0005-audio-source-clock-admission.md` | Multimodal inputs |
| [ADR 0006: Revision ancestry and unresolved review history](../docs/adr/0006-revision-review-lineage.md)<br>`docs/adr/0006-revision-review-lineage.md` | Projects and interoperability |
| [ADR 0007: Move large motion programs off the control-message path](../docs/adr/0007-large-motion-artifacts.md)<br>`docs/adr/0007-large-motion-artifacts.md` | Models and generation |
| [ADR 0008: Immutable motion artifacts and scoped transfer leases](../docs/adr/0008-motion-artifacts-and-transfer-leases.md)<br>`docs/adr/0008-motion-artifacts-and-transfer-leases.md` | Models and generation |
| [ADR 0009: Portable project contracts and capture boundaries](../docs/adr/0009-portable-project-contracts.md)<br>`docs/adr/0009-portable-project-contracts.md` | Projects and interoperability |
| [ADR 0010: Retained portable-project export operations](../docs/adr/0010-portable-project-export-lifecycle.md)<br>`docs/adr/0010-portable-project-export-lifecycle.md` | Projects and interoperability |
| [ADR 0011: Fresh-project clone import and inert origin archives](../docs/adr/0011-portable-project-clone-import.md)<br>`docs/adr/0011-portable-project-clone-import.md` | Projects and interoperability |
| [ADR 0012: In-product roleplay assistant with an allowlisted tool surface](../docs/adr/0012-in-product-roleplay-assistant.md)<br>`docs/adr/0012-in-product-roleplay-assistant.md` | Roleplay, consent and optional imagery |
| [Actual GUI and native-model architecture scenario](../docs/evidence/phase-a/gui-run.md)<br>`docs/evidence/phase-a/gui-run.md` | Cross-project or reference |
| [Bounded independent Phase A repair review](../docs/evidence/phase-a/independent-review.md)<br>`docs/evidence/phase-a/independent-review.md` | Multimodal inputs |
| [Independent review: large motion transport slice](../docs/evidence/phase-b-large-motion/independent-review.md)<br>`docs/evidence/phase-b-large-motion/independent-review.md` | Models and generation |
| [P1a scoped independent review and repairs](../docs/evidence/phase-b-portable-project/independent-review.md)<br>`docs/evidence/phase-b-portable-project/independent-review.md` | Projects and interoperability |
| [P1b scoped review and regression disposition](../docs/evidence/phase-b-portable-project/p1b-independent-review.md)<br>`docs/evidence/phase-b-portable-project/p1b-independent-review.md` | Projects and interoperability |
| [P1c independent review record](../docs/evidence/phase-b-portable-project/p1c-independent-review.md)<br>`docs/evidence/phase-b-portable-project/p1c-independent-review.md` | Projects and interoperability |
| [ADR-B1-001: Independent, bounded quality measurement](../research/phase-b/ADR-B1-001-evaluation-protocol.md)<br>`research/phase-b/ADR-B1-001-evaluation-protocol.md` | Architecture and assurance |
| [ADR-B1-002: private benchmark input staging](../research/phase-b/ADR-B1-002-benchmark-input-staging.md)<br>`research/phase-b/ADR-B1-002-benchmark-input-staging.md` | Live sources and capture |
| [ADR B1-003: production campaign execution and neutral-output scoring](../research/phase-b/ADR-B1-003-production-campaign-contract.md)<br>`research/phase-b/ADR-B1-003-production-campaign-contract.md` | Architecture and assurance |
| [B2-001: explicit temporal reference-script candidate](../research/phase-b/ADR-B2-001-temporal-reference-script-candidate.md)<br>`research/phase-b/ADR-B2-001-temporal-reference-script-candidate.md` | Projects and interoperability |
| [ADR B2-002: explicit source-frame observed-target seeds](../research/phase-b/ADR-B2-002-explicit-observed-target-seeds.md)<br>`research/phase-b/ADR-B2-002-explicit-observed-target-seeds.md` | Multimodal inputs |
| [B2-002: Frozen temporal output-span diagnosis](../research/phase-b/ADR-B2-002-frozen-model-output-span-diagnosis.md)<br>`research/phase-b/ADR-B2-002-frozen-model-output-span-diagnosis.md` | Models and generation |
| [ADR B2-003: Native project-time interval regeneration](../research/phase-b/ADR-B2-003-native-project-interval-regeneration.md)<br>`research/phase-b/ADR-B2-003-native-project-interval-regeneration.md` | Projects and interoperability |
| [ADR B2-004: Portable candidate interval restrictions](../research/phase-b/ADR-B2-004-portable-candidate-interval-restrictions.md)<br>`research/phase-b/ADR-B2-004-portable-candidate-interval-restrictions.md` | Projects and interoperability |
| [ADR-B2-005: Explicit annotated temporal training views](../research/phase-b/ADR-B2-005-explicit-annotated-training-views.md)<br>`research/phase-b/ADR-B2-005-explicit-annotated-training-views.md` | Multimodal inputs |
| [B2-006: Source-bound annotated file worker](../research/phase-b/ADR-B2-006-source-bound-annotated-file-worker.md)<br>`research/phase-b/ADR-B2-006-source-bound-annotated-file-worker.md` | Multimodal inputs |
| [ADR B2-007: conditional annotated generation reader barrier](../research/phase-b/ADR-B2-007-annotated-generation-reader-barrier.md)<br>`research/phase-b/ADR-B2-007-annotated-generation-reader-barrier.md` | Models and generation |
| [ADR B2-008: annotated receipt metadata and model correspondence](../research/phase-b/ADR-B2-008-annotated-receipt-correspondence.md)<br>`research/phase-b/ADR-B2-008-annotated-receipt-correspondence.md` | Architecture and assurance |
| [ADR B2-009: retained annotated model closure and schema-3 clocks](../research/phase-b/ADR-B2-009-annotated-portable-generation.md)<br>`research/phase-b/ADR-B2-009-annotated-portable-generation.md` | Projects and interoperability |
| [ADR B2-010: explicit public annotated file generation](../research/phase-b/ADR-B2-010-public-annotated-generation.md)<br>`research/phase-b/ADR-B2-010-public-annotated-generation.md` | Models and generation |
| [ADR B3-006: Bind prepared batch models to original capture](../research/phase-b/ADR-B3-006-stash-batch-captured-model-binding.md)<br>`research/phase-b/ADR-B3-006-stash-batch-captured-model-binding.md` | Live sources and capture |
| [ADR B3-007: Explicit future-generation batch migration](../research/phase-b/ADR-B3-007-explicit-stash-batch-generation-migration.md)<br>`research/phase-b/ADR-B3-007-explicit-stash-batch-generation-migration.md` | Library and Stash |
| [ADR B3-008: Retain Stash source associations without importing authority](../research/phase-b/ADR-B3-008-portable-stash-source-associations.md)<br>`research/phase-b/ADR-B3-008-portable-stash-source-associations.md` | Projects and interoperability |
| [ADR B3-009: Resolve historical generation facts through retained namespaces](../research/phase-b/ADR-B3-009-archived-generation-origin-resolution.md)<br>`research/phase-b/ADR-B3-009-archived-generation-origin-resolution.md` | Models and generation |
| [ADR B3-010: Verify current master ancestry through archival namespaces](../research/phase-b/ADR-B3-010-retained-master-ancestry.md)<br>`research/phase-b/ADR-B3-010-retained-master-ancestry.md` | Cross-project or reference |
| [ADR B3-011: explicit retained-origin Stash publication](../research/phase-b/ADR-B3-011-archived-stash-publication.md)<br>`research/phase-b/ADR-B3-011-archived-stash-publication.md` | Library and Stash |
| [ADR B3-012: retain explicit portable rebase parent facts](../research/phase-b/ADR-B3-012-portable-rebase-ancestry.md)<br>`research/phase-b/ADR-B3-012-portable-rebase-ancestry.md` | Projects and interoperability |
| [ADR B3-013: bounded native provider trace consistency](../research/phase-b/ADR-B3-013-bounded-native-provider-traces.md)<br>`research/phase-b/ADR-B3-013-bounded-native-provider-traces.md` | Cross-project or reference |
| [ADR B3-014: explicit CUDA resource declarations](../research/phase-b/ADR-B3-014-explicit-cuda-resource-declarations.md)<br>`research/phase-b/ADR-B3-014-explicit-cuda-resource-declarations.md` | Models and generation |
| [ADR B3-015: captured native dependency sets](../research/phase-b/ADR-B3-015-captured-native-dependency-sets.md)<br>`research/phase-b/ADR-B3-015-captured-native-dependency-sets.md` | Live sources and capture |
| [ADR B3-016: cooperative GPU reservations](../research/phase-b/ADR-B3-016-cooperative-gpu-reservations.md)<br>`research/phase-b/ADR-B3-016-cooperative-gpu-reservations.md` | Cross-project or reference |
| [ADR B3-017: held CUDA device bindings](../research/phase-b/ADR-B3-017-held-cuda-device-bindings.md)<br>`research/phase-b/ADR-B3-017-held-cuda-device-bindings.md` | Device playback |
| [B3-018: Worker-side CUDA SDK identity](../research/phase-b/ADR-B3-018-worker-cuda-sdk-identity.md)<br>`research/phase-b/ADR-B3-018-worker-cuda-sdk-identity.md` | Models and generation |
| [B3-019: Held SDK identity through strict Rust CUDA sessions](../research/phase-b/ADR-B3-019-strict-rust-cuda-session.md)<br>`research/phase-b/ADR-B3-019-strict-rust-cuda-session.md` | Models and generation |
| [B3-020: Owner CUDA startup policy and real-engine preflight](../research/phase-b/ADR-B3-020-owner-cuda-startup-policy.md)<br>`research/phase-b/ADR-B3-020-owner-cuda-startup-policy.md` | Models and generation |
| [ADR B3-021: bind CUDA worker requests to owner policy and captured image](../research/phase-b/ADR-B3-021-cuda-worker-request-binding.md)<br>`research/phase-b/ADR-B3-021-cuda-worker-request-binding.md` | Models and generation |
| [ADR B3-022: independently supervise native image capture](../research/phase-b/ADR-B3-022-supervised-native-image-capture.md)<br>`research/phase-b/ADR-B3-022-supervised-native-image-capture.md` | Live sources and capture |
| [ADR B3-023: request-bound CUDA model adapters](../research/phase-b/ADR-B3-023-request-bound-cuda-model-adapters.md)<br>`research/phase-b/ADR-B3-023-request-bound-cuda-model-adapters.md` | Models and generation |
| [ADR B3-024: retain original CUDA evidence through parent egress](../research/phase-b/ADR-B3-024-original-cuda-evidence-egress.md)<br>`research/phase-b/ADR-B3-024-original-cuda-evidence-egress.md` | Models and generation |
| [ADR B3-025: durable CUDA intent and older-writer barrier](../research/phase-b/ADR-B3-025-durable-cuda-intent-reader-barrier.md)<br>`research/phase-b/ADR-B3-025-durable-cuda-intent-reader-barrier.md` | Models and generation |
| [ADR B3-026: request-bound descriptor custody for CUDA mounts](../research/phase-b/ADR-B3-026-request-bound-cuda-mount-custody.md)<br>`research/phase-b/ADR-B3-026-request-bound-cuda-mount-custody.md` | Models and generation |
| [ADR B3-027: Confined CUDA worker bootstrap](../research/phase-b/ADR-B3-027-confined-cuda-worker-bootstrap.md)<br>`research/phase-b/ADR-B3-027-confined-cuda-worker-bootstrap.md` | Models and generation |
| [ADR B3-028: Inherited model attempt deadlines](../research/phase-b/ADR-B3-028-inherited-model-attempt-deadlines.md)<br>`research/phase-b/ADR-B3-028-inherited-model-attempt-deadlines.md` | Models and generation |
| [ADR B3-029: Durable CUDA candidate evidence](../research/phase-b/ADR-B3-029-durable-cuda-candidate-evidence.md)<br>`research/phase-b/ADR-B3-029-durable-cuda-candidate-evidence.md` | Models and generation |
| [ADR B3-030: one parent deadline for ordinary worker pipes](../research/phase-b/ADR-B3-030-worker-pipe-deadlines.md)<br>`research/phase-b/ADR-B3-030-worker-pipe-deadlines.md` | Cross-project or reference |
| [ADR B3-031: reservation custody follows worker-tree retirement](../research/phase-b/ADR-B3-031-worker-retirement-custody.md)<br>`research/phase-b/ADR-B3-031-worker-retirement-custody.md` | Cross-project or reference |
| [ADR B3-032: lease plus a durable cgroup roster](../research/phase-b/ADR-B3-032-durable-worker-fence-primitive.md)<br>`research/phase-b/ADR-B3-032-durable-worker-fence-primitive.md` | Cross-project or reference |
| [ADR B3-033: custody precedes registration and spawn](../research/phase-b/ADR-B3-033-worker-pre-spawn-custody.md)<br>`research/phase-b/ADR-B3-033-worker-pre-spawn-custody.md` | Cross-project or reference |
| [ADR B3-034: bind ordinary worker ownership before admission](../research/phase-b/ADR-B3-034-engine-worker-fence-binding.md)<br>`research/phase-b/ADR-B3-034-engine-worker-fence-binding.md` | Cross-project or reference |
| [ADR B3-035: retain retirement identity across directory deletion](../research/phase-b/ADR-B3-035-journaled-cgroup-reclamation.md)<br>`research/phase-b/ADR-B3-035-journaled-cgroup-reclamation.md` | Projects and interoperability |
| [ADR B3-036: preserve accepted CUDA evidence as inert portable facts](../research/phase-b/ADR-B3-036-portable-cuda-evidence.md)<br>`research/phase-b/ADR-B3-036-portable-cuda-evidence.md` | Projects and interoperability |
| [ADR B3-037: avoid rewriting cloneable benchmark runners](../research/phase-b/ADR-B3-037-descriptor-bound-runner-capture.md)<br>`research/phase-b/ADR-B3-037-descriptor-bound-runner-capture.md` | Live sources and capture |
| [ADR B3-038: bind required staging ledger semantics before creation](../research/phase-b/ADR-B3-038-staging-reader-requirement.md)<br>`research/phase-b/ADR-B3-038-staging-reader-requirement.md` | Live sources and capture |
| [ADR B3-039: Staging obligations share the worker ownership ledger](../research/phase-b/ADR-B3-039-production-staging-ledger.md)<br>`research/phase-b/ADR-B3-039-production-staging-ledger.md` | Live sources and capture |
| [ADR B3-040: configure worker cgroups through their held directory](../research/phase-b/ADR-B3-040-held-cgroup-configuration.md)<br>`research/phase-b/ADR-B3-040-held-cgroup-configuration.md` | Cross-project or reference |
| [ADR B3-041: require the creation-aware reader before cgroup intent publication](../research/phase-b/ADR-B3-041-cgroup-creation-reader-requirement.md)<br>`research/phase-b/ADR-B3-041-cgroup-creation-reader-requirement.md` | Cross-project or reference |
| [ADR B3-042: journal creation separately from registered worker ownership](../research/phase-b/ADR-B3-042-format4-cgroup-creation-journal.md)<br>`research/phase-b/ADR-B3-042-format4-cgroup-creation-journal.md` | Projects and interoperability |
| [ADR B3-043: own the reservation before admitted cgroup creation](../research/phase-b/ADR-B3-043-admitted-cgroup-creation-custody.md)<br>`research/phase-b/ADR-B3-043-admitted-cgroup-creation-custody.md` | Cross-project or reference |
| [ADR B3-044: commit creation admission before production launch](../research/phase-b/ADR-B3-044-production-admitted-cgroup-creation.md)<br>`research/phase-b/ADR-B3-044-production-admitted-cgroup-creation.md` | Cross-project or reference |
| [B3-045: Native capture worker retirement and image handoff](../research/phase-b/ADR-B3-045-native-capture-worker-image-handoff.md)<br>`research/phase-b/ADR-B3-045-native-capture-worker-image-handoff.md` | Live sources and capture |
| [ADR B3-046: Bind CUDA metadata to the supervised capture owner](../research/phase-b/ADR-B3-046-supervised-cuda-policy-binding.md)<br>`research/phase-b/ADR-B3-046-supervised-cuda-policy-binding.md` | Models and generation |
| [ADR B3-047: Retain CUDA mount assets in shared worker custody](../research/phase-b/ADR-B3-047-owned-cuda-mount-custody.md)<br>`research/phase-b/ADR-B3-047-owned-cuda-mount-custody.md` | Models and generation |
| [ADR B3-048: Supervised CUDA memory observation and image handback](../research/phase-b/ADR-B3-048-supervised-cuda-memory-observation.md)<br>`research/phase-b/ADR-B3-048-supervised-cuda-memory-observation.md` | Models and generation |
| [ADR B3-049: Owned CUDA inference launch through shared supervision](../research/phase-b/ADR-B3-049-owned-cuda-inference-launch.md)<br>`research/phase-b/ADR-B3-049-owned-cuda-inference-launch.md` | Models and generation |
| [ADR B3-050: Engine-owned CUDA generation](../research/phase-b/ADR-B3-050-engine-cuda-generation.md)<br>`research/phase-b/ADR-B3-050-engine-cuda-generation.md` | Models and generation |
| [ADR B3-051: Owned interactive CUDA detector frame Preview](../research/phase-b/ADR-B3-051-engine-cuda-frame-preview.md)<br>`research/phase-b/ADR-B3-051-engine-cuda-frame-preview.md` | Models and generation |
| [ADR B3-052: Separate CUDA live-stream declaration and transcript](../research/phase-b/ADR-B3-052-live-cuda-stream-contract.md)<br>`research/phase-b/ADR-B3-052-live-cuda-stream-contract.md` | Live sources and capture |
| [ADR B3-053: Owned CUDA mounts for a live stream](../research/phase-b/ADR-B3-053-owned-live-cuda-mounts.md)<br>`research/phase-b/ADR-B3-053-owned-live-cuda-mounts.md` | Live sources and capture |
| [ADR B3-054: Measured live CUDA admission and sealed observer loader](../research/phase-b/ADR-B3-054-live-cuda-memory-admission.md)<br>`research/phase-b/ADR-B3-054-live-cuda-memory-admission.md` | Live sources and capture |
| [ADR B3-055: Persistent live CUDA stream and final transcript evidence](../research/phase-b/ADR-B3-055-persistent-live-cuda-stream.md)<br>`research/phase-b/ADR-B3-055-persistent-live-cuda-stream.md` | Live sources and capture |
| [ADR B3-056: Bind persistent CUDA streams to issued RGB work](../research/phase-b/ADR-B3-056-issued-live-cuda-work.md)<br>`research/phase-b/ADR-B3-056-issued-live-cuda-work.md` | Live sources and capture |
| [ADR B3-057: Preserve a live lease through native capture](../research/phase-b/ADR-B3-057-native-capture-inherited-lease.md)<br>`research/phase-b/ADR-B3-057-native-capture-inherited-lease.md` | Live sources and capture |
| [ADR B3-058: Engine-owned private live CUDA startup](../research/phase-b/ADR-B3-058-engine-owned-live-cuda-startup.md)<br>`research/phase-b/ADR-B3-058-engine-owned-live-cuda-startup.md` | Live sources and capture |
| [ADR B3-059: Retire preparation CPU admission before GPU live startup](../research/phase-b/ADR-B3-059-live-cuda-preparation-handoff.md)<br>`research/phase-b/ADR-B3-059-live-cuda-preparation-handoff.md` | Live sources and capture |
| [ADR B3-060: Explicit provisional CUDA reports with Engine clock completion](../research/phase-b/ADR-B3-060-live-cuda-provisional-report.md)<br>`research/phase-b/ADR-B3-060-live-cuda-provisional-report.md` | Live sources and capture |
| [ADR B3-061: Retain final live CUDA bytes under memory admission](../research/phase-b/ADR-B3-061-live-cuda-evidence-admission.md)<br>`research/phase-b/ADR-B3-061-live-cuda-evidence-admission.md` | Live sources and capture |
| [ADR B3-062: Conditional schema21 for original live CUDA evidence](../research/phase-b/ADR-B3-062-live-cuda-durable-store.md)<br>`research/phase-b/ADR-B3-062-live-cuda-durable-store.md` | Live sources and capture |
| [ADR B3-063: Consume the owned CUDA worker before completed publication](../research/phase-b/ADR-B3-063-live-cuda-finalization.md)<br>`research/phase-b/ADR-B3-063-live-cuda-finalization.md` | Live sources and capture |
| [ADR B3-064: Own public CUDA capture scheduling and preserve live provenance](../research/phase-b/ADR-B3-064-public-live-cuda.md)<br>`research/phase-b/ADR-B3-064-public-live-cuda.md` | Live sources and capture |
| [ADR B3-065: Observe exact capture admission receipts after boot retirement](../research/phase-b/ADR-B3-065-capture-restart-receipts.md)<br>`research/phase-b/ADR-B3-065-capture-restart-receipts.md` | Live sources and capture |
| [ADR B3-066: Preserve the original boot in explicit CLI receipt replay](../research/phase-b/ADR-B3-066-live-capture-cli-replay.md)<br>`research/phase-b/ADR-B3-066-live-capture-cli-replay.md` | Live sources and capture |
| [ADR B3-067: Project-authorized retained live CUDA inspection](../research/phase-b/ADR-B3-067-retained-live-cuda-inspection.md)<br>`research/phase-b/ADR-B3-067-retained-live-cuda-inspection.md` | Live sources and capture |
| [ADR B3-068: Original retained CUDA evidence downloads](../research/phase-b/ADR-B3-068-retained-live-cuda-download.md)<br>`research/phase-b/ADR-B3-068-retained-live-cuda-download.md` | Live sources and capture |
| [ADR B3-069: Bounded retained capture index](../research/phase-b/ADR-B3-069-retained-live-cuda-listing.md)<br>`research/phase-b/ADR-B3-069-retained-live-cuda-listing.md` | Live sources and capture |
| [ADR B3-070: Desktop capture evidence presentation](../research/phase-b/ADR-B3-070-desktop-capture-evidence.md)<br>`research/phase-b/ADR-B3-070-desktop-capture-evidence.md` | Live sources and capture |
| [ADR B3-071: Terminal evidence worker failure](../research/phase-b/ADR-B3-071-desktop-evidence-worker-failure.md)<br>`research/phase-b/ADR-B3-071-desktop-evidence-worker-failure.md` | Cross-project or reference |
| [B3-072: Explicit retained CUDA evidence removal](../research/phase-b/ADR-B3-072-retained-cuda-removal.md)<br>`research/phase-b/ADR-B3-072-retained-cuda-removal.md` | Models and generation |
| [B3-073: Desktop retained CUDA evidence removal](../research/phase-b/ADR-B3-073-desktop-cuda-removal.md)<br>`research/phase-b/ADR-B3-073-desktop-cuda-removal.md` | Models and generation |
| [B3-074: Durable desktop removal recovery](../research/phase-b/ADR-B3-074-desktop-cuda-removal-recovery.md)<br>`research/phase-b/ADR-B3-074-desktop-cuda-removal-recovery.md` | Models and generation |
| [ADR B3-075: annotated Stash publication through shipped portable download](../research/phase-b/ADR-B3-075-annotated-stash-publication.md)<br>`research/phase-b/ADR-B3-075-annotated-stash-publication.md` | Library and Stash |
| [ADR B3-076: per-source annotated batch intent](../research/phase-b/ADR-B3-076-annotated-stash-batches.md)<br>`research/phase-b/ADR-B3-076-annotated-stash-batches.md` | Library and Stash |
| [ADR B3-077: selected annotated CPU pack generation](../research/phase-b/ADR-B3-077-annotated-model-packs.md)<br>`research/phase-b/ADR-B3-077-annotated-model-packs.md` | Models and generation |
| [ADR B3-078: annotated Stash batches from selected packs](../research/phase-b/ADR-B3-078-annotated-stash-pack-batches.md)<br>`research/phase-b/ADR-B3-078-annotated-stash-pack-batches.md` | Library and Stash |
| [ADR B3-079: explicit source-bound annotated Preview](../research/phase-b/ADR-B3-079-annotated-preview.md)<br>`research/phase-b/ADR-B3-079-annotated-preview.md` | Multimodal inputs |
| [ADR B3-080: explicit annotated CPU Preview in the desktop](../research/phase-b/ADR-B3-080-annotated-desktop-preview.md)<br>`research/phase-b/ADR-B3-080-annotated-desktop-preview.md` | Multimodal inputs |
| [ADR B3-081: explicit model-pack approval withdrawal](../research/phase-b/ADR-B3-081-model-pack-approval-withdrawal.md)<br>`research/phase-b/ADR-B3-081-model-pack-approval-withdrawal.md` | Models and generation |
| [ADR B3-082: untrusted local-assistant proposal foundation](../research/phase-b/ADR-B3-082-local-assistant-proposal-foundation.md)<br>`research/phase-b/ADR-B3-082-local-assistant-proposal-foundation.md` | Assistant and extensions |
| [ADR B3-083: one-shot learned assistant worker](../research/phase-b/ADR-B3-083-one-shot-learned-assistant-worker.md)<br>`research/phase-b/ADR-B3-083-one-shot-learned-assistant-worker.md` | Assistant and extensions |
| [ADR B3-084: Engine-owned assistant execution](../research/phase-b/ADR-B3-084-engine-owned-assistant-execution.md)<br>`research/phase-b/ADR-B3-084-engine-owned-assistant-execution.md` | Assistant and extensions |
| [ADR B3-085: authenticated assistant proposal attempts](../research/phase-b/ADR-B3-085-authenticated-assistant-attempts.md)<br>`research/phase-b/ADR-B3-085-authenticated-assistant-attempts.md` | Assistant and extensions |
| [ADR B3-086: private assistant host policy and exact first-party intents](../research/phase-b/ADR-B3-086-private-assistant-host-policy-and-cli.md)<br>`research/phase-b/ADR-B3-086-private-assistant-host-policy-and-cli.md` | Assistant and extensions |
| [ADR B3-087: desktop assistant intent and view bindings](../research/phase-b/ADR-B3-087-desktop-assistant-intent-and-view-bindings.md)<br>`research/phase-b/ADR-B3-087-desktop-assistant-intent-and-view-bindings.md` | Assistant and extensions |
| [ADR B3-088: resolve accepted assistant starts or withdraw unpublished IDs](../research/phase-b/ADR-B3-088-assistant-start-resolution.md)<br>`research/phase-b/ADR-B3-088-assistant-start-resolution.md` | Assistant and extensions |
| [ADR B3-089: bind assistant proposals to explicit generation approval](../research/phase-b/ADR-B3-089-assistant-approved-generation.md)<br>`research/phase-b/ADR-B3-089-assistant-approved-generation.md` | Models and generation |
| [ADR B3-090: explain recorded reviews through authenticated evidence reads](../research/phase-b/ADR-B3-090-assistant-recorded-review-evidence.md)<br>`research/phase-b/ADR-B3-090-assistant-recorded-review-evidence.md` | Assistant and extensions |
| [ADR B3-091: persist exact assistant generation approval before dispatch](../research/phase-b/ADR-B3-091-assistant-generation-intent-and-desktop-handoff.md)<br>`research/phase-b/ADR-B3-091-assistant-generation-intent-and-desktop-handoff.md` | Models and generation |
| [ADR B3-092: capture checked preview target references](../research/phase-b/ADR-B3-092-checked-assistant-target-catalog.md)<br>`research/phase-b/ADR-B3-092-checked-assistant-target-catalog.md` | Assistant and extensions |
| [ADR B3-093: apply retained observed target proposals through normal generation](../research/phase-b/ADR-B3-093-assistant-target-application.md)<br>`research/phase-b/ADR-B3-093-assistant-target-application.md` | Assistant and extensions |
| [ADR B3-094: approve fixed assistant recipes through authored candidates](../research/phase-b/ADR-B3-094-assistant-edit-recipes.md)<br>`research/phase-b/ADR-B3-094-assistant-edit-recipes.md` | Projects and interoperability |
| [ADR B3-095: persist exact assistant edit declarations before optional dispatch](../research/phase-b/ADR-B3-095-saved-assistant-edit-recovery.md)<br>`research/phase-b/ADR-B3-095-saved-assistant-edit-recovery.md` | Projects and interoperability |
| [ADR B3-096: lend owned offline admission to authenticated assistant work](../research/phase-b/ADR-B3-096-assistant-memory-handoff.md)<br>`research/phase-b/ADR-B3-096-assistant-memory-handoff.md` | Assistant and extensions |
| [ADR B3-097: bound assistant journal cells before decoding](../research/phase-b/ADR-B3-097-assistant-journal-capture-admission.md)<br>`research/phase-b/ADR-B3-097-assistant-journal-capture-admission.md` | Live sources and capture |
| [ADR B3-098: retry owned assistant recovery after capacity release](../research/phase-b/ADR-B3-098-assistant-capacity-release-recovery.md)<br>`research/phase-b/ADR-B3-098-assistant-capacity-release-recovery.md` | Assistant and extensions |
| [ADR B3-099: portable approved assistant asset paths](../research/phase-b/ADR-B3-099-portable-assistant-policy-assets.md)<br>`research/phase-b/ADR-B3-099-portable-assistant-policy-assets.md` | Projects and interoperability |
| [ADR B3-100: local assistant host setup](../research/phase-b/ADR-B3-100-local-assistant-host-setup.md)<br>`research/phase-b/ADR-B3-100-local-assistant-host-setup.md` | Assistant and extensions |
| [ADR B3-101: bundled CPU runtime discovery](../research/phase-b/ADR-B3-101-bundled-cpu-runtime-discovery.md)<br>`research/phase-b/ADR-B3-101-bundled-cpu-runtime-discovery.md` | Projects and interoperability |
| [ADR B3-102: offline runtime candidate installation](../research/phase-b/ADR-B3-102-offline-runtime-candidate-installation.md)<br>`research/phase-b/ADR-B3-102-offline-runtime-candidate-installation.md` | Models and generation |
| [ADR B3-103: runtime installation stage recovery](../research/phase-b/ADR-B3-103-runtime-installation-stage-recovery.md)<br>`research/phase-b/ADR-B3-103-runtime-installation-stage-recovery.md` | Models and generation |
| [ADR B3-104: single-file offline runtime candidate archive](../research/phase-b/ADR-B3-104-offline-runtime-archive.md)<br>`research/phase-b/ADR-B3-104-offline-runtime-archive.md` | Models and generation |
| [ADR B3-105: persistent runtime custody for AppImage clients](../research/phase-b/ADR-B3-105-persistent-appimage-runtime-custody.md)<br>`research/phase-b/ADR-B3-105-persistent-appimage-runtime-custody.md` | Models and generation |
| [ADR B3-106: restore actual AppImage extraction cleanup under musl](../research/phase-b/ADR-B3-106-appimage-musl-cleanup.md)<br>`research/phase-b/ADR-B3-106-appimage-musl-cleanup.md` | Cross-project or reference |
| [ADR B3-107: show preparation before graphical packaged launch](../research/phase-b/ADR-B3-107-runtime-bootstrap-window.md)<br>`research/phase-b/ADR-B3-107-runtime-bootstrap-window.md` | Models and generation |
| [ADR B3-108: explicitly select retained runtime assets](../research/phase-b/ADR-B3-108-explicit-retained-runtime-selection.md)<br>`research/phase-b/ADR-B3-108-explicit-retained-runtime-selection.md` | Models and generation |
| [ADR B3-109: explicit fixed detector letterbox](../research/phase-b/ADR-B3-109-explicit-detector-letterbox.md)<br>`research/phase-b/ADR-B3-109-explicit-detector-letterbox.md` | Cross-project or reference |
| [ADR B3-110: checked generation detector score](../research/phase-b/ADR-B3-110-checked-generation-detector-score.md)<br>`research/phase-b/ADR-B3-110-checked-generation-detector-score.md` | Models and generation |
| [ADR B3-111: model-aware analysis dimensions](../research/phase-b/ADR-B3-111-model-aware-analysis-dimensions.md)<br>`research/phase-b/ADR-B3-111-model-aware-analysis-dimensions.md` | Models and generation |
| [ADR B3-112: generation target/background flow diagnostics](../research/phase-b/ADR-B3-112-generation-flow-diagnostics.md)<br>`research/phase-b/ADR-B3-112-generation-flow-diagnostics.md` | Models and generation |
| [ADR B3-113: bounded coarse proposals with full-resolution flow checks](../research/phase-b/ADR-B3-113-bounded-multiscale-flow.md)<br>`research/phase-b/ADR-B3-113-bounded-multiscale-flow.md` | Models and generation |
| [ADR-B3-114: scope ambiguous association to affected identities](../research/phase-b/ADR-B3-114-scoped-target-association.md)<br>`research/phase-b/ADR-B3-114-scoped-target-association.md` | Multimodal inputs |
| [ADR-B3-115: checked saved-view training intake](../research/phase-b/ADR-B3-115-saved-view-training-intake.md)<br>`research/phase-b/ADR-B3-115-saved-view-training-intake.md` | Multimodal inputs |
| [ADR-B3-116: offline component runtime update](../research/phase-b/ADR-B3-116-offline-component-runtime-update.md)<br>`research/phase-b/ADR-B3-116-offline-component-runtime-update.md` | Models and generation |
| [ADR-B3-117: offline changed-payload runtime archive](../research/phase-b/ADR-B3-117-offline-runtime-update-archive.md)<br>`research/phase-b/ADR-B3-117-offline-runtime-update-archive.md` | Models and generation |
| [ADR-B3-118: explicit archive construction-stage recovery](../research/phase-b/ADR-B3-118-explicit-archive-stage-recovery.md)<br>`research/phase-b/ADR-B3-118-explicit-archive-stage-recovery.md` | Cross-project or reference |
| [ADR-B3-119: explicit axis-pattern clients and shared admission preflight](../research/phase-b/ADR-B3-119-explicit-axis-pattern-clients.md)<br>`research/phase-b/ADR-B3-119-explicit-axis-pattern-clients.md` | Cross-project or reference |
| [ADR-B3-120: constrained learned synthetic-motion proposals](../research/phase-b/ADR-B3-120-creative-pattern-proposals.md)<br>`research/phase-b/ADR-B3-120-creative-pattern-proposals.md` | Cross-project or reference |
| [ADR-B3-121: native grammar choices before their parameters](../research/phase-b/ADR-B3-121-native-schema-property-order.md)<br>`research/phase-b/ADR-B3-121-native-schema-property-order.md` | Cross-project or reference |
| [ADR-B3-122: separately approved learned creative candidates](../research/phase-b/ADR-B3-122-approved-creative-candidates.md)<br>`research/phase-b/ADR-B3-122-approved-creative-candidates.md` | Cross-project or reference |
| [ADR-B3-123: desktop synthetic proposal review and recovery](../research/phase-b/ADR-B3-123-desktop-creative-review.md)<br>`research/phase-b/ADR-B3-123-desktop-creative-review.md` | Multimodal inputs |
| [ADR-B3-124: Core-valid creative pulse choices and channel tuples](../research/phase-b/ADR-B3-124-creative-pulse-tuples.md)<br>`research/phase-b/ADR-B3-124-creative-pulse-tuples.md` | Cross-project or reference |
| [ADR-B3-125: native CPU generation and assistant contention](../research/phase-b/ADR-B3-125-native-assistant-generation-handoff.md)<br>`research/phase-b/ADR-B3-125-native-assistant-generation-handoff.md` | Models and generation |
| [ADR-B3-126: distinct bounded visual tracking plans](../research/phase-b/ADR-B3-126-bounded-visual-tracking-tiers.md)<br>`research/phase-b/ADR-B3-126-bounded-visual-tracking-tiers.md` | Models and generation |
| [ADR-B3-127: shared executable tier admission](../research/phase-b/ADR-B3-127-shared-tier-admission.md)<br>`research/phase-b/ADR-B3-127-shared-tier-admission.md` | Cross-project or reference |
| [ADR B3-128: Profile RAM within owner limits](../research/phase-b/ADR-B3-128-profile-ram-admission.md)<br>`research/phase-b/ADR-B3-128-profile-ram-admission.md` | Cross-project or reference |
| [ADR B3-129: Aggregate admitted worker CPU quotas](../research/phase-b/ADR-B3-129-aggregate-worker-cpu-admission.md)<br>`research/phase-b/ADR-B3-129-aggregate-worker-cpu-admission.md` | Models and generation |
| [ADR B3-130: Bound image conditioning in the assistant worker](../research/phase-b/ADR-B3-130-bound-image-assistant-worker.md)<br>`research/phase-b/ADR-B3-130-bound-image-assistant-worker.md` | Assistant and extensions |
| [ADR B3-131: Authenticated prepared-image assistant requests](../research/phase-b/ADR-B3-131-authenticated-prepared-image-assistant.md)<br>`research/phase-b/ADR-B3-131-authenticated-prepared-image-assistant.md` | Assistant and extensions |
| [ADR B3-132: Vision owner policy and client preparation](../research/phase-b/ADR-B3-132-vision-owner-policy-and-cli.md)<br>`research/phase-b/ADR-B3-132-vision-owner-policy-and-cli.md` | Cross-project or reference |
| [ADR B3-133: Desktop image assistant preparation](../research/phase-b/ADR-B3-133-desktop-image-assistant.md)<br>`research/phase-b/ADR-B3-133-desktop-image-assistant.md` | Assistant and extensions |
| [ADR B3-134: Image-conditioned generation origin](../research/phase-b/ADR-B3-134-image-generation-origin.md)<br>`research/phase-b/ADR-B3-134-image-generation-origin.md` | Models and generation |
| [ADR B3-135: Image-origin portable lifecycle](../research/phase-b/ADR-B3-135-image-portable-lifecycle.md)<br>`research/phase-b/ADR-B3-135-image-portable-lifecycle.md` | Projects and interoperability |
| [ADR B3-136: Preceding image-origin reader boundary](../research/phase-b/ADR-B3-136-image-preceding-reader.md)<br>`research/phase-b/ADR-B3-136-image-preceding-reader.md` | Cross-project or reference |
| [ADR B3-137: explicit persistent assistant host selection](../research/phase-b/ADR-B3-137-persistent-assistant-host-selection.md)<br>`research/phase-b/ADR-B3-137-persistent-assistant-host-selection.md` | Assistant and extensions |
| [ADR B3-138: desktop assistant host settings](../research/phase-b/ADR-B3-138-desktop-assistant-host-settings.md)<br>`research/phase-b/ADR-B3-138-desktop-assistant-host-settings.md` | Assistant and extensions |
| [ADR B3-139: explicit spectral audio analysis](../research/phase-b/ADR-B3-139-spectral-audio-analysis.md)<br>`research/phase-b/ADR-B3-139-spectral-audio-analysis.md` | Multimodal inputs |
| [ADR B3-140: audio-modulated oscillation](../research/phase-b/ADR-B3-140-audio-modulated-oscillation.md)<br>`research/phase-b/ADR-B3-140-audio-modulated-oscillation.md` | Multimodal inputs |
| [ADR B3-141: standalone script repair through shared authority](../research/phase-b/ADR-B3-141-shared-script-repair.md)<br>`research/phase-b/ADR-B3-141-shared-script-repair.md` | Projects and interoperability |
| [ADR B3-142: desktop repair proposals](../research/phase-b/ADR-B3-142-desktop-script-repair.md)<br>`research/phase-b/ADR-B3-142-desktop-script-repair.md` | Projects and interoperability |
| [ADR B3-143: atomic Linux recovery receipts](../research/phase-b/ADR-B3-143-atomic-linux-receipts.md)<br>`research/phase-b/ADR-B3-143-atomic-linux-receipts.md` | Cross-project or reference |
| [ADR B3-144: desktop Handy observation](../research/phase-b/ADR-B3-144-desktop-handy-inspection.md)<br>`research/phase-b/ADR-B3-144-desktop-handy-inspection.md` | Device playback |
| [ADR B3-145: Windows source boundaries and portable test evidence](../research/phase-b/ADR-B3-145-windows-source-boundaries.md)<br>`research/phase-b/ADR-B3-145-windows-source-boundaries.md` | Multimodal inputs |
| [ADR B3-146: Keep host paths and guest coordinates in their own namespaces](../research/phase-b/ADR-B3-146-host-and-guest-path-domains.md)<br>`research/phase-b/ADR-B3-146-host-and-guest-path-domains.md` | Cross-project or reference |
| [ADR B3-147: Retain Windows package input with checked open semantics](../research/phase-b/ADR-B3-147-windows-retained-package-input.md)<br>`research/phase-b/ADR-B3-147-windows-retained-package-input.md` | Models and generation |
| [ADR B3-148: Do not invent Windows artifact privacy or durability](../research/phase-b/ADR-B3-148-checked-windows-artifact-boundaries.md)<br>`research/phase-b/ADR-B3-148-checked-windows-artifact-boundaries.md` | Cross-project or reference |
| [ADR B3-149: Detect observable source changes despite restored modification time](../research/phase-b/ADR-B3-149-observable-source-change-metadata.md)<br>`research/phase-b/ADR-B3-149-observable-source-change-metadata.md` | Multimodal inputs |
| [ADR B3-150: Validate native completions against the offered creative palette](../research/phase-b/ADR-B3-150-native-creative-palette.md)<br>`research/phase-b/ADR-B3-150-native-creative-palette.md` | Cross-project or reference |
| [ADR B3-151: Recheck native creative choices at Engine approval](../research/phase-b/ADR-B3-151-creative-approval-palette.md)<br>`research/phase-b/ADR-B3-151-creative-approval-palette.md` | Cross-project or reference |
| [ADR B3-152: inspect captured media through a scoped worker query](../research/phase-b/ADR-B3-152-media-inspection-query.md)<br>`research/phase-b/ADR-B3-152-media-inspection-query.md` | Multimodal inputs |
| [ADR B3-153: shared visual tier admission and bounded background recovery](../research/phase-b/ADR-B3-153-cli-visual-tiers.md)<br>`research/phase-b/ADR-B3-153-cli-visual-tiers.md` | Cross-project or reference |
| [ADR B3-154: folder consumers share explicit file-generation declarations](../research/phase-b/ADR-B3-154-library-model-generation.md)<br>`research/phase-b/ADR-B3-154-library-model-generation.md` | Library and Stash |
| [ADR B3-155: explicit six-axis audio synthesis](../research/phase-b/ADR-B3-155-audio-bundle.md)<br>`research/phase-b/ADR-B3-155-audio-bundle.md` | Projects and interoperability |
| [ADR B3-156: desktop authors checked six-axis audio recipes](../research/phase-b/ADR-B3-156-desktop-audio-bundle.md)<br>`research/phase-b/ADR-B3-156-desktop-audio-bundle.md` | Projects and interoperability |
| [ADR B3-157: atomic neutral bundle publication with exact export recovery](../research/phase-b/ADR-B3-157-neutral-bundle-publication.md)<br>`research/phase-b/ADR-B3-157-neutral-bundle-publication.md` | Projects and interoperability |
| [ADR B3-158: desktop neutral bundle transfer and explicit recovery](../research/phase-b/ADR-B3-158-desktop-neutral-bundle.md)<br>`research/phase-b/ADR-B3-158-desktop-neutral-bundle.md` | Projects and interoperability |
| [ADR B3-159: exact selected packs for folder consumers](../research/phase-b/ADR-B3-159-library-selected-packs.md)<br>`research/phase-b/ADR-B3-159-library-selected-packs.md` | Library and Stash |
| [ADR B3-160: cleanup receipt lock follows its owner lifetime](../research/phase-b/ADR-B3-160-cleanup-lock-owner-lifetime.md)<br>`research/phase-b/ADR-B3-160-cleanup-lock-owner-lifetime.md` | Cross-project or reference |
| [ADR B3-161: explicit audio bundles in folder consumers](../research/phase-b/ADR-B3-161-library-audio-bundles.md)<br>`research/phase-b/ADR-B3-161-library-audio-bundles.md` | Projects and interoperability |
| [ADR B3-162: authenticated observational bundle inspection](../research/phase-b/ADR-B3-162-neutral-bundle-inspection.md)<br>`research/phase-b/ADR-B3-162-neutral-bundle-inspection.md` | Projects and interoperability |
| [ADR B3-163: Engine-owned local batch input and generation phases](../research/phase-b/ADR-B3-163-local-batch-generation-ledger.md)<br>`research/phase-b/ADR-B3-163-local-batch-generation-ledger.md` | Library and Stash |
| [ADR B3-164: explicit local batch metadata release](../research/phase-b/ADR-B3-164-local-batch-release.md)<br>`research/phase-b/ADR-B3-164-local-batch-release.md` | Library and Stash |
| [ADR B3-165: client-owned local batch publication handoffs](../research/phase-b/ADR-B3-165-local-batch-publication-handoff.md)<br>`research/phase-b/ADR-B3-165-local-batch-publication-handoff.md` | Library and Stash |
| [ADR B3-166: bounded retained local batch scheduler](../research/phase-b/ADR-B3-166-local-batch-scheduler.md)<br>`research/phase-b/ADR-B3-166-local-batch-scheduler.md` | Library and Stash |
| [ADR B3-167: durable local batch cancellation and partial metadata release](../research/phase-b/ADR-B3-167-local-batch-cancellation.md)<br>`research/phase-b/ADR-B3-167-local-batch-cancellation.md` | Library and Stash |
| [ADR B3-168: desktop retained audio library workflow](../research/phase-b/ADR-B3-168-desktop-library-batches.md)<br>`research/phase-b/ADR-B3-168-desktop-library-batches.md` | Library and Stash |
| [ADR B3-169: shared desktop audio recipe authoring](../research/phase-b/ADR-B3-169-library-audio-recipe-controls.md)<br>`research/phase-b/ADR-B3-169-library-audio-recipe-controls.md` | Library and Stash |
| [ADR B3-170: original retained batch preparation recovery](../research/phase-b/ADR-B3-170-original-batch-preparation-recovery.md)<br>`research/phase-b/ADR-B3-170-original-batch-preparation-recovery.md` | Library and Stash |
| [ADR B3-171: desktop original preparation recovery](../research/phase-b/ADR-B3-171-desktop-preparation-recovery.md)<br>`research/phase-b/ADR-B3-171-desktop-preparation-recovery.md` | Cross-project or reference |
| [ADR B3-172 — selected neutral-axis publication](../research/phase-b/ADR-B3-172-selected-axis-publication.md)<br>`research/phase-b/ADR-B3-172-selected-axis-publication.md` | Cross-project or reference |
| [ADR B3-173 — retained visual/model library batches](../research/phase-b/ADR-B3-173-retained-visual-batches.md)<br>`research/phase-b/ADR-B3-173-retained-visual-batches.md` | Library and Stash |
| [ADR B3-174 — direct desktop visual library recipe controls](../research/phase-b/ADR-B3-174-library-visual-controls.md)<br>`research/phase-b/ADR-B3-174-library-visual-controls.md` | Library and Stash |
| [ADR B3-175 — selected packs in retained local library batches](../research/phase-b/ADR-B3-175-library-model-packs.md)<br>`research/phase-b/ADR-B3-175-library-model-packs.md` | Library and Stash |
| [ADR B3-176 — direct desktop selected-pack library controls](../research/phase-b/ADR-B3-176-library-pack-controls.md)<br>`research/phase-b/ADR-B3-176-library-pack-controls.md` | Library and Stash |
| [ADR B3-177 — stale pack refusal before local batch generation intent](../research/phase-b/ADR-B3-177-library-pack-preflight.md)<br>`research/phase-b/ADR-B3-177-library-pack-preflight.md` | Library and Stash |
| [ADR B3-178 — explicit linked continuation of stopped local batches](../research/phase-b/ADR-B3-178-library-migration.md)<br>`research/phase-b/ADR-B3-178-library-migration.md` | Library and Stash |
| [ADR B3-179: observe library companions per axis before partial generation](../research/phase-b/ADR-B3-179-library-axis-inventory.md)<br>`research/phase-b/ADR-B3-179-library-axis-inventory.md` | Library and Stash |
| [ADR B3-180: retain audio output-axis selection independently of synthesis](../research/phase-b/ADR-B3-180-library-selected-audio.md)<br>`research/phase-b/ADR-B3-180-library-selected-audio.md` | Library and Stash |
| [ADR B3-181: existing-directory channel publication with pinned authority](../research/phase-b/ADR-B3-181-library-partial-bundles.md)<br>`research/phase-b/ADR-B3-181-library-partial-bundles.md` | Projects and interoperability |
| [ADR B3-182: Direct retained-library source-view controls](../research/phase-b/ADR-B3-182-library-view-controls.md)<br>`research/phase-b/ADR-B3-182-library-view-controls.md` | Library and Stash |
| [ADR B3-183: Report selected-axis unresolved coverage before ordinary publication](../research/phase-b/ADR-B3-183-unresolved-flow-diagnostics.md)<br>`research/phase-b/ADR-B3-183-unresolved-flow-diagnostics.md` | Models and generation |
| [ADR B3-184: Retain acknowledgement-covered pending metadata without deleting bytes](../research/phase-b/ADR-B3-184-library-pending-retirement.md)<br>`research/phase-b/ADR-B3-184-library-pending-retirement.md` | Library and Stash |
| [ADR B3-185: Browser frames feed existing live authority; reserve actual CPU launch limits](../research/phase-b/ADR-B3-185-browser-live-preview.md)<br>`research/phase-b/ADR-B3-185-browser-live-preview.md` | Live sources and capture |
| [ADR B3-186: Browser playback uses prepared Engine successors without extending leases](../research/phase-b/ADR-B3-186-browser-live-continuation.md)<br>`research/phase-b/ADR-B3-186-browser-live-continuation.md` | Live sources and capture |
| [ADR B3-187: Record accepted live inference under Engine ownership](../research/phase-b/ADR-B3-187-live-recording-buffer.md)<br>`research/phase-b/ADR-B3-187-live-recording-buffer.md` | Live sources and capture |
| [ADR B3-188: Opt-in browser recording with bounded stopped-capture downloads](../research/phase-b/ADR-B3-188-browser-recording-controls.md)<br>`research/phase-b/ADR-B3-188-browser-recording-controls.md` | Live sources and capture |
| [ADR B3-189: Assemble accepted live points without erasing missing coverage](../research/phase-b/ADR-B3-189-live-recording-assembly.md)<br>`research/phase-b/ADR-B3-189-live-recording-assembly.md` | Live sources and capture |
| [ADR B3-190: Retain original live recordings as a distinct candidate source](../research/phase-b/ADR-B3-190-live-recording-candidate.md)<br>`research/phase-b/ADR-B3-190-live-recording-candidate.md` | Live sources and capture |
| [ADR B3-191: Expose candidate intent before browser staging effects](../research/phase-b/ADR-B3-191-browser-recording-candidates.md)<br>`research/phase-b/ADR-B3-191-browser-recording-candidates.md` | Live sources and capture |
| [ADR B3-192: Resolve selected gaps as authored values proposals](../research/phase-b/ADR-B3-192-gap-resolution.md)<br>`research/phase-b/ADR-B3-192-gap-resolution.md` | Cross-project or reference |
| [ADR B3-193: Bind browser commitment to a reviewed proposal and original intent](../research/phase-b/ADR-B3-193-browser-recording-commit.md)<br>`research/phase-b/ADR-B3-193-browser-recording-commit.md` | Live sources and capture |
| [ADR B3-194: Export an inspected master through a retained original intent](../research/phase-b/ADR-B3-194-browser-script-download.md)<br>`research/phase-b/ADR-B3-194-browser-script-download.md` | Live sources and capture |
| [ADR B3-195: Bind browser authored repair to inspected committed gaps](../research/phase-b/ADR-B3-195-browser-gap-proposals.md)<br>`research/phase-b/ADR-B3-195-browser-gap-proposals.md` | Live sources and capture |
| [ADR B3-196: Resume original values uploads after lost outcomes](../research/phase-b/ADR-B3-196-browser-gap-recovery.md)<br>`research/phase-b/ADR-B3-196-browser-gap-recovery.md` | Live sources and capture |
| [ADR B3-197: Persist original browser gap uploads without restoring authority](../research/phase-b/ADR-B3-197-browser-gap-journal.md)<br>`research/phase-b/ADR-B3-197-browser-gap-journal.md` | Live sources and capture |
| [ADR B3-198: Retain an explicitly approved original gap commit before effects](../research/phase-b/ADR-B3-198-browser-gap-commit-recovery.md)<br>`research/phase-b/ADR-B3-198-browser-gap-commit-recovery.md` | Live sources and capture |
| [ADR B3-199: Recover only the original pinned browser script export](../research/phase-b/ADR-B3-199-browser-export-recovery.md)<br>`research/phase-b/ADR-B3-199-browser-export-recovery.md` | Live sources and capture |
| [ADR B3-200: Retire confirmed pointers while retaining original export facts](../research/phase-b/ADR-B3-200-browser-export-retirement.md)<br>`research/phase-b/ADR-B3-200-browser-export-retirement.md` | Live sources and capture |
| [ADR B3-201: Preserve empty stopped scopes without fabricating motion](../research/phase-b/ADR-B3-201-empty-recording-scopes.md)<br>`research/phase-b/ADR-B3-201-empty-recording-scopes.md` | Cross-project or reference |
| [ADR B3-202: retain browser recording identity across adapter loss](../research/phase-b/ADR-B3-202-browser-recording-index.md)<br>`research/phase-b/ADR-B3-202-browser-recording-index.md` | Live sources and capture |
| [ADR B3-203: original browser recording-stage recovery](../research/phase-b/ADR-B3-203-browser-recording-candidate-recovery.md)<br>`research/phase-b/ADR-B3-203-browser-recording-candidate-recovery.md` | Live sources and capture |
| [ADR B3-204: preserve explicitly approved recording commit requests](../research/phase-b/ADR-B3-204-browser-recording-commit-recovery.md)<br>`research/phase-b/ADR-B3-204-browser-recording-commit-recovery.md` | Live sources and capture |
| [ADR B3-205: retire confirmed recording handoffs without deleting history](../research/phase-b/ADR-B3-205-browser-recording-candidate-retirement.md)<br>`research/phase-b/ADR-B3-205-browser-recording-candidate-retirement.md` | Live sources and capture |
| [ADR B3-206: retire confirmed gap handoffs for iterative browser authoring](../research/phase-b/ADR-B3-206-browser-gap-retirement.md)<br>`research/phase-b/ADR-B3-206-browser-gap-retirement.md` | Live sources and capture |
| [ADR B3-207: release stopped playback selections while retaining scope history](../research/phase-b/ADR-B3-207-browser-recording-index-retirement.md)<br>`research/phase-b/ADR-B3-207-browser-recording-index-retirement.md` | Live sources and capture |
| [ADR B3-208: explicit tab-capture input without source-player clock claims](../research/phase-b/ADR-B3-208-browser-tab-capture.md)<br>`research/phase-b/ADR-B3-208-browser-tab-capture.md` | Live sources and capture |
| [ADR B3-209: close inaccessible tab captures without stopping a newer source](../research/phase-b/ADR-B3-209-browser-tab-lifecycle.md)<br>`research/phase-b/ADR-B3-209-browser-tab-lifecycle.md` | Live sources and capture |
| [ADR B3-210: retain declared timestamp domain through live recording lineage](../research/phase-b/ADR-B3-210-live-time-basis.md)<br>`research/phase-b/ADR-B3-210-live-time-basis.md` | Live sources and capture |
| [ADR B3-211 — Desktop ownership of the shared browser player](../research/phase-b/ADR-B3-211-desktop-browser-launch.md)<br>`research/phase-b/ADR-B3-211-desktop-browser-launch.md` | Live sources and capture |
| [ADR B3-212 — Checked desktop browser configuration from Generator](../research/phase-b/ADR-B3-212-desktop-live-options.md)<br>`research/phase-b/ADR-B3-212-desktop-live-options.md` | Live sources and capture |
| [ADR B3-213 — Fresh pack-bound live capture](../research/phase-b/ADR-B3-213-live-model-packs.md)<br>`research/phase-b/ADR-B3-213-live-model-packs.md` | Live sources and capture |
| [ADR B3-214 — Browser camera presentation provenance](../research/phase-b/ADR-B3-214-browser-camera.md)<br>`research/phase-b/ADR-B3-214-browser-camera.md` | Live sources and capture |
| [ADR B4-001: declared selected-eye analysis views](../research/phase-b/ADR-B4-001-declared-selected-eye-views.md)<br>`research/phase-b/ADR-B4-001-declared-selected-eye-views.md` | Multimodal inputs |
| [ADR B4-002: Explicit preview request time basis](../research/phase-b/ADR-B4-002-explicit-preview-time-basis.md)<br>`research/phase-b/ADR-B4-002-explicit-preview-time-basis.md` | Multimodal inputs |
| [ADR B4-003: Engine-owned immutable source placements](../research/phase-b/ADR-B4-003-engine-owned-source-placements.md)<br>`research/phase-b/ADR-B4-003-engine-owned-source-placements.md` | Multimodal inputs |
| [ADR B4-004: Versioned portable placement closure](../research/phase-b/ADR-B4-004-portable-placement-closure.md)<br>`research/phase-b/ADR-B4-004-portable-placement-closure.md` | Projects and interoperability |
| [ADR B4-005: Exact placed-source generation sampling](../research/phase-b/ADR-B4-005-placed-generation-sampling.md)<br>`research/phase-b/ADR-B4-005-placed-generation-sampling.md` | Models and generation |
| [ADR B4-006: Explicit source-clock artifact mapping](../research/phase-b/ADR-B4-006-source-clock-artifact-mapping.md)<br>`research/phase-b/ADR-B4-006-source-clock-artifact-mapping.md` | Multimodal inputs |
| [ADR B4-007: Explicit motion retiming uses retained placement clocks](../research/phase-b/ADR-B4-007-explicit-existing-motion-retiming.md)<br>`research/phase-b/ADR-B4-007-explicit-existing-motion-retiming.md` | Models and generation |
| [ADR B4-007: independently captured audio with explicit unit-rate clocks](../research/phase-b/ADR-B4-007-independent-audio-clock-alignment.md)<br>`research/phase-b/ADR-B4-007-independent-audio-clock-alignment.md` | Multimodal inputs |
| [ADR B4-008: Retiming intent and result commit as structured facts](../research/phase-b/ADR-B4-008-transactional-retiming-receipts.md)<br>`research/phase-b/ADR-B4-008-transactional-retiming-receipts.md` | Cross-project or reference |
| [ADR B4-009: Preserve exact source clocks through verified retiming chains](../research/phase-b/ADR-B4-009-retimed-source-clock-proof.md)<br>`research/phase-b/ADR-B4-009-retimed-source-clock-proof.md` | Multimodal inputs |
| [ADR B4-010: Retain original retiming facts through portable clone ancestry](../research/phase-b/ADR-B4-010-portable-retiming-facts.md)<br>`research/phase-b/ADR-B4-010-portable-retiming-facts.md` | Projects and interoperability |
| [ADR B4-011: Inspect clone retiming through a separate historical contract](../research/phase-b/ADR-B4-011-archival-retiming-correspondence.md)<br>`research/phase-b/ADR-B4-011-archival-retiming-correspondence.md` | Architecture and assurance |
| [B4-012: original-clock container audio and visual precedence](../research/phase-b/ADR-B4-012-container-audio-visual-precedence.md)<br>`research/phase-b/ADR-B4-012-container-audio-visual-precedence.md` | Multimodal inputs |
| [ADR B5-001: timing-preserving device-motion proposals](../research/phase-b/ADR-B5-001-timing-preserving-device-proposals.md)<br>`research/phase-b/ADR-B5-001-timing-preserving-device-proposals.md` | Device playback |
| [ADR B5-002: asynchronous Handy playback effects](../research/phase-b/ADR-B5-002-asynchronous-handy-playback-effects.md)<br>`research/phase-b/ADR-B5-002-asynchronous-handy-playback-effects.md` | Device playback |
| [ADR B5-003: explicit live presentation correspondence](../research/phase-b/ADR-B5-003-declared-live-presentation-clocks.md)<br>`research/phase-b/ADR-B5-003-declared-live-presentation-clocks.md` | Live sources and capture |
| [ADR B5-004: bounded captured-pixel ownership](../research/phase-b/ADR-B5-004-bounded-captured-pixel-ownership.md)<br>`research/phase-b/ADR-B5-004-bounded-captured-pixel-ownership.md` | Live sources and capture |
| [ADR B5-005: authenticated capture lease lifecycle](../research/phase-b/ADR-B5-005-authenticated-capture-lease-lifecycle.md)<br>`research/phase-b/ADR-B5-005-authenticated-capture-lease-lifecycle.md` | Live sources and capture |
| [ADR B5-006: authenticated bounded RGB ingress](../research/phase-b/ADR-B5-006-authenticated-live-frame-ingress.md)<br>`research/phase-b/ADR-B5-006-authenticated-live-frame-ingress.md` | Live sources and capture |
| [ADR B5-007: persistent confined temporal worker](../research/phase-b/ADR-B5-007-persistent-confined-temporal-worker.md)<br>`research/phase-b/ADR-B5-007-persistent-confined-temporal-worker.md` | Cross-project or reference |
| [ADR B5-008: authenticated lease-owned inference consumption](../research/phase-b/ADR-B5-008-authenticated-live-inference-consumption.md)<br>`research/phase-b/ADR-B5-008-authenticated-live-inference-consumption.md` | Live sources and capture |
| [ADR B5-009: Conservative local capture-clock probes](../research/phase-b/ADR-B5-009-conservative-local-capture-clock-probes.md)<br>`research/phase-b/ADR-B5-009-conservative-local-capture-clock-probes.md` | Live sources and capture |
| [ADR B5-010: Timestamp-bound native stream decoding](../research/phase-b/ADR-B5-010-timestamp-bound-native-stream-decoding.md)<br>`research/phase-b/ADR-B5-010-timestamp-bound-native-stream-decoding.md` | Cross-project or reference |
| [ADR B5-011: Bounded native encoded-source role](../research/phase-b/ADR-B5-011-bounded-native-encoded-source-role.md)<br>`research/phase-b/ADR-B5-011-bounded-native-encoded-source-role.md` | Multimodal inputs |
| [ADR B5-012: Parent-owned native source launch](../research/phase-b/ADR-B5-012-parent-owned-native-source-launch.md)<br>`research/phase-b/ADR-B5-012-parent-owned-native-source-launch.md` | Multimodal inputs |
| [ADR B5-013: Native delivery clock and absolute lease boundary](../research/phase-b/ADR-B5-013-native-delivery-clock-and-absolute-lease-boundary.md)<br>`research/phase-b/ADR-B5-013-native-delivery-clock-and-absolute-lease-boundary.md` | Live sources and capture |
| [ADR B5-014: Authenticated encoded capture publication](../research/phase-b/ADR-B5-014-authenticated-encoded-capture-publication.md)<br>`research/phase-b/ADR-B5-014-authenticated-encoded-capture-publication.md` | Live sources and capture |
| [ADR B5-015: explicit source-preserving capture epoch continuation](../research/phase-b/ADR-B5-015-explicit-live-epoch-continuation.md)<br>`research/phase-b/ADR-B5-015-explicit-live-epoch-continuation.md` | Live sources and capture |
| [ADR B5-016: preserve exact live project phase through epoch handoff](../research/phase-b/ADR-B5-016-exact-live-project-phase.md)<br>`research/phase-b/ADR-B5-016-exact-live-project-phase.md` | Live sources and capture |
| [ADR B5-017: bounded first-party piped encoded ingress](../research/phase-b/ADR-B5-017-bounded-piped-encoded-stream-ingress.md)<br>`research/phase-b/ADR-B5-017-bounded-piped-encoded-stream-ingress.md` | Cross-project or reference |
| [ADR B5-018: Durable stop handoff and exact callback correspondence](../research/phase-b/ADR-B5-018-durable-stop-handoff-and-correspondence.md)<br>`research/phase-b/ADR-B5-018-durable-stop-handoff-and-correspondence.md` | Architecture and assurance |
| [ADR B5-019: Retry observed stop metadata without replaying a device effect](../research/phase-b/ADR-B5-019-observed-stop-metadata-reconciliation.md)<br>`research/phase-b/ADR-B5-019-observed-stop-metadata-reconciliation.md` | Cross-project or reference |
| [ADR B5-020 — Preserve play observations independently of stop and journal failures](../research/phase-b/ADR-B5-020-observed-play-completion.md)<br>`research/phase-b/ADR-B5-020-observed-play-completion.md` | Cross-project or reference |
| [ADR B5-021 — Own the HTTP job and consume one dispatch claim](../research/phase-b/ADR-B5-021-owned-hssp-dispatch.md)<br>`research/phase-b/ADR-B5-021-owned-hssp-dispatch.md` | Device playback |
| [ADR B5-022 — Persist credential references without restoring device authority](../research/phase-b/ADR-B5-022-inert-handy-registrations.md)<br>`research/phase-b/ADR-B5-022-inert-handy-registrations.md` | Device playback |
| [ADR B5-023 — Resolve original registration identities before clearing uncertainty](../research/phase-b/ADR-B5-023-desktop-handy-intent-resolution.md)<br>`research/phase-b/ADR-B5-023-desktop-handy-intent-resolution.md` | Device playback |
| [ADR B5-024 — Bind connection observations to one Engine boot and credential snapshot](../research/phase-b/ADR-B5-024-boot-bound-handy-connections.md)<br>`research/phase-b/ADR-B5-024-boot-bound-handy-connections.md` | Device playback |
| [ADR B5-025 — Bind local script bytes before hosting consent](../research/phase-b/ADR-B5-025-exact-stroke-script-preparation.md)<br>`research/phase-b/ADR-B5-025-exact-stroke-script-preparation.md` | Projects and interoperability |
| [ADR B5-026 — Preserve proposal scope in portable export history](../research/phase-b/ADR-B5-026-device-export-archival-scope.md)<br>`research/phase-b/ADR-B5-026-device-export-archival-scope.md` | Device playback |
| [ADR B5-027 — Journal exact disclosure before temporary hosting](../research/phase-b/ADR-B5-027-explicit-handy-hosting.md)<br>`research/phase-b/ADR-B5-027-explicit-handy-hosting.md` | Device playback |
| [ADR B5-028 — Recheck both native references before device setup](../research/phase-b/ADR-B5-028-fresh-handy-setup-observations.md)<br>`research/phase-b/ADR-B5-028-fresh-handy-setup-observations.md` | Device playback |
| [ADR B5-029 — Separate retained evidence, operator decisions and live admission](../research/phase-b/ADR-B5-029-bound-handy-qualification-review.md)<br>`research/phase-b/ADR-B5-029-bound-handy-qualification-review.md` | Device playback |
| [ADR B5-030 — Bind explicit operative settings without upgrading old evidence](../research/phase-b/ADR-B5-030-operative-settings-qualification-subject.md)<br>`research/phase-b/ADR-B5-030-operative-settings-qualification-subject.md` | Cross-project or reference |
| [ADR B5-031 — Claim setup durably before transport handoff](../research/phase-b/ADR-B5-031-single-use-setup-journal.md)<br>`research/phase-b/ADR-B5-031-single-use-setup-journal.md` | Projects and interoperability |
| [ADR B5-032 — Private route namespaces and actor-scoped setup recovery](../research/phase-b/ADR-B5-032-private-routes-and-setup-recovery.md)<br>`research/phase-b/ADR-B5-032-private-routes-and-setup-recovery.md` | Cross-project or reference |
| [ADR B5-033 — Explicit qualified setup with durable request and journal links](../research/phase-b/ADR-B5-033-explicit-qualified-setup-handoff.md)<br>`research/phase-b/ADR-B5-033-explicit-qualified-setup-handoff.md` | Cross-project or reference |
| [ADR B5-034 — Exact unclaimed setup cancellation with recoverable receipts](../research/phase-b/ADR-B5-034-exact-unclaimed-setup-cancellation.md)<br>`research/phase-b/ADR-B5-034-exact-unclaimed-setup-cancellation.md` | Cross-project or reference |
| [ADR B5-035 — Retain setup responses atomically without restoring playback authority](../research/phase-b/ADR-B5-035-retained-setup-observations.md)<br>`research/phase-b/ADR-B5-035-retained-setup-observations.md` | Cross-project or reference |
| [ADR B5-036 — Correlate fresh streams without assuming whole-script buffer residency](../research/phase-b/ADR-B5-036-fresh-stream-correlation.md)<br>`research/phase-b/ADR-B5-036-fresh-stream-correlation.md` | Cross-project or reference |
| [ADR B5-037 — Retain nonplaying native state without inventing controller liveness](../research/phase-b/ADR-B5-037-boot-local-nonplaying-preparation.md)<br>`research/phase-b/ADR-B5-037-boot-local-nonplaying-preparation.md` | Cross-project or reference |
| [ADR B5-038 — Distinct renewable controller lifetime with owned expiry](../research/phase-b/ADR-B5-038-renewable-local-controller-leases.md)<br>`research/phase-b/ADR-B5-038-renewable-local-controller-leases.md` | Cross-project or reference |
| [ADR B5-039 — Explicit server-clock bounds and native custody](../research/phase-b/ADR-B5-039-native-server-clock-contract-and-custody.md)<br>`research/phase-b/ADR-B5-039-native-server-clock-contract-and-custody.md` | Architecture and assurance |
| [ADR B5-040 — Observe device timing without inferring synchronization](../research/phase-b/ADR-B5-040-device-timing-observations-without-inferred-synchronization.md)<br>`research/phase-b/ADR-B5-040-device-timing-observations-without-inferred-synchronization.md` | Device playback |
| [ADR B5-041 — Independent synchronization custody and single-use handoff](../research/phase-b/ADR-B5-041-independent-clock-synchronization-custody.md)<br>`research/phase-b/ADR-B5-041-independent-clock-synchronization-custody.md` | Cross-project or reference |
| [ADR B5-042 — Native synchronization lifetime and combined source expiry](../research/phase-b/ADR-B5-042-native-device-timing-and-combined-expiry.md)<br>`research/phase-b/ADR-B5-042-native-device-timing-and-combined-expiry.md` | Device playback |
| [ADR B5-043 — Independent stop custody and guarded dispatch claims](../research/phase-b/ADR-B5-043-independent-stop-owner-and-guarded-claims.md)<br>`research/phase-b/ADR-B5-043-independent-stop-owner-and-guarded-claims.md` | Cross-project or reference |
| [ADR B5-044 — Original play operations and durable claims](../research/phase-b/ADR-B5-044-original-play-operation-journal.md)<br>`research/phase-b/ADR-B5-044-original-play-operation-journal.md` | Projects and interoperability |
| [ADR B5-045 — Guarded native play submission and shared execution ownership](../research/phase-b/ADR-B5-045-guarded-native-play-submission.md)<br>`research/phase-b/ADR-B5-045-guarded-native-play-submission.md` | Cross-project or reference |
| [ADR B5-046 — Owned daemon listeners and explicit Engine retirement](../research/phase-b/ADR-B5-046-owned-daemon-shutdown.md)<br>`research/phase-b/ADR-B5-046-owned-daemon-shutdown.md` | Cross-project or reference |
| [ADR B5-047 — Emergency stop without a confirmed prewrite](../research/phase-b/ADR-B5-047-emergency-stop-without-confirmed-prewrite.md)<br>`research/phase-b/ADR-B5-047-emergency-stop-without-confirmed-prewrite.md` | Cross-project or reference |
| [ADR B5-048 — Qualified custody reconciliation](../research/phase-b/ADR-B5-048-qualified-custody-reconciliation.md)<br>`research/phase-b/ADR-B5-048-qualified-custody-reconciliation.md` | Cross-project or reference |
| [ADR B5-049 — Continuous timing for an existing Playing stream](../research/phase-b/ADR-B5-049-continuous-playing-timing.md)<br>`research/phase-b/ADR-B5-049-continuous-playing-timing.md` | Cross-project or reference |
| [ADR B5-050 — Independent browser control and native player stop guards](../research/phase-b/ADR-B5-050-browser-player-stop-guard.md)<br>`research/phase-b/ADR-B5-050-browser-player-stop-guard.md` | Live sources and capture |
| [ADR B5-051 — Imported browser media with declared source correspondence](../research/phase-b/ADR-B5-051-browser-source-media-binding.md)<br>`research/phase-b/ADR-B5-051-browser-source-media-binding.md` | Live sources and capture |
| [ADR B5-052 — Browser frame-clock observations before playback](../research/phase-b/ADR-B5-052-browser-frame-clock-observations.md)<br>`research/phase-b/ADR-B5-052-browser-frame-clock-observations.md` | Live sources and capture |
| [Preserve a measured browser start as one phase/time reference](../research/phase-b/ADR-B5-053-browser-observed-handy-start.md)<br>`research/phase-b/ADR-B5-053-browser-observed-handy-start.md` | Device playback |
| [Keep generated point streams separate from completed scripts](../research/phase-b/ADR-B5-054-bounded-generated-handy-streams.md)<br>`research/phase-b/ADR-B5-054-bounded-generated-handy-streams.md` | Device playback |
| [Claim exact HSP commands durably before native dispatch](../research/phase-b/ADR-B5-055-durable-hsp-command-custody.md)<br>`research/phase-b/ADR-B5-055-durable-hsp-command-custody.md` | Device playback |
| [Share native HSP Stop custody across durable and emergency dispatch](../research/phase-b/ADR-B5-056-native-hsp-stop-reservation.md)<br>`research/phase-b/ADR-B5-056-native-hsp-stop-reservation.md` | Device playback |
| [Own the HSP session through generation retirement and shutdown](../research/phase-b/ADR-B5-057-engine-owned-hsp-session.md)<br>`research/phase-b/ADR-B5-057-engine-owned-hsp-session.md` | Device playback |
| [Keep HSP qualification and recovery distinct from restored authority](../research/phase-b/ADR-B5-058-hsp-qualification-and-custody-recovery.md)<br>`research/phase-b/ADR-B5-058-hsp-qualification-and-custody-recovery.md` | Device playback |
| [Preserve original capture time across native clock domains](../research/phase-b/ADR-B5-059-native-capture-clock-correspondence.md)<br>`research/phase-b/ADR-B5-059-native-capture-clock-correspondence.md` | Live sources and capture |
| [Own original HSP synchronization before consuming native readiness](../research/phase-b/ADR-B5-060-hsp-preparation.md)<br>`research/phase-b/ADR-B5-060-hsp-preparation.md` | Device playback |
| [Bind one native analysis feed to independently observed visible HSP phase](../research/phase-b/ADR-B5-061-browser-hsp-live-source.md)<br>`research/phase-b/ADR-B5-061-browser-hsp-live-source.md` | Device playback |
| [Close exact input before draining an acknowledged finite tail](../research/phase-b/ADR-B5-062-finite-input-and-hsp-tail-drain.md)<br>`research/phase-b/ADR-B5-062-finite-input-and-hsp-tail-drain.md` | Device playback |
| [Retain original unclaimed command custody across bounded contention](../research/phase-b/ADR-B5-063-original-preclaim-custody-under-contention.md)<br>`research/phase-b/ADR-B5-063-original-preclaim-custody-under-contention.md` | Cross-project or reference |
| [Preserve stream origin while transferring source and clock ownership](../research/phase-b/ADR-B5-064-continuous-source-and-clock-ownership.md)<br>`research/phase-b/ADR-B5-064-continuous-source-and-clock-ownership.md` | Multimodal inputs |
| [Keep fresh timing reads independent of publication locks](../research/phase-b/ADR-B5-065-native-timing-publication.md)<br>`research/phase-b/ADR-B5-065-native-timing-publication.md` | Cross-project or reference |
| [Keep browser HSP controls explicit and natively owned](../research/phase-b/ADR-B5-066-browser-hsp-controls.md)<br>`research/phase-b/ADR-B5-066-browser-hsp-controls.md` | Device playback |
| [Select original observations before sampling a fresh evaluation time](../research/phase-b/ADR-B5-067-selection-before-clock-sampling.md)<br>`research/phase-b/ADR-B5-067-selection-before-clock-sampling.md` | Cross-project or reference |
| [Retain certified passage without rewriting fresh phase evidence](../research/phase-b/ADR-B5-068-certified-passage-and-phase-uncertainty.md)<br>`research/phase-b/ADR-B5-068-certified-passage-and-phase-uncertainty.md` | Cross-project or reference |
| [B6-001: revision-bound review decisions](../research/phase-b/ADR-B6-001-revision-bound-review-decisions.md)<br>`research/phase-b/ADR-B6-001-revision-bound-review-decisions.md` | Projects and interoperability |
| [ADR B6-002: Offline directory packs install as inert snapshots](../research/phase-b/ADR-B6-002-inert-offline-model-packs.md)<br>`research/phase-b/ADR-B6-002-inert-offline-model-packs.md` | Models and generation |
| [ADR B6-003: inspect ONNX dependencies and tensor declarations before inert pack publication](../research/phase-b/ADR-B6-003-inert-pack-onnx-admission.md)<br>`research/phase-b/ADR-B6-003-inert-pack-onnx-admission.md` | Models and generation |
| [ADR B6-004: persist pack staging identity and observe descriptor ownership](../research/phase-b/ADR-B6-004-persistent-pack-staging-ownership.md)<br>`research/phase-b/ADR-B6-004-persistent-pack-staging-ownership.md` | Live sources and capture |
| [ADR B6-005: explicitly discard one recognized abandoned pack stage](../research/phase-b/ADR-B6-005-explicit-abandoned-stage-discard.md)<br>`research/phase-b/ADR-B6-005-explicit-abandoned-stage-discard.md` | Cross-project or reference |
| [ADR B6-006: Transactional protection of native authored motion](../research/phase-b/ADR-B6-006-automatic-authored-motion-protection.md)<br>`research/phase-b/ADR-B6-006-automatic-authored-motion-protection.md` | Models and generation |
| [ADR B6-007: Owner-selected packs use the pinned CPU generation boundary](../research/phase-b/ADR-B6-007-owner-selected-pack-generation.md)<br>`research/phase-b/ADR-B6-007-owner-selected-pack-generation.md` | Models and generation |
| [ADR B6-008: Desktop pack receipts do not become current selection](../research/phase-b/ADR-B6-008-desktop-owner-pack-controls.md)<br>`research/phase-b/ADR-B6-008-desktop-owner-pack-controls.md` | Models and generation |
| [ADR B6-009: Selection-bound detector preview shares finite worker admission](../research/phase-b/ADR-B6-009-selection-bound-detector-preview.md)<br>`research/phase-b/ADR-B6-009-selection-bound-detector-preview.md` | Multimodal inputs |
| [ADR B6-010: Stash batch pack intent remains revision-bound](../research/phase-b/ADR-B6-010-selection-bound-stash-batches.md)<br>`research/phase-b/ADR-B6-010-selection-bound-stash-batches.md` | Library and Stash |
| [ADR B6-011: Desktop recovery intent precedes pack dispatch](../research/phase-b/ADR-B6-011-session-bound-desktop-pack-recovery.md)<br>`research/phase-b/ADR-B6-011-session-bound-desktop-pack-recovery.md` | Models and generation |
| [ADR B6-012: Verify catalog metadata before publishing trust state](../research/phase-b/ADR-B6-012-bounded-offline-catalog-verifier.md)<br>`research/phase-b/ADR-B6-012-bounded-offline-catalog-verifier.md` | Distribution and resources |
| [ADR B6-013: Publish catalog watermarks and replay facts atomically](../research/phase-b/ADR-B6-013-owner-authorized-atomic-catalog-intake.md)<br>`research/phase-b/ADR-B6-013-owner-authorized-atomic-catalog-intake.md` | Architecture and assurance |
| [ADR B6-014: Observe installed association without promoting historical trust](../research/phase-b/ADR-B6-014-installed-offline-catalog-association.md)<br>`research/phase-b/ADR-B6-014-installed-offline-catalog-association.md` | Distribution and resources |
| [ADR B6-015: Fence future owner-approved generation by exact catalog facts](../research/phase-b/ADR-B6-015-exact-catalog-bound-owner-approval.md)<br>`research/phase-b/ADR-B6-015-exact-catalog-bound-owner-approval.md` | Cross-project or reference |
| [ADR B6-016: Keep desktop catalog history separate from current approval](../research/phase-b/ADR-B6-016-desktop-catalog-intent-and-history.md)<br>`research/phase-b/ADR-B6-016-desktop-catalog-intent-and-history.md` | Cross-project or reference |
| [ADR B6-017: Open graphical recovery after engine startup refusal](../research/phase-b/ADR-B6-017-desktop-startup-recovery.md)<br>`research/phase-b/ADR-B6-017-desktop-startup-recovery.md` | Cross-project or reference |
| [ADR B6-018: transaction-scoped private recovery lock release](../research/phase-b/ADR-B6-018-transaction-scoped-recovery-lock-release.md)<br>`research/phase-b/ADR-B6-018-transaction-scoped-recovery-lock-release.md` | Distribution and resources |
| [B1 measurement groundwork result](../research/phase-b/B1-RESULTS.md)<br>`research/phase-b/B1-RESULTS.md` | Cross-project or reference |
| [B1 successor: worker regression evidence and open readiness](../research/phase-b/B1-SUCCESSOR-2026-09-11.md)<br>`research/phase-b/B1-SUCCESSOR-2026-09-11.md` | Cross-project or reference |
| [B2-003: Live-development position signal baseline](../research/phase-b/B2-003-live-development-signal-protocol.md)<br>`research/phase-b/B2-003-live-development-signal-protocol.md` | Live sources and capture |
| [B2-004: Live-reference label and sampled-clock consistency](../research/phase-b/B2-004-live-label-consistency-protocol.md)<br>`research/phase-b/B2-004-live-label-consistency-protocol.md` | Live sources and capture |
| [B2-005: Existing training pixel and source-clock correspondence](../research/phase-b/B2-005-training-pixel-correspondence-protocol.md)<br>`research/phase-b/B2-005-training-pixel-correspondence-protocol.md` | Architecture and assurance |
| [Bundled offline local assistant: runtime research](../research/phase-b/PULSAR_LOCAL_ASSISTANT_RUNTIME_RESEARCH_2026-10-10.md)<br>`research/phase-b/PULSAR_LOCAL_ASSISTANT_RUNTIME_RESEARCH_2026-10-10.md` | Models and generation |
| [Phase B research and benchmark baseline](../research/phase-b/README.md)<br>`research/phase-b/README.md` | Cross-project or reference |
| [YOLO11n candidate sources — 2026-10-11](../research/phase-b/YOLO11_CANDIDATE_SOURCES_2026-10-11.md)<br>`research/phase-b/YOLO11_CANDIDATE_SOURCES_2026-10-11.md` | Multimodal inputs |
| [Browser player timing and liveness for Pulsar](../research/phase-b/browser-player-timing-and-liveness-2026-10-22.md)<br>`research/phase-b/browser-player-timing-and-liveness-2026-10-22.md` | Live sources and capture |
| [CUDA adapter feasibility and observed dependencies](../research/phase-b/cuda-adapter-probe-2026-10-04.md)<br>`research/phase-b/cuda-adapter-probe-2026-10-04.md` | Models and generation |
| [Handy 2 Pro: continuous timing and generated motion](../research/phase-b/handy-continuous-timing-and-streaming-2026-10-22.md)<br>`research/phase-b/handy-continuous-timing-and-streaming-2026-10-22.md` | Device playback |
| [Handy HSP streaming implementation contract](../research/phase-b/handy-hsp-stream-contract-2026-10-23.md)<br>`research/phase-b/handy-hsp-stream-contract-2026-10-23.md` | Device playback |
| [Handy HSP sustained timing and finite-tail drain](../research/phase-b/handy-hsp-sustained-timing-and-drain-2026-10-23.md)<br>`research/phase-b/handy-hsp-sustained-timing-and-drain-2026-10-23.md` | Device playback |
| [Handy integration: protocol evidence and qualification boundary](../research/phase-b/handy-integration-research-2026-10-01.md)<br>`research/phase-b/handy-integration-research-2026-10-01.md` | Device playback |
| [Handy 2 Pro / FW4: public RPC contract refresh](../research/phase-b/handy2-pro-fw4-contract-refresh-2026-10-09.md)<br>`research/phase-b/handy2-pro-fw4-contract-refresh-2026-10-09.md` | Device playback |
| [Offline model/runtime packs: contracts and release evidence](../research/phase-b/model-pack-integration-research-2026-10-01.md)<br>`research/phase-b/model-pack-integration-research-2026-10-01.md` | Models and generation |
| [Good phase runtime portability research](../research/phase-b/runtime-portability-research-2026-09-12.md)<br>`research/phase-b/runtime-portability-research-2026-09-12.md` | Models and generation |
| [Local semantic image runtime for constrained Pulsar proposals](../research/phase-b/semantic-image-runtime-research-2026-10-11.md)<br>`research/phase-b/semantic-image-runtime-research-2026-10-11.md` | Models and generation |
| [Stash integration: verified broker contract](../research/phase-b/stash-integration-research-2026-09-29.md)<br>`research/phase-b/stash-integration-research-2026-09-29.md` | Library and Stash |
| [Roleplay assistant model research: local, offline, tool-calling](../research/roleplay-models-2026-11-04.md)<br>`research/roleplay-models-2026-11-04.md` | Roleplay, consent and optional imagery |

## Historical plans and observations

| Document / original path | Feature family |
| --- | --- |
| [Pulsar Architecture-First Execution Plan](../docs/PULSAR_ARCHITECTURE_FIRST_EXECUTION_PLAN.md)<br>`docs/PULSAR_ARCHITECTURE_FIRST_EXECUTION_PLAN.md` | Architecture and assurance |
| [Inspect preserved retiming facts in a cloned project](../docs/PULSAR_ARCHIVAL_RETIMING_STATUS.md)<br>`docs/PULSAR_ARCHIVAL_RETIMING_STATUS.md` | Cross-project or reference |
| [Assistant memory handoff](../docs/PULSAR_ASSISTANT_MEMORY_HANDOFF.md)<br>`docs/PULSAR_ASSISTANT_MEMORY_HANDOFF.md` | Assistant and extensions |
| [Durable Handy stop handoff — 2026-10-10](../docs/PULSAR_DURABLE_STOP_HANDOFF.md)<br>`docs/PULSAR_DURABLE_STOP_HANDOFF.md` | Cross-project or reference |
| [Pulsar Good Phase Closure Matrix](../docs/PULSAR_GOOD_PHASE_CLOSURE_MATRIX.md)<br>`docs/PULSAR_GOOD_PHASE_CLOSURE_MATRIX.md` | Cross-project or reference |
| [Inspect inference attempt history](../docs/PULSAR_INFERENCE_ATTEMPT_HISTORY.md)<br>`docs/PULSAR_INFERENCE_ATTEMPT_HISTORY.md` | Models and generation |
| [Pulsar items 2-3: implementation handoff](../docs/PULSAR_ITEMS_2_3_HANDOFF_2026-09-28.md)<br>`docs/PULSAR_ITEMS_2_3_HANDOFF_2026-09-28.md` | Cross-project or reference |
| [Large Motion Artifact Transport: Next Good-Phase Work Package](../docs/PULSAR_LARGE_PROGRAM_IMPLEMENTATION_PLAN.md)<br>`docs/PULSAR_LARGE_PROGRAM_IMPLEMENTATION_PLAN.md` | Cross-project or reference |
| [Live CUDA preparation resource handoff](../docs/PULSAR_LIVE_CUDA_PREPARATION_HANDOFF.md)<br>`docs/PULSAR_LIVE_CUDA_PREPARATION_HANDOFF.md` | Live sources and capture |
| [Pulsar Major Roadmap](../docs/PULSAR_MAJOR_ROADMAP.md)<br>`docs/PULSAR_MAJOR_ROADMAP.md` | Cross-project or reference |
| [Pulsar milestone gate ledger](../docs/PULSAR_MILESTONE_HISTORY_BEFORE_DASHBOARD.md)<br>`docs/PULSAR_MILESTONE_HISTORY_BEFORE_DASHBOARD.md` | Cross-project or reference |
| [Native CPU assistant handoff](../docs/PULSAR_NATIVE_ASSISTANT_HANDOFF.md)<br>`docs/PULSAR_NATIVE_ASSISTANT_HANDOFF.md` | Assistant and extensions |
| [Parallel implementation handoff: CPU-ROUNDTRIP](../docs/PULSAR_PARALLEL_CPU_BUILD_HANDOFF.md)<br>`docs/PULSAR_PARALLEL_CPU_BUILD_HANDOFF.md` | Models and generation |
| [Pulsar Architecture Phase A1-A6 completion record](../docs/PULSAR_PHASE_A_STATUS.md)<br>`docs/PULSAR_PHASE_A_STATUS.md` | Cross-project or reference |
| [Pulsar Good Phase: large-motion and startup checkpoint](../docs/PULSAR_PHASE_B_LARGE_MOTION_STATUS.md)<br>`docs/PULSAR_PHASE_B_LARGE_MOTION_STATUS.md` | Models and generation |
| [Pulsar Phase B Research Status](../docs/PULSAR_PHASE_B_RESEARCH_STATUS.md)<br>`docs/PULSAR_PHASE_B_RESEARCH_STATUS.md` | Cross-project or reference |
| [Pulsar Good Phase: implementation checkpoint](../docs/PULSAR_PHASE_B_STATUS.md)<br>`docs/PULSAR_PHASE_B_STATUS.md` | Cross-project or reference |
| [Portable accepted CUDA evidence implementation](../docs/PULSAR_PORTABLE_CUDA_IMPLEMENTATION_PLAN.md)<br>`docs/PULSAR_PORTABLE_CUDA_IMPLEMENTATION_PLAN.md` | Projects and interoperability |
| [Portable fallback history](../docs/PULSAR_PORTABLE_FALLBACK_HISTORY.md)<br>`docs/PULSAR_PORTABLE_FALLBACK_HISTORY.md` | Projects and interoperability |
| [Portable-project implementation plan](../docs/PULSAR_PORTABLE_PROJECT_IMPLEMENTATION_PLAN.md)<br>`docs/PULSAR_PORTABLE_PROJECT_IMPLEMENTATION_PLAN.md` | Projects and interoperability |
| [Portable project P1a: contracts, capture, and codec](../docs/PULSAR_PORTABLE_PROJECT_P1A_STATUS.md)<br>`docs/PULSAR_PORTABLE_PROJECT_P1A_STATUS.md` | Projects and interoperability |
| [Portable project P1b: retained export and scoped download](../docs/PULSAR_PORTABLE_PROJECT_P1B_STATUS.md)<br>`docs/PULSAR_PORTABLE_PROJECT_P1B_STATUS.md` | Projects and interoperability |
| [Portable project P1c: clone import status](../docs/PULSAR_PORTABLE_PROJECT_P1C_STATUS.md)<br>`docs/PULSAR_PORTABLE_PROJECT_P1C_STATUS.md` | Projects and interoperability |
| [Portable project P1d: seven-repair checkpoint](../docs/PULSAR_PORTABLE_PROJECT_P1D_REPAIR_STATUS.md)<br>`docs/PULSAR_PORTABLE_PROJECT_P1D_REPAIR_STATUS.md` | Projects and interoperability |
| [Portable project P1d: first implementation, qualification blocked](../docs/PULSAR_PORTABLE_PROJECT_P1D_STATUS.md)<br>`docs/PULSAR_PORTABLE_PROJECT_P1D_STATUS.md` | Projects and interoperability |
| [Pulsar roadmap execution checkpoint](../docs/PULSAR_ROADMAP_EXECUTION_2026-10-01.md)<br>`docs/PULSAR_ROADMAP_EXECUTION_2026-10-01.md` | Cross-project or reference |
| [Shared native image handoff checkpoint](../docs/PULSAR_SHARED_NATIVE_IMAGE_HANDOFF.md)<br>`docs/PULSAR_SHARED_NATIVE_IMAGE_HANDOFF.md` | Cross-project or reference |
| [Pulsar Software Specification](../docs/PULSAR_SOFTWARE_SPECIFICATION_HISTORY_2026-11-06.md)<br>`docs/PULSAR_SOFTWARE_SPECIFICATION_HISTORY_2026-11-06.md` | Cross-project or reference |
| [Pulsar current completion status](../docs/PULSAR_STATUS_HISTORY_BEFORE_DASHBOARD.md)<br>`docs/PULSAR_STATUS_HISTORY_BEFORE_DASHBOARD.md` | Cross-project or reference |
| [Retained WASM invocation history](../docs/PULSAR_WASM_ARCHIVE_HISTORY.md)<br>`docs/PULSAR_WASM_ARCHIVE_HISTORY.md` | Assistant and extensions |
| [Preserved implementation reference](../legacy/attended-bench/README.md)<br>`legacy/attended-bench/README.md` | Cross-project or reference |
| [Preserved Handy bench tools](../legacy/bench-tools/README.md)<br>`legacy/bench-tools/README.md` | Cross-project or reference |
| [Preserved implementation reference](../legacy/handy-hsp/README.md)<br>`legacy/handy-hsp/README.md` | Device playback |
| [Preserved implementation reference](../legacy/macos-socket-probe/README.md)<br>`legacy/macos-socket-probe/README.md` | Cross-project or reference |
| [Pre-engine source reference](../legacy/pre-engine/README.md)<br>`legacy/pre-engine/README.md` | Cross-project or reference |
| [.gitignore](../research/phase-b/historical-execution/.gitignore)<br>`research/phase-b/historical-execution/.gitignore` | Cross-project or reference |
| [README.md](../research/phase-b/historical-execution/README.md)<br>`research/phase-b/historical-execution/README.md` | Cross-project or reference |
| [Exact offline build blocker](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/BLOCKER.md)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/BLOCKER.md` | Cross-project or reference |
| [Historical twenty-regression execution receipt](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/REPORT.md)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/REPORT.md` | Cross-project or reference |
| [cargo.stderr.log](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/cargo.stderr.log)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/cargo.stderr.log` | Cross-project or reference |
| [cargo.stdout.log](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/cargo.stdout.log)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/cargo.stdout.log` | Cross-project or reference |
| [execution-Cargo.toml](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/execution-Cargo.toml)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/execution-Cargo.toml` | Cross-project or reference |
| [original-Cargo.toml](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/original-Cargo.toml)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/original-Cargo.toml` | Cross-project or reference |
| [original-test-modules.json](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/original-test-modules.json)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/original-test-modules.json` | Cross-project or reference |
| [results.json](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/results.json)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/results.json` | Cross-project or reference |
| [subject.json](../research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/subject.json)<br>`research/phase-b/historical-execution/run-c7ee1411-0cc9-4f83-827d-8e48916fb5b5/subject.json` | Cross-project or reference |

## Supporting assets, models, schemas and receipts

| Document / original path | Feature family |
| --- | --- |
| [PulsarLifecycle.cfg](../assurance/PulsarLifecycle.cfg)<br>`assurance/PulsarLifecycle.cfg` | Architecture and assurance |
| [PulsarLifecycle.tla](../assurance/PulsarLifecycle.tla)<br>`assurance/PulsarLifecycle.tla` | Architecture and assurance |
| [PulsarPackageExport.cfg](../assurance/PulsarPackageExport.cfg)<br>`assurance/PulsarPackageExport.cfg` | Projects and interoperability |
| [PulsarPackageExport.tla](../assurance/PulsarPackageExport.tla)<br>`assurance/PulsarPackageExport.tla` | Projects and interoperability |
| [PulsarPackageImport.cfg](../assurance/PulsarPackageImport.cfg)<br>`assurance/PulsarPackageImport.cfg` | Projects and interoperability |
| [PulsarPackageImport.tla](../assurance/PulsarPackageImport.tla)<br>`assurance/PulsarPackageImport.tla` | Projects and interoperability |
| [PulsarTransfers.cfg](../assurance/PulsarTransfers.cfg)<br>`assurance/PulsarTransfers.cfg` | Architecture and assurance |
| [PulsarTransfers.tla](../assurance/PulsarTransfers.tla)<br>`assurance/PulsarTransfers.tla` | Architecture and assurance |
| [WasmStartResolution.cfg](../assurance/WasmStartResolution.cfg)<br>`assurance/WasmStartResolution.cfg` | Assistant and extensions |
| [WasmStartResolution.tla](../assurance/WasmStartResolution.tla)<br>`assurance/WasmStartResolution.tla` | Assistant and extensions |
| [check-lifecycle.sh](../assurance/check-lifecycle.sh)<br>`assurance/check-lifecycle.sh` | Architecture and assurance |
| [check-wasm-start-resolution.py](../assurance/check-wasm-start-resolution.py)<br>`assurance/check-wasm-start-resolution.py` | Assistant and extensions |
| [architecture-gates.log](../docs/evidence/phase-a/architecture-gates.log)<br>`docs/evidence/phase-a/architecture-gates.log` | Architecture and assurance |
| [environment.txt](../docs/evidence/phase-a/environment.txt)<br>`docs/evidence/phase-a/environment.txt` | Cross-project or reference |
| [gui-candidate.png](../docs/evidence/phase-a/gui-candidate.png)<br>`docs/evidence/phase-a/gui-candidate.png` | Cross-project or reference |
| [gui-durable-state.txt](../docs/evidence/phase-a/gui-durable-state.txt)<br>`docs/evidence/phase-a/gui-durable-state.txt` | Cross-project or reference |
| [gui-export-preview.png](../docs/evidence/phase-a/gui-export-preview.png)<br>`docs/evidence/phase-a/gui-export-preview.png` | Projects and interoperability |
| [gui-export.funscript](../docs/evidence/phase-a/gui-export.funscript)<br>`docs/evidence/phase-a/gui-export.funscript` | Projects and interoperability |
| [gui-fixture.mkv](../docs/evidence/phase-a/gui-fixture.mkv)<br>`docs/evidence/phase-a/gui-fixture.mkv` | Cross-project or reference |
| [gui-native-frame-0.png](../docs/evidence/phase-a/gui-native-frame-0.png)<br>`docs/evidence/phase-a/gui-native-frame-0.png` | Multimodal inputs |
| [gui-native-frame-6.png](../docs/evidence/phase-a/gui-native-frame-6.png)<br>`docs/evidence/phase-a/gui-native-frame-6.png` | Multimodal inputs |
| [gui-native-job.png](../docs/evidence/phase-a/gui-native-job.png)<br>`docs/evidence/phase-a/gui-native-job.png` | Cross-project or reference |
| [structure.json](../docs/evidence/phase-a/structure.json)<br>`docs/evidence/phase-a/structure.json` | Cross-project or reference |
| [final-export.png](../docs/evidence/phase-b-large-motion/final-export.png)<br>`docs/evidence/phase-b-large-motion/final-export.png` | Projects and interoperability |
| [final-gate-summary.json](../docs/evidence/phase-b-large-motion/final-gate-summary.json)<br>`docs/evidence/phase-b-large-motion/final-gate-summary.json` | Models and generation |
| [final-gate.log](../docs/evidence/phase-b-large-motion/final-gate.log)<br>`docs/evidence/phase-b-large-motion/final-gate.log` | Models and generation |
| [final-gui-parity.log](../docs/evidence/phase-b-large-motion/final-gui-parity.log)<br>`docs/evidence/phase-b-large-motion/final-gui-parity.log` | Models and generation |
| [final-gui.json](../docs/evidence/phase-b-large-motion/final-gui.json)<br>`docs/evidence/phase-b-large-motion/final-gui.json` | Models and generation |
| [final-reopened.png](../docs/evidence/phase-b-large-motion/final-reopened.png)<br>`docs/evidence/phase-b-large-motion/final-reopened.png` | Models and generation |
| [increment.json](../docs/evidence/phase-b-large-motion/increment.json)<br>`docs/evidence/phase-b-large-motion/increment.json` | Models and generation |
| [large-motion-process-summary.json](../docs/evidence/phase-b-large-motion/large-motion-process-summary.json)<br>`docs/evidence/phase-b-large-motion/large-motion-process-summary.json` | Models and generation |
| [large-motion-process.log](../docs/evidence/phase-b-large-motion/large-motion-process.log)<br>`docs/evidence/phase-b-large-motion/large-motion-process.log` | Models and generation |
| [launcher-os-red.log](../docs/evidence/phase-b-large-motion/launcher-os-red.log)<br>`docs/evidence/phase-b-large-motion/launcher-os-red.log` | Models and generation |
| [pre-launcher-doctor-revision3.png](../docs/evidence/phase-b-large-motion/pre-launcher-doctor-revision3.png)<br>`docs/evidence/phase-b-large-motion/pre-launcher-doctor-revision3.png` | Projects and interoperability |
| [pre-launcher-export-attempt.png](../docs/evidence/phase-b-large-motion/pre-launcher-export-attempt.png)<br>`docs/evidence/phase-b-large-motion/pre-launcher-export-attempt.png` | Projects and interoperability |
| [pre-launcher-gate-captured.log](../docs/evidence/phase-b-large-motion/pre-launcher-gate-captured.log)<br>`docs/evidence/phase-b-large-motion/pre-launcher-gate-captured.log` | Live sources and capture |
| [pre-launcher-gui-parity.log](../docs/evidence/phase-b-large-motion/pre-launcher-gui-parity.log)<br>`docs/evidence/phase-b-large-motion/pre-launcher-gui-parity.log` | Models and generation |
| [pre-launcher-gui.json](../docs/evidence/phase-b-large-motion/pre-launcher-gui.json)<br>`docs/evidence/phase-b-large-motion/pre-launcher-gui.json` | Models and generation |
| [pre-launcher-ready-yaw.png](../docs/evidence/phase-b-large-motion/pre-launcher-ready-yaw.png)<br>`docs/evidence/phase-b-large-motion/pre-launcher-ready-yaw.png` | Models and generation |
| [pre-launcher-redo-ready.png](../docs/evidence/phase-b-large-motion/pre-launcher-redo-ready.png)<br>`docs/evidence/phase-b-large-motion/pre-launcher-redo-ready.png` | Models and generation |
| [pre-launcher-reopened-revision3.png](../docs/evidence/phase-b-large-motion/pre-launcher-reopened-revision3.png)<br>`docs/evidence/phase-b-large-motion/pre-launcher-reopened-revision3.png` | Projects and interoperability |
| [pre-launcher-undo.png](../docs/evidence/phase-b-large-motion/pre-launcher-undo.png)<br>`docs/evidence/phase-b-large-motion/pre-launcher-undo.png` | Projects and interoperability |
| [source-hashes.sha256](../docs/evidence/phase-b-large-motion/source-hashes.sha256)<br>`docs/evidence/phase-b-large-motion/source-hashes.sha256` | Models and generation |
| [checkpoint-review.json](../docs/evidence/phase-b-portable-project/checkpoint-review.json)<br>`docs/evidence/phase-b-portable-project/checkpoint-review.json` | Projects and interoperability |
| [final-gate.log](../docs/evidence/phase-b-portable-project/final-gate.log)<br>`docs/evidence/phase-b-portable-project/final-gate.log` | Projects and interoperability |
| [p1a-golden-vector.json](../docs/evidence/phase-b-portable-project/p1a-golden-vector.json)<br>`docs/evidence/phase-b-portable-project/p1a-golden-vector.json` | Projects and interoperability |
| [p1a.json](../docs/evidence/phase-b-portable-project/p1a.json)<br>`docs/evidence/phase-b-portable-project/p1a.json` | Projects and interoperability |
| [p1b-commit-review.json](../docs/evidence/phase-b-portable-project/p1b-commit-review.json)<br>`docs/evidence/phase-b-portable-project/p1b-commit-review.json` | Projects and interoperability |
| [p1b-final-gate.log](../docs/evidence/phase-b-portable-project/p1b-final-gate.log)<br>`docs/evidence/phase-b-portable-project/p1b-final-gate.log` | Projects and interoperability |
| [p1b-model-redundant-mutant.log](../docs/evidence/phase-b-portable-project/p1b-model-redundant-mutant.log)<br>`docs/evidence/phase-b-portable-project/p1b-model-redundant-mutant.log` | Projects and interoperability |
| [p1b-model-review-red.log](../docs/evidence/phase-b-portable-project/p1b-model-review-red.log)<br>`docs/evidence/phase-b-portable-project/p1b-model-review-red.log` | Projects and interoperability |
| [p1b-model.log](../docs/evidence/phase-b-portable-project/p1b-model.log)<br>`docs/evidence/phase-b-portable-project/p1b-model.log` | Projects and interoperability |
| [p1b.json](../docs/evidence/phase-b-portable-project/p1b.json)<br>`docs/evidence/phase-b-portable-project/p1b.json` | Projects and interoperability |
| [p1c-codec-v2-tests.log](../docs/evidence/phase-b-portable-project/p1c-codec-v2-tests.log)<br>`docs/evidence/phase-b-portable-project/p1c-codec-v2-tests.log` | Projects and interoperability |
| [p1c-final-components.log](../docs/evidence/phase-b-portable-project/p1c-final-components.log)<br>`docs/evidence/phase-b-portable-project/p1c-final-components.log` | Projects and interoperability |
| [p1c-final-gate.log](../docs/evidence/phase-b-portable-project/p1c-final-gate.log)<br>`docs/evidence/phase-b-portable-project/p1c-final-gate.log` | Projects and interoperability |
| [p1c-final.json](../docs/evidence/phase-b-portable-project/p1c-final.json)<br>`docs/evidence/phase-b-portable-project/p1c-final.json` | Projects and interoperability |
| [p1c-gui-preview.json](../docs/evidence/phase-b-portable-project/p1c-gui-preview.json)<br>`docs/evidence/phase-b-portable-project/p1c-gui-preview.json` | Projects and interoperability |
| [p1c-interim.json](../docs/evidence/phase-b-portable-project/p1c-interim.json)<br>`docs/evidence/phase-b-portable-project/p1c-interim.json` | Projects and interoperability |
| [p1c-model.log](../docs/evidence/phase-b-portable-project/p1c-model.log)<br>`docs/evidence/phase-b-portable-project/p1c-model.log` | Projects and interoperability |
| [p1c-scanner-closure-gate.log](../docs/evidence/phase-b-portable-project/p1c-scanner-closure-gate.log)<br>`docs/evidence/phase-b-portable-project/p1c-scanner-closure-gate.log` | Projects and interoperability |
| [p1c-scanner-closure-source.json](../docs/evidence/phase-b-portable-project/p1c-scanner-closure-source.json)<br>`docs/evidence/phase-b-portable-project/p1c-scanner-closure-source.json` | Projects and interoperability |
| [p1c-scanner-closure.json](../docs/evidence/phase-b-portable-project/p1c-scanner-closure.json)<br>`docs/evidence/phase-b-portable-project/p1c-scanner-closure.json` | Projects and interoperability |
| [p1c-scanner-mutations.json](../docs/evidence/phase-b-portable-project/p1c-scanner-mutations.json)<br>`docs/evidence/phase-b-portable-project/p1c-scanner-mutations.json` | Projects and interoperability |
| [p1c-source-manifest.json](../docs/evidence/phase-b-portable-project/p1c-source-manifest.json)<br>`docs/evidence/phase-b-portable-project/p1c-source-manifest.json` | Projects and interoperability |
| [pre-capture-review-gate.log](../docs/evidence/phase-b-portable-project/pre-capture-review-gate.log)<br>`docs/evidence/phase-b-portable-project/pre-capture-review-gate.log` | Live sources and capture |
| [artifact-metric-checkpoint.log](../docs/evidence/phase-b/artifact-metric-checkpoint.log)<br>`docs/evidence/phase-b/artifact-metric-checkpoint.log` | Cross-project or reference |
| [closeout-cli.err](../docs/evidence/phase-b/closeout-cli.err)<br>`docs/evidence/phase-b/closeout-cli.err` | Cross-project or reference |
| [closeout-cli.out](../docs/evidence/phase-b/closeout-cli.out)<br>`docs/evidence/phase-b/closeout-cli.out` | Cross-project or reference |
| [closeout-doctor.png](../docs/evidence/phase-b/closeout-doctor.png)<br>`docs/evidence/phase-b/closeout-doctor.png` | Cross-project or reference |
| [closeout-export.png](../docs/evidence/phase-b/closeout-export.png)<br>`docs/evidence/phase-b/closeout-export.png` | Projects and interoperability |
| [closeout-gui-yaw.funscript](../docs/evidence/phase-b/closeout-gui-yaw.funscript)<br>`docs/evidence/phase-b/closeout-gui-yaw.funscript` | Projects and interoperability |
| [closeout-initial.png](../docs/evidence/phase-b/closeout-initial.png)<br>`docs/evidence/phase-b/closeout-initial.png` | Cross-project or reference |
| [closeout-open-retry.png](../docs/evidence/phase-b/closeout-open-retry.png)<br>`docs/evidence/phase-b/closeout-open-retry.png` | Cross-project or reference |
| [closeout-redo.png](../docs/evidence/phase-b/closeout-redo.png)<br>`docs/evidence/phase-b/closeout-redo.png` | Cross-project or reference |
| [closeout-review.png](../docs/evidence/phase-b/closeout-review.png)<br>`docs/evidence/phase-b/closeout-review.png` | Multimodal inputs |
| [closeout-undo.png](../docs/evidence/phase-b/closeout-undo.png)<br>`docs/evidence/phase-b/closeout-undo.png` | Projects and interoperability |
| [closeout-yaw.funscript](../docs/evidence/phase-b/closeout-yaw.funscript)<br>`docs/evidence/phase-b/closeout-yaw.funscript` | Projects and interoperability |
| [gates-good-increment.log](../docs/evidence/phase-b/gates-good-increment.log)<br>`docs/evidence/phase-b/gates-good-increment.log` | Cross-project or reference |
| [gates-pre-review.log](../docs/evidence/phase-b/gates-pre-review.log)<br>`docs/evidence/phase-b/gates-pre-review.log` | Multimodal inputs |
| [good-increment.json](../docs/evidence/phase-b/good-increment.json)<br>`docs/evidence/phase-b/good-increment.json` | Cross-project or reference |
| [gui-export-parity.log](../docs/evidence/phase-b/gui-export-parity.log)<br>`docs/evidence/phase-b/gui-export-parity.log` | Projects and interoperability |
| [metric-checkpoint.json](../docs/evidence/phase-b/metric-checkpoint.json)<br>`docs/evidence/phase-b/metric-checkpoint.json` | Cross-project or reference |
| [pre-review-cli.err](../docs/evidence/phase-b/pre-review-cli.err)<br>`docs/evidence/phase-b/pre-review-cli.err` | Multimodal inputs |
| [pre-review-cli.out](../docs/evidence/phase-b/pre-review-cli.out)<br>`docs/evidence/phase-b/pre-review-cli.out` | Multimodal inputs |
| [pre-review-gui-generator.png](../docs/evidence/phase-b/pre-review-gui-generator.png)<br>`docs/evidence/phase-b/pre-review-gui-generator.png` | Multimodal inputs |
| [pre-review-gui-yaw-open.png](../docs/evidence/phase-b/pre-review-gui-yaw-open.png)<br>`docs/evidence/phase-b/pre-review-gui-yaw-open.png` | Multimodal inputs |
| [pre-review-yaw.funscript](../docs/evidence/phase-b/pre-review-yaw.funscript)<br>`docs/evidence/phase-b/pre-review-yaw.funscript` | Projects and interoperability |
| [source-kind-final-cli.err](../docs/evidence/phase-b/source-kind-final-cli.err)<br>`docs/evidence/phase-b/source-kind-final-cli.err` | Multimodal inputs |
| [source-kind-final-cli.out](../docs/evidence/phase-b/source-kind-final-cli.out)<br>`docs/evidence/phase-b/source-kind-final-cli.out` | Multimodal inputs |
| [source-kind-final-diagnostics.json](../docs/evidence/phase-b/source-kind-final-diagnostics.json)<br>`docs/evidence/phase-b/source-kind-final-diagnostics.json` | Multimodal inputs |
| [source-kind-final-gui-migrated-source.png](../docs/evidence/phase-b/source-kind-final-gui-migrated-source.png)<br>`docs/evidence/phase-b/source-kind-final-gui-migrated-source.png` | Multimodal inputs |
| [source-kind-final-gui-yaw.png](../docs/evidence/phase-b/source-kind-final-gui-yaw.png)<br>`docs/evidence/phase-b/source-kind-final-gui-yaw.png` | Multimodal inputs |
| [source-kind-final-yaw.funscript](../docs/evidence/phase-b/source-kind-final-yaw.funscript)<br>`docs/evidence/phase-b/source-kind-final-yaw.funscript` | Projects and interoperability |
| [artifact-tests.mjs](../research/phase-b/artifact-tests.mjs)<br>`research/phase-b/artifact-tests.mjs` | Cross-project or reference |
| [corpus.pending.json](../research/phase-b/corpus.pending.json)<br>`research/phase-b/corpus.pending.json` | Cross-project or reference |
| [metric-kat.json](../research/phase-b/fixtures/metric-kat.json)<br>`research/phase-b/fixtures/metric-kat.json` | Cross-project or reference |
| [synthetic-candidate.json](../research/phase-b/fixtures/synthetic-candidate.json)<br>`research/phase-b/fixtures/synthetic-candidate.json` | Cross-project or reference |
| [synthetic-corpus.json](../research/phase-b/fixtures/synthetic-corpus.json)<br>`research/phase-b/fixtures/synthetic-corpus.json` | Cross-project or reference |
| [synthetic-reference.json](../research/phase-b/fixtures/synthetic-reference.json)<br>`research/phase-b/fixtures/synthetic-reference.json` | Cross-project or reference |
| [synthetic-source.json](../research/phase-b/fixtures/synthetic-source.json)<br>`research/phase-b/fixtures/synthetic-source.json` | Multimodal inputs |
| [historical-regressions.json](../research/phase-b/historical-regressions.json)<br>`research/phase-b/historical-regressions.json` | Cross-project or reference |
| [cuda-owner-policy-probe.mjs](../research/phase-b/prototypes/cuda-owner-policy-probe.mjs)<br>`research/phase-b/prototypes/cuda-owner-policy-probe.mjs` | Models and generation |
| [cuda-runtime-probe.cpp](../research/phase-b/prototypes/cuda-runtime-probe.cpp)<br>`research/phase-b/prototypes/cuda-runtime-probe.cpp` | Models and generation |
| [cuda-runtime-probe.mjs](../research/phase-b/prototypes/cuda-runtime-probe.mjs)<br>`research/phase-b/prototypes/cuda-runtime-probe.mjs` | Models and generation |
| [cuda-sdk-identity-probe.mjs](../research/phase-b/prototypes/cuda-sdk-identity-probe.mjs)<br>`research/phase-b/prototypes/cuda-sdk-identity-probe.mjs` | Models and generation |
| [rust-cuda-adapters-probe.mjs](../research/phase-b/prototypes/rust-cuda-adapters-probe.mjs)<br>`research/phase-b/prototypes/rust-cuda-adapters-probe.mjs` | Models and generation |
| [rust-cuda-session-probe.mjs](../research/phase-b/prototypes/rust-cuda-session-probe.mjs)<br>`research/phase-b/prototypes/rust-cuda-session-probe.mjs` | Models and generation |
| [current-contract-correspondence-2026-09-11-successor.json](../research/phase-b/regressions/current-contract-correspondence-2026-09-11-successor.json)<br>`research/phase-b/regressions/current-contract-correspondence-2026-09-11-successor.json` | Architecture and assurance |
| [current-contract-correspondence.json](../research/phase-b/regressions/current-contract-correspondence.json)<br>`research/phase-b/regressions/current-contract-correspondence.json` | Architecture and assurance |
| [legacy-20.json](../research/phase-b/regressions/legacy-20.json)<br>`research/phase-b/regressions/legacy-20.json` | Cross-project or reference |
| [b1-artifact-binding.json](../research/phase-b/results/b1-artifact-binding.json)<br>`research/phase-b/results/b1-artifact-binding.json` | Cross-project or reference |
| [b1-empty-heldout.json](../research/phase-b/results/b1-empty-heldout.json)<br>`research/phase-b/results/b1-empty-heldout.json` | Cross-project or reference |
| [b1-independent-oracle.json](../research/phase-b/results/b1-independent-oracle.json)<br>`research/phase-b/results/b1-independent-oracle.json` | Cross-project or reference |
| [b1-metric-kat.json](../research/phase-b/results/b1-metric-kat.json)<br>`research/phase-b/results/b1-metric-kat.json` | Cross-project or reference |
| [b1-synthetic-example.json](../research/phase-b/results/b1-synthetic-example.json)<br>`research/phase-b/results/b1-synthetic-example.json` | Cross-project or reference |
| [annotation.schema.json](../research/phase-b/schemas/annotation.schema.json)<br>`research/phase-b/schemas/annotation.schema.json` | Cross-project or reference |
| [corpus.schema.json](../research/phase-b/schemas/corpus.schema.json)<br>`research/phase-b/schemas/corpus.schema.json` | Cross-project or reference |
