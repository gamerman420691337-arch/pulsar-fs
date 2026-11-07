# Fill missing channels in existing local audio bundles

Shared retained audio intake now inspects all six recognized channels in an
existing `<media filename>.pulsar-audio-bundle` directory. Adjacent stroke and
present regular bundle files remain protected, including empty or unvalidated
files. Only absent channels enter the captured publication selection. Unknown
members and an existing `.pulsar-bundle.json` are preserved. A complete set is
excluded from new intake; this means file presence, never useful-motion validity.

Symlink/non-file channels within an eligible directory and metadata errors
refuse intake. Conflicting symlink/non-directory bundle roots keep the previous
whole-entry exclusion rule and remain visible as blockers in companion inventory. Directory device/inode identity is captured alongside the immutable media,
recipe and selected axes. The Engine rechecks that identity before recording a new
batch. Generation still produces a full six-axis audio candidate; publication
selects the missing output channels independently. No prior script is imported,
truncated, rewritten or deleted.

Existing-directory batches use generation/publication schema 6 and reader floor
34. Mixed batches can contain existing-directory fills and ordinary new bundles.
Cancellation, migration and immutable journal reconciliation preserve the selected
axes and existing-directory identities; older admission helpers cannot downgrade
the reader floor. Fresh-bundle semantics and omitted-field legacy bytes remain.

For an existing directory, the private client journal retains a distinct exact
request ID for every selected channel before dispatch. Each scheduler step
attempts at most one unfinished channel. Reports show the axis, original request
ID, retained positive acknowledgement and bounded error. Earlier completed
channels survive later failure. `publication_completed` records acknowledgements;
it is not a fresh filesystem observation or quality claim.

`ExportAxisPinned` carries project/revision, output path and captured directory
identity through the existing authorized export reservation/replay path. Its
preflight refuses a stale directory or existing target before claiming an export
effect. Publication holds a no-follow directory descriptor, writes/syncs an
anonymous `O_TMPFILE` inode, and links that exact open inode into the held
directory without replacement. It verifies the directory pathname's identity
around publication and syncs the held directory. An unexpected later failure
keeps the existing export's unknown-outcome protection; it never invents another
request ID or treats matching bytes as a positive receipt.

This is **per-file atomic publication**, with durable per-channel recovery. It is
not an aggregate atomic merge of an existing bundle. The original bundle receipt
stays unchanged; new positive channel acknowledgements live in the retained
client journal. There is no aggregate `PublishedNeutralBundle` receipt for this
mode. A replaced directory remains a conflict; restoring the original identity
can retry a request whose preflight proved that no effect was claimed. Rebinding
to a different directory requires a separate explicit workflow, not an implicit
retry.

Linux/Android support requires `O_TMPFILE`, the current process's proc-fd links,
and filesystem hard-link/directory-sync support. Unsupported filesystems or
platforms fail closed. The held descriptor prevents publication into a replacement
directory, but this does not establish a hostile-host boundary or immutable files
after acknowledgement. External owners can subsequently move or modify outputs.

The scoped campaign uses authored PCM and untouched sentinel scripts, exercises
actual CLI/desktop controller IPC, injects loss by restoring exact prior journal
bytes or removing/restoring an actual positive cache row in an owned retired
fixture database, and retains resulting refusals/replays. It does not claim real
client death, rendered GUI, useful media/model quality, physical Handy or human
acceptance. Full roadmap remains PARTIAL.

See [ADR B3-181](../research/phase-b/ADR-B3-181-library-partial-bundles.md) and
[checkpoint](evidence/roadmap-2026-10-12/library-partial-bundles/checkpoint.json).

The later [actual client-death campaign](PULSAR_LIBRARY_PARTIAL_CLIENT_DEATH.md) supplies separate SIGKILL evidence on both sides of the acknowledgement record rename. The injected-loss scope and limitations above describe the original checkpoint. Broader storage/receipt/orphan disposition and all independent qualification gates remain open.
