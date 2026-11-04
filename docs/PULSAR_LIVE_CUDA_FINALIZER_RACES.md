# Native CUDA consuming-finalizer boundary tests

Three explicitly configured Linux tests now exercise the actual Engine consuming finalizer after three real CUDA frame-pair estimates. Each uses the original Engine-local monotonic clock and thirty-second lease, a pinned authored scalar model/native runtime and a private test-inserted capture registry entry. There is no physical actuation or real-media/model-quality claim.

The pauses are thread-local and compiled only under `cfg(test)`. They interrupt the actual finalizer at named boundaries; they do not replace worker Finish, evidence verification or SQLite retention. The shipped runtime contains no hook.

- **Stop before worker Finish:** after Finishing freezes intake, an authenticated public Stop retires the capture. Releasing the finalizer makes worker consumption refuse; the terminal report stays Stopped with no evidence publication or retained row.
- **Real expiry before commit:** worker Finish and native evidence admission succeed, then the transaction is held before the lease check until the original lease expires. Finalization refuses and the transaction rolls back, leaving no retained row.
- **Real expiry after commit:** original evidence commits, then publication is held until the original lease expires. The report stays Expired with no completion evidence or latest estimate. The retained historical row contains three model runs and nine CUDA computation events; durable bytes do not reactivate the capture or authorize Completed.

All cases verify empty ownership journals, retired GPU/storage reservations and zero offline jobs. Independent retained-file readback verifies the three report outcomes, exact manifest identity, native worker identity and original after-commit trace. SQL and custody assertions execute inside the native tests; the private temporary database is not separately retained.

The after-commit pause is **outside** SQLite commit. This checks the post-commit publication deadline branch; it does not inject a slow filesystem sync inside SQLite. Stop while worker Finish is already blocked, true disk-commit stalls, public scheduler/CLI/UI variants, additional death/overlap cuts, power loss, sustained acquisition, known parallel failures and broader product/model/media/six-axis/Handy/platform/human/release gates remain open.

Fresh gates pass 1,032 serial workspace tests (55 ignored), 549 serial CUDA Engine tests (60 ignored), all three explicitly run native cases and 265 structural checks. All 361 source/snapshot/archive records and 76 native supplier/model records match; only the test module and test-build hooks changed.

See [the source-bound evidence packet](evidence/roadmap-2026-10-07/live-cuda-finalizer-races/checkpoint.json). Existing [finalization](PULSAR_LIVE_CUDA_FINALIZATION.md) and [public CLI recovery](PULSAR_CUDA_CLI_RECOVERY.md) checkpoints keep their original verification scopes.

A subsequent [native WAL-sync stall](PULSAR_LIVE_CUDA_SQLITE_SYNC.md) now verifies real lease expiry while SQLite's actual commit is held inside its test-owned sync callback, followed by successful native sync and refusal of Completed publication. The earlier after-commit pause above retains its narrower scope.

The subsequent [blocked native worker Finish campaign](PULSAR_LIVE_CUDA_BLOCKED_FINISH.md) now verifies public Stop after a real pending Finish reply, using only the held owned cgroup's freezer and existing cancellation/retirement path. Public scheduler/CLI variants retain their open scope.
