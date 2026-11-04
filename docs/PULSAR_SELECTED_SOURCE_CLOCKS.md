# Explicit selected source clocks

Date: 2026-10-09
Status: pure arithmetic implemented; Engine correspondence/publication integration pending

## Why action selection is explicit

The [native Stash counterexample](PULSAR_STASH_GENERATION_CLOCK_GUARD.md) distinguishes a preserved merge boundary from a contributed interior point. At project 125 ms, the boundary retains the parent clock. At project 250 ms, the contributed value originates at source 500 ms/frame 4. Assigning a clock solely by half-open merge interval membership can therefore mislabel the boundary even after fixing the interior.

The core now exposes `map_track_to_selected_source_clocks`. Each current action has an explicit `(project_time, carrier)` selection. A carrier is an existing checked `SourceClockMapper`, including its original sample anchors and verified retiming arithmetic. The function requires exact project-order action/selection correspondence, one source version, bounded carrier count, bounded aggregate sample anchors, bounded nonempty motion and resolved gaps. It preserves values, evidence, name and axis while sorting output actions, mappings and carrier indices together into source order. Quantized source timestamp collisions refuse; no value is dropped or implicitly resampled. Authored actions with no retained sample anchor carry no original-frame claim.

This operation supplies arithmetic, not ancestry or publication authority. A caller can supply an incorrect selection; the pure kernel does not certify it. It does not decide continuous interpolation across source-overlapping pieces. Existing Stash mixed-clock refusal is unchanged, and there is no new CLI, wire command or enabled publication path.

## Evidence and next integration gate

Six new tests cover two different forward clocks, exact boundary/interior selection, a correctly mapped collision with the parent at 500 ms source time, reverse output ordering, single-carrier equivalence with existing fractional/reverse mapping, synthesized evidence preservation, no-frame authored actions, foreign sources, missing/stale/absent selections, unresolved gaps, aggregate admission and quantization overflow. These establish deterministic arithmetic on authored fixtures, not real-media quality.

The source-bound [checkpoint](evidence/roadmap-2026-10-09/selected-source-clocks/checkpoint.json) retains exact direct-compiler commands, dependencies, test outcomes and limitations. These are direct scoped builds, not Cargo workspace/release gates. A first downstream compile omitted Cargo's manifest environment and the new core dependency search path; its diagnostics are retained, then the corrected invocation passes. The prior released runtime candidate remains an older supplier and is not relabelled as containing this change.

Before publication can use the new kernel, the Engine must verify each carrier's immutable job/candidate/source/receipt identity and every action selection against effective revision, merge-boundary, edit, replacement, rebase, undo/redo and retiming facts. It must reject unknown/mixed imported ancestry and overlapping or colliding output that needs explicit resampling. A versioned durable proof must retain carrier identities and output-parallel selections; replay, restart, portable retention and older-reader behavior require production regressions. The known mixed example must still refuse when its correct mapping collides. Model, platform, real-media, physical-device and human acceptance gates remain independent and open.


## Native origin prerequisite, 2026-10-09

[Authority-owned native origin loading](PULSAR_NATIVE_CLOCK_ORIGINS.md) is now integrated into existing native publication. It bounds metadata and receipt/program reads, binds source/job/candidate/receipt identities and checks actual stored original motion bytes under retained memory admission. It prepares verified native carriers for future selection composition; action-by-action ancestry derivation and durable mixed-clock proofs are still pending, and the differing-clock publication refusal remains in force.


## Exact merge action origins — 2026-10-09

[Exact merge action origins](PULSAR_EXACT_MERGE_ACTION_ORIGINS.md) adds an optional collector to the production merge kernel, retaining each output action’s exact Current/Candidate input and original index under the actual protection/write mask. Equal-valued boundaries remain Current. Core composition checks the correct source frame for an interior contributor and refuses a source-time collision. All core unit/integration targets and default/CUDA Engine regression suites pass. Engine ancestry composition, durable mixed-clock proofs and portable closure remain pending; roadmap status remains PARTIAL.
