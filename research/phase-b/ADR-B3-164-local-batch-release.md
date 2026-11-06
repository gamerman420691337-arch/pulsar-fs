# ADR B3-164: explicit local batch metadata release

Date: 2026-10-12. Status: implemented software contract, verified scoped checkpoint; full roadmap PARTIAL.

## Problem

The local generation ledger pins captured source membership and admits at most 128 batches. Without an explicit release, completed or never-started records consume capacity indefinitely. Discarding a pending phase would lose its exact recovery authority.

## Decision

Add authenticated global `LibraryBatchRelease { batch_request_id }` and `library-batch-release ID --request-id RELEASE_ID`. Permit only a record whose entries are all selected or ready_for_publication, with no item errors or retained inner/outer acknowledgement. Refuse partial generation and foreign ownership. This does not dispatch cancellation or alter project grants.

Under the existing batch gate and a reauthorized database transaction, remove only the owner's source membership and batch record, and remember the exact final-report receipt atomically. The standard request fingerprint/replay contract recovers a lost reply across restart. The CLI requires an explicit request identity. Retain the storage reader header and every filesystem object and downstream project/job/candidate/export record. Pins owned by other batches remain effective. A new request cannot release an absent record.

## Consequences and evidence

Release frees the bounded ledger slot and removes only this batch's snapshot pin. Existing source eviction and artifact ownership checks remain authoritative. Historical preparation and advance replies remain replayable history; they do not restore released membership. The release receipt retains generation identifiers without asserting current project revision, publication, quality or qualification. It records removed memberships, not unique objects or reclaimed disk bytes.

Current configured suites pass 1,507 tests (80 ignored): Engine 862, Clients 347, Protocol 160 library tests plus selected-package integrations. Both strict Clippy configurations, Windows source check, native builds, formatting and source/architecture guards pass. Tests cover owner/refusal/restart replay, exact request conflicts, preserved files, pending/partial refusal and the 128-record capacity with shared source pins. An actual network-disabled native campaign generated two six-axis projects, separately published twelve scripts, refused partial release, then removed only one batch/two source memberships. Restart replays the identical receipt and published bytes remain unchanged. Three Engine instances were joined; owned scratch was removed after accessible reference audit. See [checkpoint](../../docs/evidence/roadmap-2026-10-12/library-release/checkpoint.json). General garbage collection, cancellation/reconciliation of partial batches, session migration, aggregate publication scheduling, physical durability and broader platform qualification remain open.
