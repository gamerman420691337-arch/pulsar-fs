# ADR B3-037: avoid rewriting cloneable benchmark runners

Status: implemented; ten local supervisor cases pass; complete application qualification remains PARTIAL (104/108 pass, four deadline failures, no skips).

The benchmark supervisor copied every runner through synchronous 64 KiB writes before starting the attempt deadline. An observed reduced campaign blocked in Btrfs allocation while capturing its executable; an ext4 contrast also retained disk-write throttling. These observations establish actual filesystem waits, but do not prove the causes of earlier engine-readiness or output-limit failures. Existing failures remain retained.

On Linux, attempt a forced native copy-on-write clone from `/proc/self/fd/<held input>` into the fresh private capture directory. Never reopen the caller's pathname for the clone. Unsupported cloning alone permits the bounded streaming-copy fallback; other errors refuse rather than starting another expensive copy. Open the captured file with no-follow flags, verify its exact bounded byte count and SHA-256 from that held descriptor, synchronize it, and publish owner read/execute permissions (0500). Captured bytes remain authoritative even if the original pathname is replaced or the original inode changes after cloning.

The native regression first verifies host reflink support, then forbids executable-byte writes during capture. It fails on the preceding implementation and passes on the new one. Additional cases exercise caller replacement and original-inode mutation at capture, unsupported-clone fallback, expected-digest refusal and clone I/O refusal without fallback. Existing execution, cancellation, output-limit and descendant-retirement cases remain unchanged.

This changes the benchmark supervisor only. There is no engine schema, portable format, binary, GPU admission, training or physical-device change. Clone, fallback copy, hashing, synchronization and log retention can still block in host I/O. No hard host-I/O deadline, exhaustive crash qualification, media/model quality or release acceptance is established. See the [capture evidence](../../docs/PULSAR_RUNNER_REFLINK_CAPTURE.md).
