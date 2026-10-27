# ADR B3-079: explicit source-bound annotated Preview

Status: implemented and verified against authored CPU fixtures. Roadmap PARTIAL.

## Problem

Single-frame detector Preview cannot interpret an annotated temporal model or choose its target implicitly. A temporal result needs the exact target pixels, original source sample pair and model/profile identities, with absence kept explicit.

## Decision

Add a distinct annotated Preview request with a source-bound view, nonzero source stride, explicit file profile or owner-selected pack, and explicit source/placement time. Reuse the existing confined Preview lifecycle, imported-source authority, native annotated spatial plan and frame-pair adapter. Keep legacy detector commands unchanged. Choose the latest original stride sample at or before the requested source time and the immediately preceding sample in original source order, including reverse placements.

Return target pixels and separately typed unqualified temporal evidence. Preserve first-frame, scene-cut and >500 ms discontinuity absence. Transfer both pixel artifacts through bounded hashed egress, verify original model/profile/runtime/recipe/view and pair support before completion, and require current owner choice throughout pack execution. Do not create a generation job or mutate motion.

## Verification and limits

See [implementation and evidence](../../docs/PULSAR_ANNOTATED_PREVIEW.md). Authored CPU correspondence is separate from useful natural-media model quality, actual desktop/device qualification and human acceptance. Annotated CUDA and desktop wiring remain open. The request uses a new command variant; optional worker/output extensions serialize away for legacy Preview. No persistent Preview-job or storage schema is introduced.
