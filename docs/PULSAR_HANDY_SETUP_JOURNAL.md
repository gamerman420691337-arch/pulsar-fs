# Durable setup intents and single-use claims

Current storage is version 3: [retained setup observations](PULSAR_HANDY_SETUP_OBSERVATIONS.md)
extend the version-2 lifecycle below with atomic response data and explicit legacy absence.
Public [unclaimed cancellation](PULSAR_HANDY_SETUP_CANCELLATION.md) is also available.

The Engine's device effect journal now has a setup lifecycle. The native backend is now used by the separate
[guarded setup sender](PULSAR_HANDY_SETUP_SEND.md); the journal itself performs no network I/O. A transport caller
must first supply fresh qualification, controller authority and the correct opaque credential
route. It must consume a successfully committed claim before any external setup request.

A record binds the actor/request identity, hosting request, Engine boot, device session,
registration, qualification revision, project/revision/master and exact script/download
identities. It stores neither credential values nor hosting URLs. Request identity is stable
across boots: changing the hosting selection or route cannot reuse the same actor/request ID.
The caller is responsible for deriving a private route identity that remains consistent
across aliases and restarts. That derivation is not supplied by the journal itself.

| State | Meaning | Route reserved |
| --- | --- | --- |
| Prepared | Metadata committed; no claim issued | Yes |
| Claimed | Dispatch claim consumed; effect may or may not have occurred | Yes |
| Acknowledged | Caller recorded its transport acknowledgment | Yes |
| Unknown | Caller recorded an uncertain transport outcome | Yes |
| Cancelled | Explicit cancellation of a provably unclaimed preparation | No |

Preparation can repeat its identical pending metadata receipt. Claim commits before returning
a non-cloneable, non-deserializable token and cannot repeat. Completion may retry only the
same metadata outcome; it does not redispatch. Failed completion leaves the claimed record
and reservation intact. An acknowledgment does not establish loaded-program authority or
physical stopping. Attempted records need separate reconciliation before route release;
that workflow is not implemented here. No quota reclamation or automatic pruning is added.

A new boot fences old writers and receipts. History cannot reconstruct a dispatch token.
The current owner can cancel a still-unclaimed record by its exact ID, including after
restart; it cannot cancel a claimed/acknowledged/unknown attempt. Cancellation remains
metadata-only and cannot stop or retract a remote operation.

## Storage and compatibility

Journal version 2 adds setup metadata and unique request/active-route indices. The
transactional upgrade preserves version-1 play/stop rows byte for byte. Setup has a separate
1024-record cap and 8-KiB per-record bound. Startup validates bounded metadata and refuses
unknown layouts or versions; no automatic repair is attempted. The current reader performs
a read-only preflight before opening writable SQLite, so refusal cannot checkpoint a
committed future-version WAL.

The preceding reader refuses version 2 but its older writable-open path can change WAL
bytes while leaving logical rows unchanged. This retained compatibility finding is not
reported as byte-preserving refusal. Do not use an older binary as a recovery procedure.

The [manufacturer setup contract](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)
defines a distinct HSSP setup operation and response. Connecting that transport, loaded-program
correlation, actual authority/route acquisition and reconciliation remain work beyond this
journal. Physical firmware, stopping and human qualification gates remain open.

See [ADR B5-031](../research/phase-b/ADR-B5-031-single-use-setup-journal.md) and
[checkpoint](evidence/roadmap-2026-10-16/handy-setup-journal/checkpoint.json).

## Scoped verification

Eight setup-journal tests, 51 playback regressions and 88 Handy checks pass, along with both
strict workspace lint configurations, build, formatting and architecture checks. Native CLI
fixtures verify the version-1 journal upgrade, exact retained legacy records, qualification
recovery and refusal of unsupported metadata. The retained pre-fix binary changes database/
WAL bytes on future-version refusal; the rebuilt reader preserves them. The preceding reader
refuses version 2 with unchanged logical rows but may reset WAL bytes. These are synthetic
SQLite/application checks in capped private memory filesystems, not disk power-loss or
physical-device qualification. No real setup or acceptance record was created.

The [setup recovery command](PULSAR_HANDY_SETUP_RECOVERY.md) now resolves original actor/request
identities through the shared Engine/CLI path. New records pin a keyed route namespace;
active legacy or changed namespaces block new preparations, and logical-device reservations
survive connection-key changes. Historical reads still restore no claim or loaded authority.

The sender durably links a prepared record from the project database before claiming it.
Read-only startup validation refuses missing linked journal records before any automatic
storage initialization. See the current setup-sender checkpoint for that cross-store check.
