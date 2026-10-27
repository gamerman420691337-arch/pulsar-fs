# ADR B3-029: Durable CUDA candidate evidence

Status: Implemented completion/membership seam; GPU scheduling disabled.

## Problem

`run_job` previously took only `WorkerOutput` from the launcher and discarded the original CUDA evidence carried by `LaunchExecution`. The schema-15 intent barrier did not define accepted-result membership, so an old reader could consume a completed candidate without those checks.

## Decision

Keep the complete launch result through the engine's completion transaction. Require CUDA evidence for CUDA requests, reject it for CPU requests, independently verify the parent copies and retain both identities in candidate lineage. Require exact current running request/attempt and the live engine epoch before candidate publication.

Publish bounded canonical role metadata and a conditional schema-16 reader barrier with candidate/job/attempt completion. Validate layout, capacity, intent, candidate scope, lineage, state and artifact bytes at read-only startup. Preserve schema 16 through existing migrations. Refuse portable project capture for affected projects until its accepted-CUDA closure is implemented.

Opaque lineage alone was rejected because it would not preserve typed role membership. Enabling GPU scheduling before capture/admission/retirement integration was rejected because result consistency does not establish launch authority. Treating hashed trace metadata as authentic native provenance was rejected because owner-equivalent processes can forge or alter it.

## Evidence and remaining work

Authored tests reach the real completion transaction and the real reader/WAL boundary. Rebuilt confined native CUDA generation remains a separate worker scenario. The [contract](../../docs/PULSAR_CUDA_CANDIDATE_EVIDENCE.md) records their distinct scopes and retained fixture failure.

This is not end-to-end engine GPU qualification. Portable closure, aggregate admission, durable launch, independent IO deadlines, accepted-result custody through retirement/restart, trace completeness and model/device/platform/release gates remain required.

Rollback: keep GPU scheduling disabled and stop new CUDA candidate acceptance. Existing schema-16 projects require a reader that preserves the evidence; never downgrade the database or erase records as rollback.
