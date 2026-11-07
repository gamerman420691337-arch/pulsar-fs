# Portable fallback history

The portable fallback work is in progress under `LIVE-FALLBACK-HISTORY` in
[the execution dashboard](PULSAR_MILESTONES.md). Format-16 payloads now pass the root/ancestor checker before export or clone
publication. This document distinguishes implemented metadata interchange from
remaining native, model, platform, media, physical and human qualification.

The snapshot-owned payload builder now retains original equivalence decisions
and all eight raw evidence documents from the same database transaction as the
attempt chain. It follows the original decision/request link and canonical
receipt, including when the current record is withdrawn or expired. It validates
the declared evidence at the original decision and admission times; it does not
read current acceptance as permission or advance the current clock floor.
Independent truth of the measurements, review and approval remains outside
structural archive validation.

Each payload starts with the eight-byte marker `PULSFH1\0`, a four-byte
little-endian header length, canonical schema-1 JSON and raw evidence bodies in
digest order. The JSON retains the job state, inert pending-intent observation,
history and original reports. The header is bounded to 32 MiB, distinct evidence
bodies to the registry's 128 MiB aggregate limit and the whole payload to
160 MiB; the inner history retains its separate 8 MiB bound. Missing, extra,
changed, truncated and trailing bytes refuse. Pending observations bind the
original predecessor, engine epoch and exact failed outcome; they cannot become
a new runtime intent or lease. Admission checks precede allocating the output
frame, under the capture operation's existing metadata reservation.

Local capture attaches one payload object to every original attempt of the job.
The node checker binds the payload to the exact original project/job namespace,
attempt set, states, source kind and canonical worker bytes, and checks the
original profile/environment/directed transition against the referenced report.
Filesystem paths inside evidence stay inert. A bounded graph index checks current
and ancestor nodes, inherited candidate policy identities, every opaque worker
manifest and every payload-role object before publication. Missing, stripped,
detached, cross-job and contradictory declarations refuse. CPU-image checks
remain independent and retain their original declaration-refusal precedence.

The interchange uses package format 16, the
`inference_fallback_history` object role and an optional bounded history-object
reference on each original generated attempt. Omitted references preserve the
older canonical bytes. A history reference, its object role or a format-16
ancestor requires format 16; the ordinary object catalog checks exact lengths
and roles. A consumed `assistant_paused` state requires this explicit history
declaration. Structural declarations alone never admit a graph: full payload bytes, original
worker correspondence and root/ancestor closure must also pass.

Conditional reader 41 distinguishes admitted fallback graphs from the earlier
draft reader-40 declaration. It wraps reader 40, which wraps portable CPU reader
39. Underlying publication preserves every marker, and arming is transactional with
the metadata requiring it. Startup checks the canonical marker, scalar bounds,
associated objects and temporary shadows before recovery. Format-16 current or
ancestor declarations, a history field or its object role require reader 41;
removing the marker does not turn the metadata into ordinary history. Export
and import publication arm this floor in the same transaction as their retained
metadata. The barriers themselves grant no authority or qualification.

Public capture additionally requires the initiating session's project-specific
host-owner packaging approval and its retained canonical acknowledgement. A
delegated `PackageProject` grant alone does not disclose host-owner registry
evidence. Capture includes only exact decisions referenced by the project,
never registry enumeration or management rights. The shared CLI/desktop privacy
notice explicitly includes model evaluation, decision and review records.

## Browser recording graph (format-17 implementation under verification)

A live recording source has its own optional `browser_fallback_history` object
reference and the `browser_inference_fallback_history` role. A declaration, role,
or format-17 ancestor requires format 17; omitted fields retain legacy canonical
bytes. The object binds the exact recording snapshot hash/length, its original
source version and project namespace, the original canonical admission
acknowledgements, referenced equivalence reports and all their raw documents.
This includes the original admission of every failed epoch, even when the
recording contains only the eventual target. Missing, detached or substituted
acknowledgements and directed profile/environment mismatches refuse.

The browser frame starts with `PULSBH1\0`, then a little-endian four-byte header
length, canonical schema-1 JSON and raw document bytes in digest order. The
header is limited to 32 MiB, 1,024 admission acknowledgements and 256 referenced
reports. Raw documents retain the 128 MiB aggregate bound and the full frame the
160 MiB bound. Original recording snapshots retain their separate 40 MiB bound;
local preflight admits at most 64 MiB of recording bytes, and graph verification
charges at most 256 MiB across distinct source/payload bindings before reading.
These are engineering admission limits, not product capacity guarantees.

Capture first authorizes and catalogues the sources, then reads and hashes their
bytes outside the authority transaction. The actual capture transaction rechecks
each exact source tuple and the initiating session's project-specific host-owner
disclosure acknowledgement. Original admission and equivalence records are
collected from that same SQL snapshot; files are never read under that lock.
The plan holds the already checked source bytes. Imported recordings reuse their
checked ancestor evidence, without looking up acceptance in the destination.

Both export materialization and clone import check the entire root/ancestor
graph, original source namespace, inherited source reference, exact recording
snapshot bytes, original media-source identity and placement definition,
acknowledgement scope, decision times, directed profiles/environments,
all eight evidence documents and detached role objects. Original reports remain
historical data; the importer never installs accepted decisions, worker jobs,
native source handles or permissions from this payload.

Conditional reader 43 wraps reader 42 and retains older marker composition.
Publication arms it in the same transaction as format-17 export/import metadata.
Startup refuses stripped markers, wrong scalars, temporary shadows and retained
format-17 declarations without their barrier before recovery. Actual previous-
reader main/WAL refusal, public Engine export/clone/restart/re-export/second clone,
resource and crash/fault verification remain required. Compilation or authored
codec fixtures do not close those obligations.

The authored Engine campaign reaches public export readiness, uploads/imports
the package, restarts the destination, re-exports and imports a second clone.
Payload bytes survive both clones, and no executable job/attempt or acceptance
registry record is installed. Delegating the package scope without the new
session's own host-owner approval refuses disclosure. Root and ancestor state
mutations refuse before project publication even after rebuilding their hashes
and container identities. A pinned preceding App refuses both cold checkpointed
reader-41 clones with observed inventory, modes and contents unchanged. This is
metadata/compatibility evidence, not confined inference, measured equivalence,
useful script quality, broad hot-WAL or physical qualification.

The separate ignored native test
`native_cpu_fallback_portable_history_roundtrip` extends this consumer to real
confined CPU successors. Completed, cancelled-after-progress and
withdrawn-after-progress cases pass public export, two clone imports, restart
and re-export. Both original CPU-image declarations remain byte-identical in
the ancestor manifests, the fallback payload remains checked and inert, and
root/ancestor state mutations refuse before publication. Imported databases
contain no executable jobs/attempts, CPU execution rows or equivalence
acceptance records. Each case joins its workers and removes its private
database before the host supervisor removes its scratch directory.

Eligibility and primary provider absence in this campaign remain explicitly
simulated. The exported evaluation/decision records are authored software-test
shapes, not genuine measurements or human approvals. The actual neutral-model
CPU execution proves software custody and archival behavior only. This native
campaign now also captures a real successor while its confined CPU group is
held after at least 16 analysed samples. The original 60-second attempt budget
and accepted-record lifetime remain unchanged. The running archive completes
its clone checks before that admission expires. A parent pins the original
Engine and worker process identities, kills the separate Engine process with
`SIGKILL`, thaws only the held group and reopens the same database through normal
recovery. The original target becomes interrupted; no successor, candidate or
pending intent is created. The interrupted archive then passes the same two
clone/restart/re-export consumer. No database state is rewritten by the test to
manufacture running or interrupted history.

The child passes its session credential through an anonymous socket in memory;
neither the rendezvous nor the credential is written to the campaign receipt.
The socket is close-on-exec before generation starts, and does not pass to
workers or preceding readers. Failed runs leave child storage for the host
supervisor to remove only after joining descendants.

All five native archive lifecycles (completed, cancelled, withdrawn, running
and interrupted) now repeat the cold reader-41 check on both clones: ten
preceding-App refusals preserve observed file inventory, modes and contents.
The campaign confirms original process exit through held pidfds, retirement
of execution custody and removal of its private databases and scratch.
Remaining corruption/resource/fault cases, broad hot-WAL behavior and
browser/live fallback still require evidence. Scientific, real-media,
platform, physical and human acceptance remain separate open gates.

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

1. Complete public capture qualification of the checked payload alongside original worker manifests within the
   same project snapshot. Account for aggregate metadata and retained object
   bytes before assembly; keep filesystem reads outside the authority lock.
   The transaction-owned builder and local-node checker are implemented and
   exercised with authored database/record shapes; this is not native execution
   or model qualification.
2. Extend the implemented format-16 payload/graph campaigns across the remaining
   lifecycle and fault scenarios. Preserve the selected policy, original environments, outcomes, manifests,
   model/runtime declarations and referenced evidence through root and ancestor
   graphs. Reject omitted, substituted, duplicated, detached and contradictory
   current or ancestral declarations, including rehashed mutations.
3. Qualify the implemented full-graph import checks across broader faults and
   actual confined-worker histories. Clone maps
   must preserve original historical identities without installing origin
   acceptance decisions, grants, owner policies or executable attempts. Keep
   active local acceptance and imported historical evidence distinct.
   The project-specific host-owner approval check, delegated-scope denial and
   privacy notice are implemented. Registry enumeration, management and current
   acceptance remain separate capabilities.
4. Extend reader-41 qualification beyond the cold checkpointed metadata campaign
   through actual confined format-16 publication, import
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
