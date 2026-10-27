# Native clock origin verification and read admission

Date: 2026-10-09
Status: native origin loading integrated; mixed-clock composition/publication pending

## Production origin boundary

Native Stash publication now uses one Authority-owned loader for its originating generation metadata. It checks bounded typed job metadata against the selected project/source/job and the immutable source catalogue's hash/length. It follows only the existing bounded same-job candidate-rebase links, reads bounded retained lineage and motion descriptors, and verifies the actual stored original motion bytes. The existing receipt verifier binds the producer attempt, base revision, configuration, placement and raw/canonical program identities. Original sample-plan arithmetic is checked when constructing the existing source-clock carrier; this is not independent real-media frame/quality qualification.

The loader retains memory reservations while its manifest and parsed original clocks remain live. Raw-program verification has its own reservation held by the opened reader until verification completes. Receipt and canonical-motion admission uses checked byte arithmetic; low-memory refusal occurs before those buffers are populated. SQL metadata bounds are 1 MiB for manifest/lineage and 16 KiB for a motion descriptor. The existing receipt ceiling and worker output budget remain unchanged.

This loader is used by the current single-clock native publication path. It does not publish a new wire command, grant authority, alter the master, admit a physical device, enable differing-clock publication, or replace archived-origin verification. It supplies the native-origin building block for the [selected-clock arithmetic](PULSAR_SELECTED_SOURCE_CLOCKS.md).

## Reproduced read-bound defects

The production receipt/program verifier accepted an observed byte count as an eventual identity check but allowed reading up to a larger general output ceiling. Authored readers with trailing whitespace exposed the consequence:

- A 10,313-byte observed receipt consumed 75,849 bytes before refusal.
- A 558-byte admitted raw program consumed 66,094 bytes before refusal.

These are deterministic reader-growth counterexamples exercising the actual verifier, not filesystem race, power-loss or platform qualification. Both are now bounded by their observed/declared byte counts. A buffered underlying reader may prefetch up to its fixed 8 KiB buffer; it cannot stream the larger general budget after the declared end. Growth still refuses rather than being silently accepted or clipped into a successful receipt.

The native authored-media regression also verifies original candidate/program/receipt identity, exact sample-plan retention, source catalogue mismatch refusal, oversized manifest/lineage/descriptor refusal, low-memory admission and replaced/corrupted CAS motion refusal followed by restoration. The corruption fixture replaces only its owned test object and restores the original read-only object; it does not make immutable files writable.

## Verification boundaries

The [source-bound evidence checkpoint](evidence/roadmap-2026-10-09/native-clock-origins/checkpoint.json) records failing counterexamples, intermediate compilation/setup failures, corrected direct default/CUDA builds, release identity and native command outcomes. No intermediate failure is relabelled as a pass. Default/CUDA suites remain scoped direct Engine checks rather than a full workspace/OS/vendor release matrix. The final executable is separately checked through public Stash workflows.

Engine composition still needs to derive every selected action's clock from effective revision, merge-boundary, edit, replacement, rebase, restoration and retiming facts, then retain a versioned durable proof through replay/restart and portable export. The known mixed-clock source collision must remain a refusal. Useful models, real-media evaluation, measured performance, platform coverage, Handy and human acceptance remain separate open gates.


## Native contributor receipt verification — 2026-10-09

[Contributor origin verification](PULSAR_CONTRIBUTOR_CLOCK_VERIFICATION.md) closes an actual CLI defect: the predecessor published an equal-clock merged script despite a corrupted contributor receipt. The bounded ancestry walk now verifies each contributing native origin and sample-plan arithmetic. Corrupt/missing receipts refuse before sibling creation; restoration permits valid publication. Default/CUDA Engine suites, offline release, architecture guards and both native CLI regressions pass. Per-action clock derivation and durable mixed-clock proofs remain pending; roadmap status remains PARTIAL.
