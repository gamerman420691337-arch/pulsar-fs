# ADR B6-018: transaction-scoped private recovery lock release

Status: implemented; software verification retained separately. Full roadmap INCOMPLETE.

## Problem

A recovery transaction relied on closing its lock descriptor to release the advisory lock. An owned child retaining a duplicate of that open-file description keeps the lock after the caller returns, producing a false busy refusal for the next transaction. CLOEXEC does not resolve a fork-to-exec window or explicitly inherited duplicates. Linux documents these semantics in [flock(2)](https://man7.org/linux/man-pages/man2/flock.2.html).

## Decision

Keep nonblocking acquisition and every existing private/no-follow/single-link/inode check. Add a borrowed guard after successful acquisition. Hold the lock through callback, synced atomic rename and directory sync; explicitly unlock before returning success. An unlock refusal returns an error, preserving original intent for recovery instead of authorizing dispatch. The guard also attempts release on early error and unwinding. No descriptor or execution capability is exposed outside the private implementation; a narrow internal helper lets the regression safely pass a duplicate to an owned sleeping child without unsafe Rust.

An advisory unlock releases only transaction exclusion. It neither acknowledges engine work nor deletes original identities, relaxes private file checks, retries a busy operation or extends a timeout. OS-level unlock failure on an error/unwind path remains best-effort cleanup; abort, process kill, power loss and filesystem failure remain independent qualification conditions.

## Compatibility and verification

No public API, request/reply format, schema, dependency, retry policy or unsafe-code exception changes. All client journals sharing this backend receive the same transaction lifecycle. A real-file/real-child regression first reproduces EAGAIN after return, then checks successful mutation, callback refusal and caught unwind while the same child descriptor remains open. Competing reads stay refused during the active transaction; the owned child is killed and joined before assertions. See [scope and source-bound verification](../../docs/PULSAR_JOURNAL_LOCK_RELEASE.md). This mechanism is proven, but its presence in every earlier intermittent full-suite failure is not independently established. Slow sync, lock fairness, platform/power-loss/model/media/device/human/release gates remain separate.
