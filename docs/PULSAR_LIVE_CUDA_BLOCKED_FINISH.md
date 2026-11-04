# Stop during a blocked native CUDA worker Finish

An explicitly configured Linux native test now verifies authenticated public Stop while the private consuming finalizer is waiting for the actual CUDA worker's Finish reply. The fixture uses three accepted native provisional estimates, a pinned authored scalar model/native runtime, and a test-inserted capture registry entry. The original thirty-second lease and Engine-local monotonic clock stay intact.

The test freezes only the owned worker's exact held cgroup and confirms `frozen 1`. Controls are opened relative to its held directory, preserving inode ownership. Linux documents the [cgroup freezer and fatal-signal behavior](https://docs.kernel.org/admin-guide/cgroup-v2.html); a frozen group remains killable. An RAII guard attempts thaw on exit, including failure paths. No parent cgroup or unrelated process is changed.

The real finalizer enters Finishing and sends the actual Finish header. A thread-local, one-shot observer confirms its reply read returned WouldBlock, then returns immediately. After another 100 ms, the finalizer remains pending. Public Stop retires the capture; the finalizer exits within the two-second test bound after Stop returns, while still inside the original lease. Cancellation uses the existing monitor/watchdog and owned-group retirement path. The test performs no manual kill or custody deletion.

The retained case completes in 16.26 seconds overall; Stop is recorded at 15,338,880,732 ns. These are authored-case observations, not a playback or capture-to-command latency qualification. The terminal report remains Stopped, with no latest estimate or completion evidence. No evidence row is retained, zero offline jobs exist, and GPU/storage reservations plus all ownership journals retire. Final provider traces are not published for this cancelled capture.

Independent retained-report readback confirms the outcome. The standalone typed report serializes f32 scale values differently from the embedded serde_json::Value summary. Exact IEEE f32 scale bytes correspond and all other parsed fields are identical. The initial raw-number comparison failure and its corrected scope are retained; no byte equality is claimed. Freezer, WouldBlock, bounded-join, SQL and custody assertions execute inside the native test; its temporary database is not separately retained.

All additions are compiled only in test builds. All 362 current/snapshot/archive source records and 76 native supplier/model records match. See [the source-bound checkpoint](evidence/roadmap-2026-10-07/live-cuda-blocked-finish/checkpoint.json).

Fresh gates pass 1,032 serial workspace tests (55 ignored), 549 serial CUDA Engine tests (62 ignored), the new blocked-Finish case, all four preceding native finalizer cases rerun on this subject, the CUDA test build and 266 structural checks. The preceding SQLite checkpoint summary undercounted its passing structural checks by one; the raw log has 266 records and current prose is corrected, with the original sealed metadata retained.

Public startup/scheduler/CLI/UI race variants, additional inference/finalizer/commit death and overlap cuts, disk errors/power loss, sustained acquisition, evidence inspection/retention and known parallel-test failures remain open. Model/media/multimodal/six-axis/performance/Stash integration, physical Handy, platform, human and Preview/Stable release gates remain open.

The subsequent [public blocked-Finish cancellation case](PULSAR_PUBLIC_CUDA_BLOCKED_FINISH.md) now exercises public capture startup and automatic scheduling, observes pending Finishing, calls public Stop and verifies exact terminal Start/Finish replay, retired custody and no retained evidence row. CLI/UI and additional scheduler race variants remain open.
