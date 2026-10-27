# Source-time Stash publication after authored retiming

An explicitly retimed descendant of a placed generation can now publish a sibling script through the existing authenticated engine operation:

```text
pulsar stash publish-script PROJECT SCENE FILE --url URL \
  --source-version SOURCE --candidate ORIGIN_CANDIDATE --job ORIGIN_JOB \
  --revision CURRENT_REVISION --request-id RETAINED_REQUEST_ID --source-time
```

The desktop source-time checkbox expresses the same intent. Retimed stroke lineage requires this explicit option. Publication does not change the project master, revision, evidence labels or review disposition. It remains unqualified.

## Clock correspondence

The engine follows bounded authored source/job/candidate links, including effective undo/redo targets. Every encountered retime must have a structured receipt whose independent bindings, immutable before/after objects and complete transformation verify. Stroke retimes form an ordered chain; other-axis retimes do not change the stroke clock. Legacy retimes without receipts refuse rather than acquiring reconstructed intent.

A pure incremental mapper starts with the original verified placement and generation sample plan. For each retime it uses the actual historical before-track timestamps to reproduce the edit's quantized project mapping, carrying each original exact source clock to its resulting timestamp. This preserves fractional sample clocks that cannot be recovered by inverting rounded project nanoseconds. Original frame ordinal claims accompany only carried generation samples. Authored timestamps use the explicitly declared placement inverse and carry no frame claim. Current positions and evidence labels are exported unchanged.

The complete chain must share the source identity and each next from-placement specification must equal the preceding to-placement specification. Mapping refuses unresolved motion gaps, negative/out-of-trim clocks, overflow and timestamp collisions. Carrier memory is admitted from the maximum original-plan, current-track and historical before-track action counts and held through export; verified historical reads have additional per-step admission.

Stroke merges with different retiming chains and generation rebases onto a different clock history refuse. A source/job identity alone does not establish piecewise clock correspondence. A future piecewise proof is required to support such mixed histories. Non-stroke contributions retain their existing source-derivation behavior.

## Retained proof and observation

The private content-addressed timing proof uses schema 2 when it contains retiming receipts and schema 1 for unchanged origin clocks. It retains the original verified generation receipt/program identities, origin placement, complete ordered retiming receipts, current master identity, resulting script identity and per-action exact mappings. Project database schema remains 6. The subsequent [portable format-5 increment](PULSAR_PORTABLE_RETIMING_RECEIPTS.md) retains retiming facts; portable Stash source/proof integration remains open.

The proof reference and pending export are retained before descriptor-relative no-clobber publication. Existing authority, expected-revision, immutable capture association, replay and unknown-outcome rules remain in force. Query-only publication status accepts both proof versions, hashes the retained bytes and checks the recorded publication context and artifact identities. It observes the dispatch-time artifact; it does not recompute all historical transformations or grant current model, device or release qualification.

## Evidence and remaining work

Pure tests cover a fractional sample clock through reverse and forward retimes, edited labels, authored times without frame claims, collision refusal and atomic mapper updates. A lineage composition test refuses mixed stroke histories and generation rebases while retaining unrelated-axis contributions. The native CLI/mock-Stash fixture performs a reverse retime after 3× generation, refuses a corrupted receipt before writing a sibling, restores the receipt and verifies all eight original source-frame times, positions, proof bytes, unchanged master/revision, replay, authority refusal and restart. The existing reverse-origin fixture remains covered.

The fixture uses authored textured media and motion inferred by the current baseline; it is not a comparison against the sterile reference or qualified perception evidence. No actual Stash or Handy instance is contacted. Portable receipt/source/proof closure, mixed-history correspondence, destructive trim boundaries, real-media synchronization, model quality, physical Handy transport/stopping and release acceptance remain open. Exact source/binary identities and raw gate logs are retained in [this increment's receipt](evidence/roadmap-2026-10-01/retimed-stash-publication/validation.json).
