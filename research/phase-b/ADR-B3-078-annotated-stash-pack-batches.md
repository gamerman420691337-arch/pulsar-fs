# ADR B3-078: annotated Stash batches from selected packs

Status: implemented with authored native evidence. Roadmap PARTIAL.

## Problem

Direct annotated pack generation and file-model annotated Stash batches existed, but combining an annotated catalog with a selected pack was refused. Validating an unbound batch template as ordinary single-source generation would either reject the template or encourage sharing one source binding across jobs.

## Decision

Keep the catalog/template distinction. Allow a selected pack in place of a file model, validate every resolved catalog entry against its exact selected profile during batch preparation, and keep ordinary single-source generation validation unchanged. Check selected eligible catalog coverage before annotated pack model capture. Retain early unsupported-provider refusal for file models and legacy pack preparation behavior.

Dispatch one source-bound GenerateFromModelPack command per captured source. Require the current owner choice for each job and migration; retain exact original pack/profile/approval dependencies. Reuse the existing version-5 ledger and strict old-reader refusal. Migration preserves previous intent and remains paused until explicit resume.

## Verification and limits

See [implementation and evidence](../../docs/PULSAR_ANNOTATED_STASH_PACK_BATCHES.md). Authored native correspondence does not establish useful natural-media quality, installed Stash compatibility, physical Handy qualification or release acceptance. Annotated CUDA, target-bound Preview and placed/interval batch contracts remain separate work.
