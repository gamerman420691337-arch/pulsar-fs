# Durable original play-operation custody

The device journal now supports typed original play operations. This is the recovery foundation
for native play submission; it adds no public play command and does not enable physical playback.

A play record binds the actor, original request and fingerprint, original controller generation,
project/revision, device/session, qualification, acknowledged setup, clock request/snapshot,
start offset and exact effect metadata. The preparation ID must derive from that actor, Engine
boot, preparation request and setup record. The setup parent must still match the original
device/session, project, qualification and hosted program. These stored facts are historical
metadata; they cannot recreate current grants, a controller or a native clock after restart.

`capture_play_intent` checks correspondence to the current sealed runtime effect without
database I/O. `prepare_play` writes the original operation and effect in one transaction.
Exact repeated preparation may return another prepared token while the original remains
unclaimed. A different fingerprint or record under the same actor/request refuses. An existing
legacy effect cannot be adopted, because its prepared row may already have been sent.

Only `claim_play` releases a dispatch receipt. Its transaction changes Prepared to Claimed
once; duplicate claims refuse. The caller must still verify current Engine authority and timing
through the dispatch claim continuation before HTTP. A durable journal claim is not permission
to play and is not proof that the request reached the device.

`complete_play_operation` commits the operation state and effect outcome atomically. Exact
metadata retry is accepted; contradictory outcomes refuse. Generic effect preparation and
completion cannot bypass an operation that owns the effect. Nonblocking forms preserve the
shared journal's ordinary busy timeout while allowing the execution worker to retry metadata.
Transport acknowledgment still supplies no physical motion or stopping proof.

Only an exact Prepared operation may be cancelled as unclaimed. The Cancelled result can be
read and repeated, including after restart. Claimed, Acknowledged and Unknown operations cannot
be cancelled as unsent. Cancellation does not change runtime state or clear physical uncertainty.
Lost claimed work remains unresolved; neither request history nor a journal record can reconstruct
the native dispatch token. Older Prepared effect rows retain their prior uncertainty semantics.

## Migration and verification

Opening the current build upgrades the device journal to version 5. Main project schema 34 is
unchanged. Setup, synchronization and legacy play/stop rows retain their original bytes; the
upgrade creates no play-operation records for them. Earlier v4 binaries refuse the new journal.
Read-only validation checks schema, bounded records, indexes, parent/effect correspondence and
state/outcome consistency before changing ownership. `verify_play_records` supplies the exact
read-only record check needed by the forthcoming main-request witness integration.

The [checkpoint](evidence/roadmap-2026-10-22/handy-play-intents/checkpoint.json) records actual
private SQLite tests for duplicate claims, exact cancellation, immutable outcomes, transaction
rollback, metadata retry, restart fencing, parent corruption, legacy-effect refusal and v4
migration. It also records the isolated native binary campaign. Histories in that campaign are
explicit synthetic fixtures; they do not establish a real device operation or acceptance.

The final scope passes 32 journal tests, 66 playback-runtime tests and 170 Handy Engine tests,
with no failures or ignored cases. Both strict workspace/all-target lint configurations,
App/Worker builds, formatting and architecture/source guards pass. The original six restart
failures from the setup-reader version ceiling and two integer-bound lint findings are retained
with their repairs. The integer comparison retains the same 53-bit limit.

The actual preceding v4 binary refuses v5. Current-version restarts preserve Prepared, Claimed,
Acknowledged, Unknown and Cancelled synthetic histories; a missing linked effect refuses startup.
Database and existing WAL bytes remain unchanged on refusal, including a case with explicitly
held nonempty WAL files. SQLite may create an empty WAL file when it was absent, so whole-directory
or sidecar-presence identity is not claimed. The initial harness's unclosed-connection checkpoint
and subsequent strict sidecar-presence failure are retained separately from the final results.

Native Engine submission, its main-request reservation and witness, current authorization under
the shared DB guard, queue/status/recovery commands and shared play/stop execution remain open.
The existing stop owner is unchanged. Browser correspondence, emergency stopping and real
device/model/media/platform/storage/human/release qualification also remain open. Full roadmap
PARTIAL; physical playback remains disabled.
