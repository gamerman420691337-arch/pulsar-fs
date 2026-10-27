# Shared native image handoff checkpoint

The real generation/annotated-preview overlap now completes on one captured native image. This closes the reproduced shared-loader mutation defect for the authored Linux CUDA campaign. Cold preview readiness and full roadmap qualification remain open.

## Failure and repair

The original overlapping campaign completed preview but failed generation with a truncated worker reply. Transport supervision discarded its already bounded stderr. A regression reproduces that loss; the repair retains diagnostics on request-write and reply-read errors, with two additional nonblocking reads and the existing 16 KiB retention cap. Cancellation, deadlines and custody retirement remain separate and unchanged.

The rebuilt diagnostic campaign exposes `mounted alias changed after byte verification`. Both consumers use the same captured image, and every CUDA mount preparation calls `chmod` on its loader. Even an unchanged permission mode updates ctime, invalidating a sibling's active inode proof. A deterministic regression reproduces the exact error across the hash/final-recheck boundary.

Capture now finalizes the declared Linux loader to owner-read/execute mode `0500` before publishing the image. Offline, owned and live launch preparation verifies that mode without changing it. Altered permissions are refused without repair. SHA-256, inode, size, mode, mtime, ctime and final alias rechecks remain enforced; no byte-proof checks are relaxed.

## Verification

- Transport tests pass four cases; existing pipe tests pass six. Mounted-byte tests pass seven, including corruption, replacement, conflicting identities and deadlines. Native dependency tests pass five, including final loader permissions and refusal of altered modes.
- Final direct Engine suites pass 631 default tests with 55 ignored and 633 CUDA tests with 69 ignored. These use fingerprint-selected read-only cached dependencies, not a Cargo workspace test gate. Earlier concurrent runs were terminated at their 85-second test limits and remain incomplete evidence; serial reruns pass with longer test-only limits.
- Offline locked CUDA release build passes in 217.09 seconds. All seven assembled runtime files are independently rehashed. Architecture/source guards pass 289/36; selected CUDA source tests and diff checks pass. Compiler warnings are retained.
- The final source checkpoint contains 417 records, with five changed paths and 412 unchanged relative to the preceding capture-coalescing checkpoint. One changed path adds the mounted-byte regression; four change production behavior. Source identity is rechecked after build and before retention.
- The corrected native campaign completes generation in 71.182 seconds and preview in 10.952 seconds. Preview starts after observed full capture writes; neither software lease is extended. Samples show one capture producer, one image root held by both consumer plans and two inference worker-exec stages. All three campaign states have empty final worker/staging/creation custody after their owned Engines terminate.
- Generation has 32 samples: one explicit synthesized anchor and 31 inferred positions. Original CUDA trace bytes prove 31 runs/372 CUDA computation events. Independent CPU positions differ by at most `2.9802322387695312e-8`; the integer export matches all 32 original sample times and rounded positions.
- Preview selects source frames 0/1. Both target RGB frames match the independently decoded fixture exactly; the CPU position is identical. Original CUDA trace bytes prove one run/12 CUDA computation events.

The [evidence packet](evidence/roadmap-2026-10-09/shared-native-image-handoff/checkpoint.json) retains both red regressions, compile and runtime negatives, the diagnostic and repaired suppliers, source snapshots, tests, original outputs/traces, independent checks and custody records. Private databases and session credentials are excluded. Owned build/test/campaign scratch is removed after process-reference and packet-integrity checks.

## Open gates

This campaign deliberately starts preview after capture has written the full 2,434,823,256-byte image. It does not prove a cold annotated preview can become ready inside its original 30-second lease, nor sustained live latency. The matching CUDA policy is an explicitly selected diagnostic candidate, not a promoted policy. Authored media/model execution does not establish useful model quality, sterile real-media evaluation, actual Stash interoperability, Handy/firmware playback, other platforms, Core/Complete packaging or human acceptance. Full roadmap status remains PARTIAL.
