# ADR B3-040: configure worker cgroups through their held directory

Status: production repair implemented after native reproduction; current-source regression gates pass within the documented scope.

Worker creation held a directory FD but used the pathname for limits and process placement. Replacing that path between opening and configuring could modify a different group. The native counterexample removes its owned empty group, creates a replacement, and proves that the preceding configuration follows the replacement. The failure and extracted preceding production operation are retained.

Use the same descriptor-relative control opener for initial limits, process placement, population observations and signaling. Opening controls uses NOFOLLOW, CLOEXEC and NONBLOCK. Failed initial setup may reclaim only the exact recorded device/inode with independently empty population. Missing or altered identities refuse cleanup; no replacement is removed. No ledger, database, portable or public protocol change is required for this correction.

This decision does not establish durable creation ownership. The mkdir-to-open and mkdir-to-registration windows remain. Creation intent, failed-cleanup custody, startup reconciliation, reader barriers and actual process-kill coverage require their own implementation and evidence before broad recovery can be claimed. Concurrent hostile same-UID namespace mutation and host power loss remain unqualified.

See the [implementation and native evidence](../../docs/PULSAR_CGROUP_CONFIGURATION_IDENTITY.md).
