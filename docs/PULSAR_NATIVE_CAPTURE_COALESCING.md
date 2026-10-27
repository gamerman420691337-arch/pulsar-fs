# Coalesced native CUDA preparation

Date: 2026-10-09
Status: Software checks PASS; native cold readiness and successful shared-image handoff remain PARTIAL.

## Change and retained boundaries

Matching cold native-image requests now wait for one producer per exact serialized dependency declaration within the Engine. The existing cache remains weak: it neither pins a warm image nor persists across Engine processes. Producer success publishes a weak reference; overlapping consumers retain the existing image and its staging/storage custody. The final consumer still releases the image through the original identity-aware cleanup path. Producer failure or unwinding releases the transient production slot and wakes waiters.

Live startup, file generation, preview and fresh retries use the same helper after their existing profile, owner-policy, request-authority, revision, device and worker-fence admission. Distinct declarations can prepare independently. Each waiter retains its own cancellation flag and absolute preparation deadline; cancelling a waiter does not cancel another request's producer. Waiting consumes that existing deadline, and a successor producer receives only its remaining time through `capture_tracked_in_lease`. A cache lookup checks captured metadata; every inference launch still authenticates mounted bytes. No runtime promotion, strong cache, supplier change, worker budget increase, deadline extension or physical device dispatch is introduced.

This removes redundant concurrent capture work. It does not establish that a first capture can consistently finish within the preview/live deadline.

## Regression and software verification

The producer-count regression first fails with two producers instead of one. The first command uses the wrong test namespace and selects zero tests; its output is retained and is not a pass for the regression. The corrected exact command is red. After the implementation, twelve native-capture tests pass. New cases cover matching cold callers, waiter cancellation and expiry without cancelling the producer, release after producer failure, distinct declarations proceeding concurrently and shared final-owner storage release. Existing weak expiry, metadata/corruption and custody cases remain green.

Direct default Engine tests pass 626 with 55 explicitly ignored; direct CUDA Engine tests pass 628 with 69 explicitly ignored. These are Engine suites compiled against fingerprint-selected read-only cached dependencies, not a Cargo workspace gate. The offline locked CUDA release build passes in 205.49 seconds. The release bundle is assembled with the unchanged static media tools and independently rehashed. Architecture and source guards pass 289 and 36. The source checkpoint has 417 records: three changed production paths, 414 unchanged. Compiler warnings and the baseline-relative patch are retained.

## Native campaigns and unresolved failures

The first public overlapping-preview campaign observes one capture worker but a client receives `InvalidRequest: transport I/O: failed to fill whole buffer`. Its harness asserts before retaining the complete outcome and terminates the Engine with a sibling pending. That run does not prove successful reuse. A fresh correctly configured owned Engine subsequently reconciles its retained custody; readiness and empty custody are verified before stopping it.

A second campaign stops at its 30-second Engine startup observation deadline and never invokes preview. Its Engine is confirmed terminal; its version-2 fence has empty groups/retired custody and no later staging/creation fields. This preserves the existing startup reliability gap.

A third campaign allows 60 seconds for observing Engine startup; production preview/capture budgets are unchanged. Both public preview requests still report `native capture absolute deadline exceeded`, after 30.029 and 36.273 seconds of client elapsed time. End-to-end client elapsed time includes admission before each request's separate preparation deadline. Both 500 ms samplers observe the same single native capture PID for about 29 seconds, with at most one producer simultaneously sampled. This is sampled runtime evidence of coalescing, not proof of every instantaneous interleaving. No successful CUDA preview, shared mounted-image handoff or cold-readiness repair is claimed. Final groups, retired custody, staging and creation records are empty.

## Rejected direct-write experiment

Before changing the preparation contract, a separate bounded diagnostic compares 1 MiB buffered writes with aligned direct destination writes under identical 1 GiB memory, one-core and 1536 MiB address-space limits. Both retain source metadata checks, fsyncs and full per-file/final SHA-256 proofs. The direct variant pads the final block and truncates back to the declared size before verification. Every one of the four 81-library, 2,434,823,256-byte outputs also passes independent Python readback.

Ordered buffered/direct/direct/buffered times are 31.636/22.341/20.618/13.318 seconds. The reversed buffered run beats both direct runs, so these uncontrolled host-cache/load observations do not establish a consistent direct-I/O benefit. No direct-I/O or buffering change is adopted in Pulsar. The initial diagnostic compile fails on indentation with warnings treated as errors; the corrected compile passes without warnings. Diagnostic service/process handles are terminal and their owned copy outputs are removed.

## Evidence and remaining roadmap

[Evidence packet](evidence/roadmap-2026-10-09/native-capture-coalescing/) retains source/patch identities, regression negatives and positives, suite/build logs, the runtime candidate, native harnesses and pressure samples, terminal/recovery custody and copy-experiment results. State databases, session credentials and owner tokens are excluded. Owned build, copy and campaign scratch is removed after terminal/process-reference checks.

Cold capture/startup reliability, successful native reuse/authority interleavings, sustained live latency, useful held-out model quality, installed Stash, broader fault/retention and platform/offline qualification, physical Handy and human/release acceptance remain open. Full roadmap PARTIAL.
