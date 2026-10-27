# Actual client-death recovery for existing audio-bundle channels

The [partial-bundle checkpoint](PULSAR_LIBRARY_PARTIAL_BUNDLES.md) originally
injected journal/receipt loss by restoring exact retained state. This later
campaign exercises actual SIGKILL at two local-journal publication boundaries
through the production CLI, Engine and per-channel pinned export path.
No production source or binary change is made.

An uninjected control traces only renameat/renameat2 and calibrates the one
`.pending-*` to `record.json` rename after the first channel's positive Engine
acknowledgement. The fixture then delays that exact syscall entry in an owned
client. Before death, the first Sway output exists, the Engine retains the original
positive request row, and the original record still names all channel intents as
unacknowledged. The genuine fully written pending journal contains the new
acknowledgement but has not replaced the original record. The observed traced
client PID is checked against the exact application executable and fixture
namespace before SIGKILL; the tracer is waited and confirms the child death.
No journal bytes or request/receipt rows are manually rolled back, deleted,
restored, or fabricated.

After restarting the owned Engine, one workflow step reuses the original Sway
request ID and obtains its existing positive response. Export count and Sway
bytes remain unchanged; the journal now records the acknowledgement. Remaining
channels complete normally, and a completed repeat creates no new effects.
The genuine unrenamed pending file remains byte-for-byte protected. The service
does not automatically adopt or delete it; cached Engine acknowledgement supplies
recovery authority, not matching file presence.

A separate fresh fixture delays the same rename at syscall exit. The new original
record visibly contains the Sway acknowledgement before death; the pending name
is gone, and directory-sync completion has not been acknowledged. After SIGKILL
and Engine restart, one step preserves that original Sway ID/acknowledgement and
publishes Surge, the next channel. It adds one expected export, with no duplicate
Sway reservation or rewrite. The remainder completes with four exports for one
job. This is process-interruption evidence on private RAM, not a power-loss or
durable-media guarantee.

Across the uninjected control and two interrupted inputs, 12 outputs match the
independent authored PCM oracle: 101 original timestamps from 0 through 1000 ms,
45 before 500 ms and 55 from 500 ms. Adjacent Stroke, existing Roll and unknown
owner bytes remain untouched. Final aggregate counts across the two independent
fixture namespaces are three projects/jobs/candidates and 12 exports. Two actual
clients are killed with their tracers joined, and four owned Engines are joined.
The after-rename fixture directly queries reader floor 34. The first script's
emitted floor was source-derived, so its audited result excludes that unqueried
field; no independent first-fixture floor observation is asserted.

All 537 production source identities and App/Worker/controller binary hashes
match the preceding [unresolved-flow diagnostic checkpoint](PULSAR_UNRESOLVED_FLOW_DIAGNOSTICS.md).
Its passing 406 Client tests (ten ignored), strict lint/build/platform/format and
architecture/source-guard checks retain their exact source scope. They are not
fresh suites in this campaign. The syscall traces contain only rename metadata,
not IPC payloads, auth headers or secret values. Fixture credentials, private
Engine database and journals live in an owned private RAM namespace; only public
authored outputs and bounded diagnostics are retained.

Broader interruptions, actual storage failure/power loss, orphan disposition,
missing-positive outcomes, other filesystems/platforms, rendered GUI, useful
media/model quality, physical Handy, human and release acceptance remain open.
Earlier missing-positive refusal evidence retains its own original checkpoint;
these two death probes require the genuine cached positive response and do not
extend that evidence to receipt loss after arbitrary corruption.
Full roadmap PARTIAL.

See [checkpoint](evidence/roadmap-2026-10-12/library-partial-client-death/checkpoint.json).


## Subsequent explicit metadata retirement

[The next software checkpoint](PULSAR_LIBRARY_PENDING_RETIREMENT.md) adds explicit
byte-preserving retirement of an inspected pending journal after current positive
acknowledgements cover its exact history. It exercises a newly created genuine
CLI death orphan and retains its bytes under a deterministic name. The original
campaign above remains unchanged; no automatic deletion, file adoption or general
unknown-effect disposition follows.
