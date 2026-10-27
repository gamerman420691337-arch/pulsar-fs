# ADR B3-168: desktop retained audio library workflow

Date: 2026-10-12. Status: implemented software contract; scoped verification. Full roadmap PARTIAL.

## Decision

Expose the existing retained audio-recipe library workflow in the desktop through a single background controller. Share validated intake with the CLI and the existing generation/publication scheduler and journals. Freeze intake before possible dispatch; retain original identities and options on unknown acknowledgement. Permit read-only inspection during recovery and refuse new mutations until the retained operation is reconciled. Show current cancellation policy and preserve its original request ID for incomplete retries.

Provide local pause through an atomic control checked between scheduler decisions. The current operation may finish; durable Engine cancellation remains separate. Do not claim immediate worker retirement, rollback or physical stop. Keep the ordinary CLI runner unchanged through an unstopped default control. Add no Engine/Protocol/storage contract or default model.

## Evidence and boundaries

See [contract](../../docs/PULSAR_DESKTOP_LIBRARY_BATCHES.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-12/desktop-library/checkpoint.json). Seven headless panel tests and the actual network-disabled native controller IPC fixture pass; twelve published scripts match retained authored timing oracles. Final CLI scheduler restart/failure regression passes. Client tests pass 366 with ten ignored, both strict lint configurations and remaining scoped checks pass. Seven owned Engines are joined. Initial compile/recovery/lint/harness failures remain recorded.

Native rendered desktop acceptance remains open. Visual/model folder workflow parity, broader input/provider/edition work, fault/platform/model/media/device/human and release gates are not closed. This software creates no missing training annotations or useful default qualification.
