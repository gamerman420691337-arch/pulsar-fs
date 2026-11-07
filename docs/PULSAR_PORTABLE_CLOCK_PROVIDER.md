# Portable retained clock provider — 2026-10-09

Status: PARTIAL. This checkpoint implements the retained sample-plan provider. It does not complete portable per-action ancestry or enable mixed-clock publication.

## Production change

`archived_generation.rs` now shares bounded receipt/plan verification between archived generation inspection and archived source-time publication. The provider resolves the explicitly selected retained manifest namespace, producer candidate/job, worker manifest, original receipt and raw motion object. It checks source and generation settings, canonical motion identity, receipt lineage, sample-plan arithmetic and the declared archived clock metadata. It reads only retained objects with declared roles; historical paths and imported jobs are never executed. Decoded plans retain memory admission.

The existing archived ancestry guard remains strict: contributing generations must match the selected historical producer. The authored equal-placement/different-sampling clone control refuses publication with `DependencyMismatch: archived publication ancestry differs from retained evidence`. That control passed on the preceding schema-4 native-proof binary; it establishes no clone bypass in that fixture, not current-candidate runtime qualification.

## Evidence and limits

The [checkpoint packet](evidence/roadmap-2026-10-09/portable-clock-provider/checkpoint.json) retains exact commands, terminal results, source snapshots/diffs, authored fixture evidence and an unqualified runtime candidate.

- 422 source records: three existing paths changed; 419 unchanged; no new source modules.
- Default Engine: 639 passed, 55 ignored. CUDA Engine: 641 passed, 69 ignored. These are suite results, not proof that the new retained provider completed a native publication workflow.
- Architecture: 294 checks passed; 36 source guards passed. Static checks do not qualify runtime behavior.
- Offline locked CUDA release and final source-consistency build passed. Existing compiler warnings remain retained.
- The current candidate's source-clock and generation-clock CLI campaigns failed during Engine startup. No green current native publication campaign is claimed. The earlier source fixture also exceeded its 60-second test budget; that cancelled run is incomplete, not a pass.
- Startup diagnosis: the original independent probe became ready in 8.26 seconds; a failed-state probe did not become ready within 50 seconds and sampled journal/block-I/O wait channels in 99/100 observations. A corrected fresh-state probe became ready in 38.17 seconds. A first fresh-state diagnostic mistakenly used mode 0755 and was refused; that harness error is retained separately. These observations support storage-delay investigation, not a complete causal explanation or startup performance qualification.
- Longer fixture startup/test allowances did not establish a repair and were removed. The source fixture retains its original 15-second readiness and 60-second whole-test limits, a bounded CLI subprocess timer, signal-aware liveness checks and terminal diagnostics. The fixture prefix is shorter for Unix socket capacity.

Current candidate: SHA-256 `7484b713892dc6f94d0dcfa41e5bea96217bbc58c12c7b08bfcabee7b8fc355d`, 35,167,968 bytes. This is an authored-fixture software candidate, not a qualified release.

## Remaining work

Complete portable action-origin composition across imported namespaces, generation, merge, authored edits, restoration and retiming. Retain and rederive exact per-action clock carriers before allowing mixed sampling/placement publication. Verify current native campaigns and corruption/restart/clone controls. CUDA historical receipt selection also needs explicit qualification; CPU fixtures do not establish it.

Real models/media, measured or nonlinear clocks, performance, physical Handy and six-axis hardware, Stash authentication/TLS/user-library integration, desktop coverage, platform/vendor packaging and human acceptance remain open under the full roadmap. Live references remain permitted for training; sterile references remain evaluation-only. This checkpoint uses authored fixtures.

## Custody

Owned diagnostic engines and test processes are terminal. Final custody and owned scratch removal are recorded in the packet; shared services, build caches and unrelated work are preserved. Retained fixture paths name the original execution location, which is removed after evidence capture.
