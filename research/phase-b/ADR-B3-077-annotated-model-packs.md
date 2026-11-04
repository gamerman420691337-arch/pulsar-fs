# ADR B3-077: selected annotated CPU pack generation

Status: implemented with authored native and portable evidence; roadmap PARTIAL.

## Problem

The pack store rejected all annotated profiles even after direct annotated generation, receipt validation and portable closure were implemented. A selected pack must retain explicit recipe/model/profile identities and must not supply a target or infer source correspondence.

## Decision

Use the existing schema-1 pack assets plus the content-bound annotated ONNX recipe. Choose the shared bounded annotated ABI/metadata parser during pack integrity admission, then require the Engine-pinned runtime and CPU tensor adapter for explicit owner selection. Permit direct selected-pack generation only with an explicit source-bound view and complete ordinary generation settings. Validate source binding and settings before model capture. Retain existing current-selection/grant/revision checks, original pack/profile/approval dependencies, confined worker and annotated membership/receipt barriers.

Keep qualification flags false. Annotated Preview and annotated Stash pack batches remain guarded until their distinct target/template contracts are implemented. Legacy pack and provider paths remain covered by regressions.

## Evidence

The shipped native campaign preserves 28 samples, 14 exact Python estimates and exact pack/profile/selection documents. Actual format-11 download/import, re-export with original pack/model paths unavailable, and second-clone restart pass without new active jobs. Rehashed recipe/name corruption, missing or wrong source binding, installed-byte drift and removed approval refuse. A preceding executable explicitly rejects the pack. See [implementation scope](../../docs/PULSAR_ANNOTATED_MODEL_PACKS.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-08/annotated-model-packs/checkpoint.json).

## Limits and rollback

The streaming parser is not a full ONNX graph verifier. This evidence does not establish trained quality, rights, installed-platform compatibility, desktop/human usability or physical Handy qualification. Reverting admission must refuse annotated packs and preserve existing schema-23/24 histories; do not erase reader barriers, approval records or retained model objects to make old execution appear compatible.
