# ADR B3-031: reservation custody follows worker-tree retirement

Status: implemented for ordinary Linux worker launch; durable restart and production CUDA lifecycle remain open.

Job reservations previously lived only in the dispatch thread. The ordinary launcher killed the owned cgroup on errors and waited for the direct child, but did not establish that all descendants had exited before the dispatch reservation could drop. Success likewise relied on direct-child exit. Deadline-aware pipes alone do not establish retirement.

Share the existing admitted reservation between the engine operation and a launcher-owned child/cgroup guard. Successful launch requires observed direct-child exit and an empty owned cgroup. On error, allow a two-second observation interval after signaling, then transfer unresolved custody to a reaper. Treat observation errors as absence of retirement proof. Retain capacity if a reaper cannot be created, rather than admitting another job against potentially live resources.

Keep the engine share through completion or preview publication. Standalone benchmark calls receive their own local claim; shared benchmark-caller admission is not implied. Preserve CPU-only production admission while GPU mount, capture, acceptance and restart custody are still being integrated. No DB schema, portable format, device effect or model-quality policy changes are introduced.

See the [contract](../../docs/PULSAR_WORKER_RETIREMENT.md) for executed test scope and outstanding lifecycle obligations.
