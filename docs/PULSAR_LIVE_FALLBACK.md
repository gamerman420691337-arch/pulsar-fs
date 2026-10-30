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
   `LiveCaptureOptions` currently declares no quality tier; the fallback
   selection must require an explicit tier rather than infer one from a model
   name or backend. File-job evidence must not silently become live evidence.
3. Add a live-specific settled-failure and retirement path. Only the original
   native worker's eligible provider-unavailable result, after confirmed
   retirement of worker/resource/delivery custody, may permit an automatic
   target. Cancellation, lost source/timing ownership, stale input, malformed
   output, denied resources and expired authority must not become substitutes
   for that result. Recheck the original lifetime before fresh target resource
   admission; never renew it as a side effect of switching.
   The live reply currently carries `Error(ProtocolError)`, which is not an
   eligible native failure witness. Do not recover eligibility from its error
   code or message; preserve typed provider-absence evidence through actual
   worker and resource settlement.
4. Admit the target through the actual live CPU/CUDA consumers and use a fresh
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
