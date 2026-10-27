# Living native image reuse, 2026-10-09

Status: PARTIAL. Software ownership and Engine checks pass; a rebuilt native CUDA campaign is still required. First-start cost and full roadmap qualification remain open.

## Implemented contract

An Engine holds a single weak reference to its most recently captured living native image. An overlapping live CUDA startup may borrow that frozen image only after request, revision, worker-fence and owner-policy admission, with an exact serialized NativeDependencySpec match, an active cancellation/deadline check and intact captured metadata. No original supplier paths are reopened on reuse, consistent with the existing frozen-capture contract.

SupervisedNativeDependencies now owns an Arc of the captured image, completion provenance and mutex-protected staging custody. The mutex makes the Send-only staging retention safely shareable; it does not expose mutation or unlock cleanup early. Each borrower retains the same storage claim. Releasing a use does not delete another borrower's image. The final use transfers the existing identity-aware staging obligation to cleanup. An explicit final close waits for cleanup; Drop retains asynchronous cleanup behavior. The weak cache itself retains neither storage nor files and cannot resurrect a removed image.

Authority performs metadata checks only. Existing GPU-memory observation and inference bootstrap continue to authenticate mounted bytes on every launch. Reuse is not admission based on a trusted cached digest. Device selection, GPU budget, source/model/profile checks, Ready revalidation and worker retirement remain required. No limits or lease deadlines change.

There is no persistence, cross-Engine reuse, warm job, strong cache pin or single-flight producer. Concurrent cold misses may each capture under existing capacity/storage admission. A new capture after the final use must read the supplier again. First startup still captures the full native image; this change targets overlapping preparation, not cold-start reliability.

## Verification and limits

The [evidence packet](evidence/roadmap-2026-10-09/live-native-image-reuse/checkpoint.json) binds 414 source records: three changed production paths and 411 unchanged, plus the baseline-relative patch and complete source archive.

Six native-capture tests pass. New cases exercise explicit close and Drop with multiple uses, retained storage until final cleanup, supplier deletion after frozen capture, weak-cache expiry, exact-spec mismatch, cancellation/expiry, missing alias refusal and same-size content corruption. The latter passes metadata-only lookup but fails full captured-byte verification, demonstrating that lookup is not a byte proof. Existing cleanup-failure tests retain the storage claim until retry succeeds.

Direct rustc compilation uses exact fingerprint-selected cached dependencies, read only, and writes compiled outputs to owned NVMe scratch. The first compilation fails because staging retention is Send but not Sync; the corrected custody mutex compiles. Retained logs record both outcomes. Default Engine passes 619 tests with 54 ignored; CUDA Engine passes 621 with 68 ignored. Architecture passes 289 checks; guards pass 36. These counts describe the recorded direct builds, not Cargo workspace verification. Ignored native/platform/model tests remain unexecuted.

The [current CUDA release rebuild](PULSAR_NATIVE_IMAGE_REUSE_RUNTIME.md) passes, but three native campaigns fail before initial Ready and never exercise handoff. Native prepared handoff, actual copy avoidance, mounted corruption refusal and durable staging cleanup with shared uses still need runtime evidence. Prior workspace/startup failures remain unresolved. Exact native Ready/revocation interleavings, sustained encoded/camera live latency, six-axis, real model/media quality, Stash, offline/platform, physical Handy and human/release acceptance remain independent open gates.

## Next gate

Resolve the measured cold preparation/readiness gap while retaining fresh byte proofs and existing leases, then run actual overlapping live capture with independently checked output and terminal custody. Observe whether the successor reuses the same staged image without another capture producer; test last-use cleanup and mounted byte refusal. Retain cold-start failures separately and keep existing budgets/deadlines unchanged.
