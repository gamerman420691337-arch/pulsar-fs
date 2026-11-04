# ADR B3-076: per-source annotated batch intent

Status: implemented with scoped authored evidence; roadmap PARTIAL.

## Problem

The restartable Stash batch ledger retained one shared generation setting and rejected annotated profiles. An annotated target must be bound to exact source content; copying one view to every selected scene would violate that requirement.

## Decision

Retain a bounded catalog keyed by exact Stash scene/file IDs, each carrying the shared typed annotated source binding. Batch settings are a template; resolve and validate one complete setting against each retained capture before its generation request. Require complete eligible selection coverage before model capture. Restrict this batch adapter to explicit CPU file models and unplaced full sources, retaining existing guards for unsupported packs/CUDA/placement orchestration.

Use ledger/report version 5 with the existing schema-13 pre-recovery validator. Atomically arm that minimum reader without downgrading later worker/portable storage layers. Reject missing/downgraded requirements; omit empty legacy fields. Migration replaces the complete catalog under existing observed-ledger CAS and owner authority, retains old catalogs and accepted jobs, and stays paused until explicit resume. Historical catalogs keep their reader requirement.

## Verification

The native two-source campaign proves distinct source/view selection, exact Python correspondence, restart recovery and terminal replay. A third deliberately mismatched source refuses before a job; explicit migration and separate resume repair future intent without erasing old metadata. The preceding executable refuses before changing observed database/WAL bytes. Focused and full workspace gates plus preceding Stash campaigns pass. See [the detailed scope](../../docs/PULSAR_ANNOTATED_STASH_BATCHES.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-08/annotated-stash-batches/checkpoint.json).

## Limits and rollback

No installed Stash, natural-media quality, GUI/human usability, physical Handy or release qualification is claimed. The report barrier does not establish unchanged startup custody/filesystem preparation. Reverting support must preserve or refuse retained version-5 records; do not drop catalogs, rewrite historical jobs or lower database reader requirements to make an older executable recover them.
