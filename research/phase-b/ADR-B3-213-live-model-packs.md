# ADR B3-213 — Fresh pack-bound live capture

Date: 2026-10-14
Status: Implemented and scoped software verification passed; qualification PARTIAL
Architecture: ARCH160

## Problem

Browser live supports explicit file models, while the desktop's owner-selected offline packs have fresh selection/approval and integrity semantics. Reusing a pack path as raw inference would lose classification and allow a continuation to escape the choice fence.

## Decision

Add optional model_pack choice to live options and recording pages with legacy omission. Resolve a pack-only admission inside Engine through shared pack checks/snapshot preparation, outside database/registry locks and under the artifact gate. Recheck actor/revision in the ordinary live handler. Retain original request fingerprint/ID, reject conflicting inference and enforce equal pack identity during continuation. Only pack admissions use the artifact gate; raw live paths keep their original locking behavior.

Continue-at inherits and freshly rechecks the original choice. The registry retires a pack-bound capture when its current selection disappears/changes. Historical replay observes an existing receipt without execution. Recording/assembly/archive/durable source proof carry the admitted choice and reject mixed origins. Desktop capture is explicit, displays choice/profile and freezes the pack-only request; it performs no selection/approval/import or raw-path extraction.

## Limits

The diagnostic pack and fixture owner decisions are test inputs, not production human acceptance. Capture cancellation is not device stopping. An immutable admitted model remains immutable after filesystem changes; new admission rechecks bytes and approval. Long-term approval-document portability, broader concurrency/faults, useful models/media, real rendering/platform, source-player clocks, Handy/six-axis and human/release evidence remain open. No physical playback is enabled.

See [contract](../../docs/PULSAR_LIVE_MODEL_PACKS.md) and [source-bound checkpoint](../../docs/evidence/roadmap-2026-10-13/live-model-packs/checkpoint.json).
