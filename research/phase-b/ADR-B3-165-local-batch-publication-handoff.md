# ADR B3-165: client-owned local batch publication handoffs

Date: 2026-10-12. Status: implemented software contract; scoped validation passes. Full roadmap PARTIAL.

## Decision

Connect retained local generation to the existing atomic neutral publisher with a private client journal. Capture immutable generation selection, current credential digest, exact prepared stage/export request identity and parent/stage filesystem identity before exports. Reuse current project grants, master revision fences, Engine exact-request acknowledgement checks, full script verification and no-replace publication. Store actual publisher receipts per entry; a later failure cannot erase earlier success.

Expose explicit prepare, publish and status commands with bounded steps. Prepare dispatches no exports. A prepared resume compares captured identities before dispatch, so an identical intent copied into another directory inode cannot replace the original stage. Refuse unresolved preparing state and missing stage after a lost local publication acknowledgement. Matching destination bytes are observation, not acknowledgement or retry authority.

Store only nonsecret binding and private bounded metadata. The journal uses held directory descriptors, named identity checks, exclusive locks, atomic replacement and sync. Locks explicitly unlock on retirement even with inherited aliases. No new database schema or client dependency on the Engine implementation is introduced. Generation publication_completed remains false; independent client history does not become an Engine truth claim.

## Remaining scope

This connects explicit generation and publication with durable per-entry handoffs. It is not a unified autonomous folder scheduler, general cleanup, partial cancellation, migration or resolution of unknown acknowledgements. Platform, death-at-every-boundary, physical durability, real-model/media/device/human and release gates remain open. See [contract](../../docs/PULSAR_LOCAL_BATCH_PUBLICATION.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-12/library-publication/checkpoint.json).

Scoped verification passes 1,512 configured selected-package tests and all eight retained checks. Actual Linux CLI preparation/restart/conflict/exact-resume, repeated completion, non-adoption of an authored lost-ack state and the legacy controller regression pass. Five Engine instances are joined; owned scratch is audited and removed. The checkpoint retains initial fixture/compiler/setup failures and the precise boundaries of this evidence.
