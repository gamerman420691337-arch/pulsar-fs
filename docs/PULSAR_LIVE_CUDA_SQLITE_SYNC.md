# Native CUDA lease expiry inside SQLite WAL sync

An explicitly configured Linux native test now stalls the actual SQLite commit used by the consuming CUDA finalizer. Three authored RGB frame-pair estimates run on the pinned CUDA worker, followed by real worker Finish and original-byte evidence admission. The capture keeps its original thirty-second lease and Engine-local monotonic clock.

The test obtains only its own WAL file through [SQLite file control](https://www.sqlite.org/c3ref/file_control.html), whose [journal-pointer opcode includes WAL](https://www.sqlite.org/c3ref/c_fcntl_begin_atomic_write.html). It temporarily replaces that file's [xSync method](https://www.sqlite.org/c3ref/io_methods.html) under the Engine connection mutex, preserving all other methods. A thread-local callback holds the first sync until the lease expires, then invokes the original native sync function with its original arguments. The original method table is restored before subsequent database access; no process-global VFS is changed. All interception code is confined to the Linux CUDA test module.

The retained case observes one WAL sync inside commit, a 13,759,769,148 ns injected delay, successful return from the original sync and subsequent commit return. The finalizer refuses publication with `CUDA finalization commit exceeded lease`. The capture reports Expired with no latest estimate or completion evidence. One original historical row remains readable, containing three model runs and nine CUDA computation events; its manifest identifies the pinned worker. GPU/storage reservations and ownership journals retire, with zero offline jobs.

Independent retained-file readback checks those outcomes, manifest identity, worker identity and original trace. SQL and custody assertions execute inside the native test; its temporary database is not separately retained. All 362 current/snapshot/archive source records and 76 native supplier/model records match. The preceding default workspace gate remains historical; only CUDA verification paths changed in this subject.

Fresh gates pass 549 serial CUDA Engine tests (61 ignored), the new explicit native sync-stall case, all three preceding native finalizer cases rerun on this subject, and 266 structural checks. The final CUDA test build succeeds.

This closes a selected injected WAL-sync stall across the actual consuming finalizer's commit boundary. Stop while worker Finish is already blocked, public scheduler/CLI race variants, native disk errors, power loss, additional death/overlap cuts, sustained acquisition, UI/retention work and known parallel-test failures remain open. Model/media/multimodal/six-axis/performance/Stash integration, physical Handy, platform, human and Preview/Stable release gates remain open.

See [the source-bound checkpoint](evidence/roadmap-2026-10-07/live-cuda-sqlite-sync/checkpoint.json) and [preceding boundary cases](PULSAR_LIVE_CUDA_FINALIZER_RACES.md).

The subsequent [blocked native worker Finish campaign](PULSAR_LIVE_CUDA_BLOCKED_FINISH.md) now verifies public Stop after a real pending Finish reply, using only the held owned cgroup's freezer and existing cancellation/retirement path. Public scheduler/CLI variants retain their open scope.
