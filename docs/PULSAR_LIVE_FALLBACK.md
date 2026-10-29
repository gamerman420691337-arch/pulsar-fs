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
currently has only a decoded-media kind. Copying a file selection into a live
report would not establish live applicability or permission to switch.

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

The next implementation must cross these interfaces in order:

1. Introduce an explicit browser/live source policy and semantic correspondence
   check. The admitted source must come from retained `BrowserMediaBinding`,
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
3. Add a live-specific settled-failure and retirement path. Only the original
   native worker's eligible provider-unavailable result, after confirmed
   retirement of worker/resource/delivery custody, may permit an automatic
   target. Cancellation, lost source/timing ownership, stale input, malformed
   output, denied resources and expired authority must not become substitutes
   for that result. Recheck the original lifetime before fresh target resource
   admission; never renew it as a side effect of switching.
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
