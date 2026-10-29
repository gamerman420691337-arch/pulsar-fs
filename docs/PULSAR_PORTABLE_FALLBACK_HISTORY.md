# Portable fallback history

The portable fallback work is in progress under `LIVE-FALLBACK-HISTORY` in
[the execution dashboard](PULSAR_MILESTONES.md). Existing project-package
refusals remain enabled. This document describes the implemented shared
metadata contract and the remaining integration requirements; it is not a
claim that fallback projects can already be exported or cloned.

The next integration layer declares package format 16, the
`inference_fallback_history` object role and an optional bounded history-object
reference on each original generated attempt. Omitted references preserve the
older canonical bytes. A history reference, its object role or a format-16
ancestor requires format 16; the ordinary object catalog checks exact lengths
and roles. A consumed `assistant_paused` state requires this explicit history
declaration. These are structural declarations only: the Engine still refuses
format-16 graphs until the complete history payload and original worker-object
correspondence are implemented.

Conditional storage reader 40 wraps the existing portable CPU reader 39.
Underlying publication preserves both markers, and arming is transactional with
the metadata requiring it. Startup checks the canonical marker, scalar bounds,
associated objects and temporary shadows before recovery. Format-16 current or
ancestor declarations, a history field or its object role require reader 40;
removing the marker does not turn the metadata into ordinary history. Export
and import publication have the conditional arming calls, behind the retained
full-graph refusal. The barrier itself grants no authority or qualification.

An initial native probe found that the pinned preceding executable rejects
reader 40 without changing existing files, but creates an empty daemon lock
and SQLite WAL/SHM files on a cold startup. That failure remains retained;
it does not establish a filesystem-wide unchanged-state result for that
executable. Current startup adds a read-only main-file-header negative check
before daemon lock creation and SQLite opening. It refuses unsupported cold
main-file versions without those side effects. This check never admits a
schema: the normal SQLite and full reader validation still inspect committed
WAL state. A newer version present only in a WAL remains refused; complete
hot-WAL startup side-effect qualification is still open. The header layout is
defined by [SQLite's database format](https://www.sqlite.org/fileformat.html#the_database_header).

`InferenceFallbackHistory` retains one original job's selected policy and its
complete predecessor-ordered attempt chain. Each attempt contains its original
manifest and profile hashes, state, admission trace, observed environment and
optional worker outcome. Missing outcomes remain unknown. A transition to an
equivalent backend requires the original failed predecessor's recorded
provider-unavailable settlement. Consumed legacy cancelled resume predecessors
remain readable as inert history; they do not become eligible for a new resume.

The shared Protocol validator checks the original project, job, revision and
source namespace, unique membership, initial/current endpoints, predecessor
order, original admission clocks, ordinal progression and profile cycles. It
checks exact selected references and environment identities. With original
worker manifests supplied by the caller, it also checks canonical manifest and
profile digests, policy tags, worker/runtime correspondence and preserved
generation semantics. Validation opens no paths itself. The Engine supplies
only rows from the same checked SQLite transaction as the historical read.

The canonical encoding has schema 1, an explicit `history_only` declaration,
at most 128 attempts, at most 128 KiB per entry and at most 8 MiB in total.
Raw-byte parsing applies the aggregate bound before JSON parsing, rejects
unknown fields, and requires the exact canonical encoding. The existing compact
trace bound of 512 KiB also remains in force for local history reads. These are
metadata admission limits, not an application's total memory reservation.

Neither these checks nor a serialized environment prove that an equivalence
decision is currently accepted or that a worker, grant, model or device is
currently available. The archive contains no native execution or settlement
capability, cancellation token, current acceptance state, secret or renewed
lease. Recorded filesystem paths remain historical values. Captured queued or
running states do not grant imported work permission to execute.

## Remaining integration and exit evidence

1. Capture the checked archive alongside original worker manifests within the
   same project snapshot. Account for aggregate metadata and retained object
   bytes before assembly; keep filesystem reads outside the authority lock.
2. Connect the format-16 declaration and object role to the checked history
   payload. Preserve the selected policy, original environments, outcomes, manifests,
   model/runtime declarations and referenced evidence through root and ancestor
   graphs. Reject omitted, substituted, duplicated, detached and contradictory
   current or ancestral declarations, including rehashed mutations.
3. Validate the complete imported object graph before installation. Clone maps
   must preserve original historical identities without installing origin
   acceptance decisions, grants, owner policies or executable attempts. Keep
   active local acceptance and imported historical evidence distinct.
4. Qualify the reader-40 barrier through actual format-16 publication, import
   and recovery. An earlier executable must refuse before
   writable recovery or custody changes; record the exact database/WAL and
   filesystem observations rather than inferring them from an error message.
5. Run confined export, clone, restart, re-export and second-clone campaigns
   with completed, failed, cancelled, interrupted and captured-running attempts.
   Verify exact original correspondence, unknown observations, no restored
   authority, bounded admission, corruption refusals and actual resource release.
6. Connect live/browser fallback through its own admitted source and timing
   lifetime. Decoded-file policy does not admit a browser source. Withdrawal,
   expiry, interruption, replacement and restart need actual consumer evidence;
   serialized policy metadata cannot authorize a native successor.

Useful model quality, real-media validation, platform coverage, physical Handy
behavior and human acceptance remain separate gates. No step above closes them
through structural fixtures or package tests.
