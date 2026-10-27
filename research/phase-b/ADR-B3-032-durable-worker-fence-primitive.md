# ADR B3-032: lease plus a durable cgroup roster

Status: private primitive implemented; engine and reader-barrier integration pending.

An in-process reaper retains capacity only while its process remains alive. A persisted cgroup roster alone also has a gap: a forked child may not yet have entered its cgroup when a replacement engine inspects the empty group. Pair a pre-fork synchronized roster with a read-only CLOEXEC lease inherited by the child until exec, and require cgroup placement in pre_exec.

Bind ledger scope, lease inode and cgroup inode/device identities. Refuse unresolved or malformed entries without signaling any recorded group. Do not treat a missing ledger or replaced lease as fresh ownership. Read population through a held cgroup directory. Clear records only after independent exit/emptiness observations and durable replacement.

Native fork and cgroup tests establish the primitive's local behavior. They do not establish engine admission, an older-reader barrier, failed-spawn cleanup, complete crash recovery or GPU lifetime qualification. Engine integration must retain the lease and roster through all dispatch and retirement paths before enabling production GPU scheduling.

See the [contract](../../docs/PULSAR_WORKER_FENCE_PRIMITIVE.md) and its source-bound packet for exact executed scope.
