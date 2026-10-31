# Browser/live fallback implementation

The execution queue remains in [PULSAR_EXECUTION.json](PULSAR_EXECUTION.json).
This document explains the remaining implementation under
`LIVE-FALLBACK-HISTORY`; it does not establish acceptance or replace the
[software specification](PULSAR_SOFTWARE_SPECIFICATION.md).

File fallback currently uses `InferenceFallbackSelection`, a decoded-media
`WorkerRequest`, current accepted equivalence records, held environment
observations and an original settled worker failure. Browser/live inference
instead uses `LiveInferenceManifest`, a capture epoch, presentation clock,
browser media/timing ownership and a native worker lifetime. The source policy
for file jobs retains its decoded-media kind and original canonical bytes.
Browser source applicability now has a separate
`BrowserInferenceFallbackSourcePolicy` type, with an explicit
`browser_media_capture` namespace and a distinct canonical identity. The two
policy types refuse cross-deserialization. Copying a file selection into a live
report still cannot establish live applicability or permission to switch.

The browser metadata matcher requires bound media and analysis declarations,
source-media presentation time, exact boot/project/revision/source and
presentation-clock correspondence, the selected profile/model path, the
original declared capture lease and any explicit content allowlist. Live
manifest construction, captured CPU admission rechecks and live CUDA startup
consume this check. Ordinary capture modes retain their existing path.
These checks do not establish acquisition, current native media/timing
ownership or accepted equivalence. No automatic backend-selection option is
exposed by the namespace and correspondence work.

Browser view applicability has a separate canonical policy identity. Its
semantic projection starts with the complete typed capture declaration and
removes only execution-profile selection and per-epoch coordinates: capture
anchors, continuation mode, declared lease lengths, media preparation/lease
IDs and analysis feed/timing IDs. Model content and pack choice remain bound,
along with boot/project/revision, source content and placement, declared source
clock, player identity, lookahead, maximum analysis age and buffering. Advancing
capture anchors must still follow the same validated source/project mapping.
New fields remain in the projection unless explicitly classified as transient.
The projection adds no inferred quality tier, target, view or device permission.

The actual native successor reservation retains its original semantic identity.
Preparation checks the derived successor against it; later consumption checks
both epochs against that original value, including when both declarations have
changed identically. Existing source/timing pins, profile equality, cancellation,
worker join and original deadlines remain required. This held correspondence
does not approve a backend switch or create fallback failure authority.

Current browser succession preserves the predecessor's exact profile and
source semantics. Its shared-frame transfer proves a particular accepted
boundary and a joined predecessor; a failed worker cannot manufacture that
graceful boundary. Automatic alternate-backend selection is not implemented
by `PrepareBrowserAnalysisSuccessor`.

Initial fallback selection now declares its tier, axis and ordered equivalence
references before worker startup. Engine preparation holds the original native
source and timing pins and observes the selected execution environment outside
authority locks. Publication and later consumption recheck the current accepted
record, exact source/view/profile correspondence and original environment
lifetime. The admission report is inert metadata; it cannot restore those held
objects. Retained selection/admission conditionally requires reader42.

CUDA initialization now has a distinct `ProviderUnavailable` stream reply. It
can originate only from the typed negative provider-list query after driver
checks and pinned runtime initialization. The worker rechecks its native inputs
and inherited deadline before sending it. The parent binds the reply to the
complete original manifest, requires clean EOF and the expected failed child
exit, then confirms process-tree, staging, watchdog, fence and resource
retirement before creating a private settled failure. Cancellation, expiry,
trailing output, a different exit or unconfirmed retirement cannot create that
object. Generic error text and serialized metadata cannot substitute for it.
The CPU API always reports its provider available, so ordinary CPU errors are
not reclassified as provider absence. The Engine startup result preserves the
private CUDA failure separately from host errors.

The Engine coordinator now retains the original native media/timing pins before
retiring the failed epoch. A process-local lifetime carries the original deadline
and cancellation across automatic targets. Stopping a retired ancestor signals
every registered target worker; registration after cancellation cannot escape
that signal. Preparing a target reserves a capture slot, and live descendants
keep their ancestor cancellation handles from being evicted. Ordinary explicit
succession remains a separate operation.

Target preparation rechecks the declared preference order against current
accepted directed records, the original profile/environment and the same source
semantics. It observes the target environment outside authority locks, then
publishes through the normal capture/worker consumers with a fresh attempt,
epoch and browser feed. The selected pack/model and original deadline remain
bound. Failed pixels and graceful shared-frame evidence are never transferred.
Provider absence must precede Ready and have no outstanding delivery or transfer
custody. Target admission retains the failed manifest, typed observation and
selected-reference chain as inert history. Status links expose preparing,
published-target and refusal outcomes without creating execution authority.

Recordings now retain their original capture options, fallback admission and
failure/transition history. A conditional reader42 admission index binds them to
the exact original actor, request, project/revision, attempt and canonical receipt
digest. Startup and candidate preparation check that correspondence; removing
fallback fields cannot silently turn a selected recording into legacy history.
Page assembly preserves complete headers and permits a profile change only
through a matching recorded fallback chain. A failed pre-Ready epoch contributes
no motion, and each new epoch still starts with a gap rather than fabricated
frame continuity. Candidate source snapshots keep the complete recording pages;
project packages carry those bytes through their existing SourceSnapshot role.

These paths remain under verification. Component recording/index/assembly checks
pass, but the expanded native campaign has not produced a recording/candidate
pass. It exposed a deadline mismatch: captured CPU admission still expected the
uncapped lease after the coordinator began supplying the held original cap.
Admission now checks that exact same held cap. Subsequent runs retained a timeout
and lost original timing ownership under observed filesystem write/journal
waits. No native limit, durability setting or unrelated process was changed.
The first-party bridge and browser now follow recorded target links and wait
for Running before opening a new feed. They preserve the original local deadline,
player/media/timing membership and playback grouping, clear prior observations,
and discard callbacks already captured during startup or switching. Recording
begins before the fresh target's first upload. A lost acknowledgement retains its
original intent and stops further automatic replacement. These paths have
component coverage; rendered/native handover, complete portable evidence-graph
correspondence and the alternate-execution workflow remain unverified.

The actual SessionClient and bulk-channel handshake now have framed peer tests
over authored Engine reports. They check recording-before-bulk order, preserved
deadline/playback membership, cleared per-feed state, waiting without another
Start, expired/timing-mismatched refusal, and lost recording/bulk acknowledgements.
Stop acknowledgements must name the exact original identity, boot and clock;
an unrelated inactive reply leaves the outcome unresolved and replacement blocked.
These tests exercise the client transport and control path without running a
model, native Engine or rendered browser.

Live admission now derives the CPU thread reservation from the explicitly
selected profile. The former two-thread constant rejected valid one-thread
profiles at manifest validation and larger profiles at budget admission. The
same checked budget still preserves the lease and output class, grows RAM only
to the declared profile floor, and enforces the owner RAM ceiling. The native
browser campaign can select an explicit test thread count; its clean-input and
prepared-successor cases check the actual captured worker path. This change
does not select an equivalent backend or qualify model quality or hardware.
Prepared succession briefly owns both workers. Two four-thread workers need
an explicitly configured eight-slot owner pool; the ordinary four-slot limit
must continue to refuse that overlap. Backend selection must account for
available owner resources rather than silently increasing that limit.

First-party browser succession now waits for the predecessor's actual worker
join and settled delivery before opening the successor writer or capturing its
first pair frame. Preparation and boundary reservation alone are insufficient.
Callbacks received while waiting are not captured or retained for later use;
the next eligible callback supplies the new frame and its original timestamp.
This removes the race where a fresh successor estimate aged out while waiting
for predecessor retirement. The capture-age, pairing-gap and lease limits remain
unchanged, and an unknown upload still consumes its one-use send latch.

Native stamp refusals report the source role, sealed state, stamped/current
continuity generations, upper age bound, limit and sampled clock bounds. These
are diagnostics from the same already-sampled refusal predicate; they do not
sample a new clock or renew any timestamp. Generation mismatches still refuse
for sealed history; the existing sealed-history age rule is unchanged. A native
test can select one existing case with `PULSAR_CAPTURED_CPU_BROWSER_CASE` and
record bounded transfer phase observations. Its controlled producer wait is a
software scenario, not physical timing or latency qualification.

The next implementation must cross these interfaces in order:

1. Verify browser/live source and semantic correspondence through the native
   consumers. Distinct source/view namespaces, manifest correspondence and the
   native successor's retained semantic identity are implemented. Fallback
   admission must still retain its own actual source and timing ownership
   through failed-worker retirement. The admitted source must come from retained `BrowserMediaBinding`,
   `BrowserAnalysisCaptureDeclaration` and current native timing/source
   ownership. Bind the project/revision, source placement/view, capture epoch,
   presentation mapping and original lease. File admission must continue to
   require decoded-media policy; neither namespace may impersonate the other.
   Public declarations alone cannot construct a held source proof.
2. Resolve the selected directed equivalence against current accepted records
   and freshly observed actual profile environments. Recheck exact source/view
   applicability, decision revision, evidence identities, permissions,
   withdrawal and expiry when publishing and consuming the live admission.
   A serialized report or archived decision grants no current authority.
   The fallback selection requires an explicit tier and axis rather than
   inferring them from a model name or backend. File-job evidence must not
   silently become live evidence.
3. Verify the source/delivery transition around the private settled worker
   failure. Only the original
   native worker's eligible provider-unavailable result, after confirmed
   retirement of worker/resource/delivery custody, may permit an automatic
   target. Cancellation, lost source/timing ownership, stale input, malformed
   output, denied resources and expired authority must not become substitutes
   for that result. Recheck the original lifetime before fresh target resource
   admission; never renew it as a side effect of switching.
   The CUDA startup result now distinguishes the original settled negative
   provider query from host/transport errors. Ordinary CPU
   `Error(ProtocolError)` remains ineligible. Do not recover eligibility from
   any error code or message. The coordinator consumes the original private
   object and retains native source pins across old-epoch retirement. Exercise
   current-record withdrawal, original-deadline, root-cancel, capacity, failed
   publication and unknown-outcome races through that actual path.
4. Verify target admission through the actual live CPU/CUDA consumers and use a fresh
   attempt and epoch. Do not reuse incompatible analysed samples, estimates or
   a fabricated shared-frame completion. Preserve the actual source clock and
   disclose the selected profile, switch reason and limitations through shared
   clients. Native dispatch must retire the original delivery and stop safely
   when continuity cannot be established.
5. Exercise ordinary public selection/refusal first, then real confined
   execution, withdrawal/expiry/cancellation races, source replacement, failed
   join, resource pressure, restart and inert history. Keep authored eligibility
   and neutral-model evidence separate from measured equivalence, rendered
   acquisition, useful script quality and physical Handy qualification.

The exact implementation may reuse the existing environment observer,
equivalence resolver and native retirement machinery. It must not weaken their
proofs to make file and live declarations appear interchangeable. Remaining
portable-history corruption/resource/fault and hot-WAL checks also stay open in
the execution dashboard.


The portable browser graph implementation is under verification in
[PULSAR_PORTABLE_FALLBACK_HISTORY.md](PULSAR_PORTABLE_FALLBACK_HISTORY.md).
Format 17 binds recording snapshots to original admission acknowledgements and
historical equivalence documents; reader 43 protects retained graph metadata.
Public Engine restart/interchange, actual native target execution, resource/fault
and preceding-reader proof remain open until their retained campaigns pass.
