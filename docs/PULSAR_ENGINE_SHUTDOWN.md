# Engine shutdown and worker retirement

The Unix Engine daemon now handles SIGTERM and SIGINT through an owned shutdown path.
The signal handler only sets an atomic flag; ordinary threads close admission, retire work,
interrupt IPC connections and join their workers. This is Engine process shutdown. Closing
the desktop or CLI client still leaves the separate Engine available for retained offline work.

The [checkpoint](evidence/roadmap-2026-10-22/engine-shutdown/checkpoint.json) identifies the
checked source, executable hashes, tests and native Linux process observations. The default
Engine suite passes 1,119 tests with 70 explicit environment-dependent ignores. The optional
WASM filter passes 22 with seven ignores, and launcher tests pass two. Five shutdown cases and
nine task-group cases are included in the default suite. Strict default/all-feature lint,
App/Worker builds and architecture/format guards pass.

The final native binaries exit zero on SIGTERM and SIGINT with ten accepted idle or incomplete
control/bulk connections open. Peer connections close, owned sockets disappear, the instance
lock is released, and projects survive restart. Partial startup with an untrusted bulk file
retires the already-started control listener and leaves that file unchanged; moving only that
owned fixture permits restart. No forced kill was used. Initial compilation diagnostics and
two RAM-budget tests that assumed a locally installed ONNX runtime are retained with repairs.
Those identity-only tests now use an explicit nonexecuted runtime artifact.

## Lifecycle

The Engine owns separate control and bulk listeners. Each listener retains its accepted
connection threads and duplicate socket descriptors. On shutdown, those descriptors interrupt
idle or incomplete reads and writes. The listener stops accepting, then joins its connections.
Owned socket paths are removed only if their device/inode identity still matches. A regular
file or other untrusted endpoint is refused; partial listener startup is cleaned up.

Engine requests and new background tasks refuse after shutdown begins. The independent Handy
authorizer checks the same closing flag before its final play claim. Existing controllers retire
and retain their stop requests. The monitor and shared play/stop worker join while their original
DB, journal, runtime and transport resources still exist. Their panics return an error rather than
being reported as successful shutdown. An acknowledged stop remains physically unconfirmed.

Generation, assistant, WASM, package, live capture, transfer and batch supervisors now retain
join handles in a bounded task group. It closes admission atomically, reaps finished tasks,
joins outside its mutex, retains panic diagnostics and refuses self-join. It has no joining
destructor because the last Engine reference can belong to a worker. Native preparation keeps
its existing explicit producer handles, which are cancelled and joined separately.

Shutdown signals registered cancellation flags before waiting for supervisors, then checks
native worker custody. A pending worker-fence roster after the bounded retirement observation
produces an error. Failure does not fabricate an empty roster, successful device stop or cleared
uncertainty. The instance lock remains owned through listener and supervisor joining. Startup
failure after Engine construction follows the same retirement path.

## Operation and limits

For a foreground `pulsar engine`, Ctrl+C requests this shutdown. A service manager or operator
can send SIGTERM to the exact Engine PID they own. This change installs no service and does not
stop another running application. Repeated signals do not turn unresolved work into success.
SIGKILL, host power loss and an unresponsive operating-system resource remain abrupt failures;
existing restart and custody checks still apply.

The socket shutdown path and signal handling are compiled for Unix; native execution evidence
in this increment is Linux-only. Windows still requires its secured listener implementation.
The native signal campaign uses fresh private state and no model/media jobs or hardware. Composed
Handy shutdown tests use localhost HTTP with synthetic timing/qualification. Neither establishes
physical stopping, real-device synchronization, GPU/native storage-failure qualification, or
successful cancellation of every ignored environment-dependent scenario.

The [emergency fallback](PULSAR_HANDY_EMERGENCY_STOP.md) now attempts retained stops when
persistence cannot confirm their prewrite and reports unresolved outcome metadata at shutdown.
Attempted-effect reconciliation, physical emergency qualification, continuous clock renewal and
browser presentation/heartbeat integration remain open. Model, real-media,
platform, physical-device, storage, human and release gates are not closed by software teardown.
