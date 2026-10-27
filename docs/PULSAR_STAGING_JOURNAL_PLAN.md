# Durable staging and creation ownership

Status: [production format-3/live-inference integration](PULSAR_STAGING_FORMAT3.md) implemented; complete recovery acceptance pending. Three [selected shipped-engine crash scenarios](PULSAR_STAGING_ENGINE_CRASH.md) now pass on the production integration. The actual bound native primitive now reproduces admission with unaccounted staging after an owned process is killed, twice; see [the crash regression](PULSAR_STAGING_CRASH_REGRESSION.md). The current in-process cleanup implementation does not satisfy this stage. A [test-only lifecycle prototype](PULSAR_STAGING_OBLIGATION_PROTOTYPE.md) now exercises intent/identity/retirement/clear ordering at eight actual owned-process kill boundaries and rejects replaced parents/roots. The [schema-19 reader requirement](PULSAR_STAGING_READER_REQUIREMENT.md) is now implemented and focused-tested, with fixture-only arming. The forward production journal and tracked live-inference transfer are implemented; full engine crash recovery, native images, cgroup creation intent and legacy migration remain pending.

The [glossary](../CONTEXT.md) separates persistent ownership scope from capture epoch and cancellation acknowledgement from completed cleanup. Existing worker-fence recovery validates cgroup identities and emptiness, then reclaims the recorded groups before SQLite is opened for writes. It does not record live-preparation snapshot roots or native images. A pending cleanup task retains capacity only while its owning process survives.

## Stage card

- **Goal:** preserve creation and cleanup obligations across process death, without freeing unknown resources or deleting unrelated state.
- **Claim:** admission can resume only after every retained obligation is reconciled by exact identity and confirmed retirement/cleanup. Cancellation, process exit, an absent pathname and an empty cgroup are separate observations.
- **Deliverable:** durable creation intent and staging identity records integrated with the engine's ownership scope, live preparation, worker transfer, checked cleanup and restart. Include cgroup creation-before-registration and native-image ownership; preserve the full roadmap's remaining lifetimes.
- **Gate:** reproduce the current crash-to-unaccounted-directory gap through the actual bound native ownership primitive, then through the engine. Exercise every cut below with actual owned-process termination, exact byte readback, retained claims and reader DB/WAL preservation. Seeded records alone are insufficient for crash claims.
- **Means:** declare intent before resource creation; bind the created identity before transfer or use; retain a retirement tombstone until deletion and durable clear are confirmed. Reuse engine scope and held directory identities. Keep metadata/capture/cleanup work off deadline-sensitive callers and retain claims while host I/O blocks. Refuse ambiguous or replaced resources instead of scanning shared temporary directories.
- **Provenance:** retain the current 311-path checked-cleanup subject and all failed storage gates. The next subject must archive its own source, native identities, state cuts and reader evidence. Authored fixtures remain separate from model/media/device qualification.
- **Rollback:** preserve the preceding source and private failure states. Refuse affected admission if journal publication/recovery cannot establish ownership. A rollback must not discard outstanding obligations or downgrade reader protection.

## Crash cuts and required observations

| Cut | Required recovery behavior |
| --- | --- |
| Before acknowledged intent | No resource creation is authorized. Preserve any uncertain journal publication for validation. |
| Intent acknowledged, before creation | Verify the intended resource is absent in its bound parent before clearing the intent. |
| Creation completed, identity not acknowledged | Retain the intent. An existing unbound resource is ambiguous; its name alone does not authorize deletion or capacity release. |
| Identity acknowledged, before worker use | Recover only the recorded identity; preserve the obligation through cleanup failure. |
| Worker or capture still active | Retain its scope, capacity and byte ownership until actual use is retired; no implicit restart or physical replay. |
| Work retired, cleanup pending | Reconcile the owned staging identity and complete checked deletion before admission release. |
| Deletion confirmed, durable clear pending | Retain a tombstone that can prove an absent resource is a completed deletion rather than a missing active record. |

Every cut also needs missing/changed parent identity, replacement directory, symlink, malformed/oversized/noncanonical record, missing obligation, storage/permission failure and lost acknowledgement variants. No new hardware actuation is part of these tests.

## Reader and portability decisions to settle

An older reader must refuse the new obligations before opening SQLite for writes. Merely adding a separate staging file would let a format-2 reader ignore it. The host-local fence accepts canonical formats 1/2/3; tracked live inference enables format 3 and binds its minimum through schema 19. The schema-19 binding now requires the exact minimum-version layout before reconciliation, and the actual preceding reader refusal and missing-column regression are checked. The versioned fence extension and complete obligation integration remain required; a missing/downgraded obligation must not be hidden by an accepted empty map. Do not infer that an unknown-field refusal alone establishes complete downgrade protection.

Portable project export must preserve project/model evidence without exporting original host staging paths or granting ownership over the original host's resources. Evaluate whether a new conditional reader requirement affects logical archive correspondence and clone/re-export behavior. Retain exact preceding readers and their positive pre-feature opening cases before verifying refusal.

The prototype establishes the creation/identity handoff and deletion/clear ordering at its retained actual process-kill cuts. Before production transfer, creation failures must retain the pending obligation and admitted reservation even if an error occurs after creation but before identity acknowledgement. The raw lifecycle prototype error return alone does not provide that resource custody. The [admitted creation prototype](PULSAR_STAGING_CREATION_CUSTODY.md) now retains a reservation and captured identity through failed publication, checked retirement/deletion and clear acknowledgement, with explicit byte-user retirement required for successful handles. Its preceding in-memory adapter remains a test fixture. Production format-3 integration now supplies the forward tracked live-inference journal; complete engine crash qualification and the other ownership families are still required. Boot changes, hostile same-UID races, complete GPU/device ownership, universal host-I/O guarantees and platform/model/media/Handy/human/release gates remain independent requirements; none is closed by this plan.
