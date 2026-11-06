> Current follow-up: [transaction-scoped lock release](PULSAR_JOURNAL_LOCK_RELEASE.md) now repairs a demonstrated inherited-descriptor mechanism. Complete standalone and engine-feature-unified client libraries pass 198 tests/four ignored each. The historical sync delays and failures below remain retained; no universal latency or storage-root-cause claim follows.

# Client journal contention reproduction and contract checks — 2026-10-10

The previous full workspace failure is now reproducible in a short, owned high-contention workload. This increment changes only pack-recovery tests and retains explicit runtime non-passes. It is not a production journal repair or a latency qualification.

## Reproduction and minimization

The original workspace test failed `pack_recovery::tests::concurrent_windows_merge_distinct_requests_under_the_same_session` with `assertion failed: done`. Its four threads each allow 1,000 retries with one-millisecond sleeps. The actual storage contract ([ADR B6-011](../research/phase-b/ADR-B6-011-session-bound-desktop-pack-recovery.md)) permits immediate busy refusal before mutation dispatch and preserves original request identity for explicit retry; it promises no lock fairness or completion within that retry count.

Using the exact unchanged baseline client test binary:

- Four serial complete client suites PASS (193 tests, four ignored each).
- Four simultaneous complete suites PASS unchanged.
- Sixteen simultaneous complete suites all FAIL with the original pack assertion, about 3.8 seconds each. One suite also observes a distinct sequential evidence-journal load refusal (`Recovery journal is busy; no mutation was dispatched`, os error 11); that observation is not asserted explained or repaired here.
- Sixteen simultaneous executions of only the original pack concurrency test all PASS.
- Sixteen simultaneous recovery-only suites (24 cases each) all FAIL with the original pack assertion. This narrows the load-bearing workload from the complete suite to related durable journal work.
- A recovery-only probe with one traced process and fifteen untraced processes again FAILS all sixteen. The failed traced thread receives exactly 1,000 `flock` EAGAIN refusals. The trace records 734 completed sync calls, maximum 0.739325 seconds; the 49.406841-second sum includes overlapping threads and is not elapsed wall time.
- Sixteen pack-only suites (five cases each), with one descriptor-identity trace, PASS. Returning to the recovery-only workload with descriptor identity gives fourteen failures and two passes, including the traced pass. Instrumentation therefore affects the observed outcome and must not be called a repair.

Ranked hypotheses before probes were long durable writes while the journal lock is held, thread scheduling delay without long I/O, and unexpectedly retained/replaced lock identity. The observations favor storage-related contention but do not isolate every cause of the historical failure or explain the separate sequential load refusal. No unrelated process, filesystem setting, cache or service was changed.

## Stronger test-only assertions

The revised concurrency regression submits all four windows' original requests, records successes and documented busy refusals, joins the windows and compares the exact successfully persisted ID set before retry. It then explicitly retries refused original IDs and requires all 32 exact IDs, original typed intent and owner binding, no duplicate records and no inferred acknowledgment. The artificial retry-count success assumption is removed rather than increased. This verifies merge/backpressure/retry semantics; it supplies no latency guarantee.

A new controlled held-lock regression requires the actual WOULDBLOCK cause, unchanged journal bytes on refusal, original-ID retry after lock release, deduplicated replay and two unacknowledged original records. It PASSes in the focused pack run.

The entire production prefix through the actual `#[cfg(all(test, unix))] mod tests` boundary is byte-identical to the captured source. No store, lock, sync, retry policy, client command, protocol, schema or authority behavior changed. Compared with the preceding durable-stop checkpoint, only `pack_recovery.rs` changes; 347 source identities remain unchanged. Existing interface ADRs remain applicable; no new shared interface is introduced.

## Current gates and limits

- Default client test build PASS. Default and all-feature workspace/all-target strict Clippy PASS. Formatting PASS; architecture 350 and source guards 36 PASS.
- Candidate sixteen-process recovery workload does **not** PASS: existing WASM/archive ten-second journal waits fail, then all sixteen commands exceed the unchanged 30-second limit (SIGKILL, return -9). No completion summary or successful candidate merge assertion is inferred.
- Focused six-case candidate pack suite times out at 45 seconds (exit 124) after four completed passes, including the new held-lock case. Complete candidate pack/client/workspace runtime verification remains open.
- A single revised merge test, under the unchanged 45-second command limit, also does **not** complete (exit 137 after termination/kill grace). Its descriptor-bound trace records directory sync 19.541659 seconds, file sync 21.049707 seconds and following directory sync 8.123166 seconds. The writer holds the same pack lock while peers receive immediate EAGAIN. A live owned-thread observation reports `folio_wait_bit_common`; the parent and test thread wait on futexes. This locates a storage wait in that probe, not a universal device/filesystem root cause or a passing timing result.

No test/storage deadline was extended and no synchronization was removed. Before/candidate client binaries use different Cargo feature-unification contexts (previous workspace all-feature versus direct default client build); their separate hashes and source subjects are retained. This is not a paired performance comparison. The prior engine 712-pass checkpoint and prior workspace failure remain their own evidence; no current full-workspace PASS is claimed.

[Public checkpoint](evidence/roadmap-2026-10-10/client-journal-contention/checkpoint.json) retains all bounded loop results, public logs/traces, original/candidate source and runtime hashes. Private generated fixture state is excluded and removed after owned process references disappear.

Full roadmap remains active. Candidate runtime verification, client/storage stall and distinct sequential-busy investigation, engine/native intermittent reliability, remaining assistant/worker/device/distribution software, model/media/annotation quality, platform, physical Handy 2 Pro and six-axis behavior, rendered GUI, human and release acceptance remain open.
