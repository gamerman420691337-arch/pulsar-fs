# Native image reuse runtime checkpoint, 2026-10-09

Status: PARTIAL. The current CUDA release builds; initial readiness prevents native handoff verification.

## Build and source

The [packet](evidence/roadmap-2026-10-09/native-image-reuse-runtime/checkpoint.json) records an offline, locked Cargo CUDA release build, completed in 4m16s with 25 Engine warnings. It uses an owned NVMe copy of the existing release cache. The original shared supplier is preserved. The rebuilt executable is retained at `evidence/roadmap-2026-10-09/native-image-reuse-runtime/native-release/pulsar`, SHA-256 `4838d0f9027092d08ba38df7814e54065a8409cd275e5a0713c3185718eb3909` (34,934,880 bytes).

The complete source checkpoint contains 415 records: one added native test and 414 unchanged from the living-image-reuse checkpoint. Production Rust is unchanged in this slice. Architecture/guards pass 289/36; test syntax passes. These do not establish workspace, model, media or native qualification.

## Native test and actual outcomes

`scripts/live-native-image-reuse.test.mjs` launches an owned Engine using the pinned CUDA runtime, scalar ONNX model and declared native dependency set. A private policy copy permits two interactive GPU reservations at 256 MiB; the supplied 128 MiB policy remains unchanged. The fixture is intended to compare one image's alias inode set before/after prepared handoff, prove successor RGB hashes/PTS/oracle and actual CUDA Finish, and confirm final durable cleanup. None of those handoff/output assertions are reached in these runs.

Initial and observed runs FAIL with `AssertionError: capture Ready` at the fixture's 29-second observation cutoff. Their raw failures and initial fixture versions are retained. The corrected fixture observes through 31 seconds without changing the Engine's 30-second lease. That run FAILS with `false !== true`: the initial capture is observed inactive with inference state `retired` around 30.17 seconds. Its inference error field is null; the fixture does not retain the capture's retirement-reason field, so this packet does not prove a specific retirement reason. No successful Ready or reused image is claimed.

The corrected fixture records descendants of the exact owned Engine PID, obtains bounded process inventories and retains only whitelisted exec-role names, numeric I/O and wait channels. It does not log command lines, credentials or environment values. Sampling is roughly once per second; it is not a complete exec trace or exact phase timer.

In the observed early-cutoff run, `native-capture-worker` is sampled through about 22.6s, followed by `cuda-live-memory-observer` at 24.7–25.7s and `cuda-live-worker` at 28.9s. The latter has read about 1.1GB at its observation. In the terminal run, native capture is sampled through about 25.6s and the memory observer at 27.8–29.9s; no live-worker sample is seen. Native capture writes the full declared 1,945,994,000-byte image (75 distinct files, 84 aliases) and is repeatedly sampled in dirty-page throttling and file/page waits. Memory observation reads substantial bytes without writes. These observations distinguish actual exec roles; they do not isolate all scheduler, filesystem, hashing or library-loading costs.

All three owned Engines terminate by SIGTERM and each final fence has empty groups, retired, staging and creation records. Disposable credentials and databases are removed with the run roots; only allowlisted logs, observations and fence/exit metadata are retained. The copied build cache is removed after retaining the executable and build evidence.

## Remaining gate

Native reuse and its durable shared cleanup still need successful actual handoff. Rebuilding did not remove the cold preparation cost. The next implementation/measurement decision must account for the capture copy/hash work and both independent mounted-byte proofs, with fresh verification, storage custody and existing leases retained. Do not skip byte proofs or treat another authored model as a quality result.

Prior workspace/startup failures, native actor/revocation races, complete sustained live capture-to-command and six-axis behavior, trained model/real-media quality, installed Stash, offline/platform matrix, physical Handy and human/release acceptance remain open. The full roadmap objective is unchanged.
