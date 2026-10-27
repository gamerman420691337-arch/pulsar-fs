# ADR B3-075: annotated Stash publication through shipped portable download

Status: implemented with scoped authored evidence; roadmap PARTIAL.

## Problem

Annotated format-11 capture/import passed preceding Engine tests, but the shipped package download path rejected its retained descriptor. The store had independent descriptor and READY header ceilings ending at format 10. This prevented an actual CLI clone before archived Stash publication.

## Decision

Admit format 11 in both existing store checks while retaining all identity, header, size and authorization checks. Refuse unknown versions. Add a bounded descriptor regression and an actual CLI/local-IPC/native-worker Stash campaign that downloads, imports, freshly rebinds and publishes from a clone with original attempt/model paths unavailable.

The campaign also requires explicit source-time selection and gap-resolved motion. A deliberate trailing gap refuses; only explicit replacement trim and regeneration make the authored publication admissible. Unknown scan acknowledgement stays unknown through restart and attachment observation; exactly one remote mutation is allowed for that publication.

## Evidence and limits

See [implementation and campaign](../../docs/PULSAR_ANNOTATED_STASH_PUBLICATION.md) and its [checkpoint](../../docs/evidence/roadmap-2026-10-08/annotated-stash-publication/checkpoint.json). The mock does not qualify an installed Stash release. Authored model correspondence is not learned quality. Generic annotated batches/model packs/CUDA, broader recovery, desktop, Handy, platform and human/release gates remain independent.

## Rollback

Remove the two format-11 storage admissions and new regressions to return to the previous download refusal. Do not rewrite stored packages, drop reader requirements, alter historical evidence or claim existing format-11 files are compatible with older readers. No database migration is introduced by this repair.
