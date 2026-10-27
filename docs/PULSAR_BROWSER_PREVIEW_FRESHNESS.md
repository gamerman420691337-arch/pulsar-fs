# Browser preview freshness

The live player's position meter now expires estimates using both declared source
time and elapsed browser time. This fixes three reproduced display errors: a slow
reply could show an already old estimate, a stalled video or request could leave
the last position visible indefinitely, and busy callbacks could hide advancing
presentation time from the freshness check.

The browser records a monotonic timestamp before sampling pixels. On reply, the
remaining display budget is the Engine's maximum analysis age minus the estimate's
age at that sampled source timestamp and the elapsed time since sampling. Elapsed
time includes pixel extraction, Start, queued requests and the frame response.
The latest observed presentation timestamp supplies another source-age check,
including callbacks received while a request is pending. The player still sends
at most one frame request at a time.

For example, a result stamped 0 seconds, sampled at source time 0.125 seconds with
a one-second limit, has 875 ms left at sampling. A reply 200 ms later has at most
675 ms left for display. A callback at source time 2 seconds makes that result
stale even if very little browser time has elapsed.

Each displayed estimate owns one expiry timer. When the timer or a new frame
callback observes expiry, the meter clears and reports that it is waiting for a
current estimate. Repeating the same estimate timestamp cannot extend or revive
its deadline; older estimates refuse display. A newer estimate may restore the
preview. Stop, seek, source changes, a new Play and page exit clear timer ownership
and remembered timing. Obsolete timer callbacks cannot clear replacement motion;
the existing playback-generation check still rejects late replies. When no usable
estimate is available, the status retains the Engine's inference state, including
starting and failed, alongside the waiting message.

This is a browser display check. Timer execution can be delayed while the browser
is suspended or busy; it is not a hard real-time deadline. Source timestamps remain
producer declarations. No sensor exposure calibration, source-tab player mapping,
physical response timing or synchronized device output follows from this change.
The position meter sends no toy commands. Recording and candidate provenance still
come from the existing Engine paths.

## Evidence

The actual embedded player script is exercised with a mock DOM/HTTP transport and
deterministic monotonic clock/timers. Before the fix, seven new tests failed, including
`0.6 !== 0` after expiry, while the prior 31 passed. Accounting for elapsed sampling
time fixed the two delayed-response tests. Adding owned expiry left only the busy
presentation test failing; observing busy callbacks fixed it. Forty-one tests now cover
those regressions, deadline non-extension, replacement playback, renewed estimates
and obsolete timer callbacks as well as existing camera/tab/recording controls.
Final review reproduced and repaired a new generic-message regression that hid the
Engine inference state; its failing test and corrected result are retained too.

The full Engine rerun timed out at 900 seconds after 312 reported passes and no
reported assertion failures; it remains incomplete. An initial parallel Client
run passed 427 tests and failed five bounded socket, child-retirement and journal
contention checks. All five passed individually. A serial run then passed 431 and
reproduced the fake socket's accept timeout. That fixture allowed only three seconds
between connections, including the client's local durable staging work. Its bounded
wait is now 30 seconds; two-second socket read/write timeouts, production deadlines
and recovery assertions are unchanged. This test-only correction does not qualify
performance under host I/O pressure. Original failures and final rerun results are
retained separately.

Rendered browser verification is unavailable in this session: browser setup returned
`No browser is available` and discovery returned an empty list. Mock timers do not
qualify real-browser scheduling. Current native/build/test results and the full
Engine-suite outcome are recorded in the
[checkpoint](evidence/roadmap-2026-10-16/browser-preview-freshness/checkpoint.json).

See [browser player](PULSAR_BROWSER_LIVE_PREVIEW.md),
[camera input](PULSAR_BROWSER_CAMERA.md), and
[timestamp provenance](PULSAR_LIVE_TIME_BASIS.md). Full roadmap remains PARTIAL.


## Final scoped results

The final player has 41 passing script-level tests. Protocol has 299 passes and
Client response-storage integration has two passes. Both strict Clippy configurations,
App/Worker builds, formatting, architecture and source guards pass. The final native
CPU/HTTP campaign serves the exact updated HTML and passes the existing packed
inference, continuation, withdrawal, historical replay, corruption refusal and recording
provenance checks. Owned native processes join with empty Worker custody. This native
campaign does not execute browser JavaScript or qualify real browser timers.

The final serial Client library attempt times out at 600 seconds after 428 reported
passes and no reported assertion failures. The corrected recovery-socket case passes
in that run. The remaining library cases were not completed; the separately passing
integration cases do not turn this into a complete Client-suite verdict. The Engine
run remains incomplete at 900 seconds after 312 reported passes. Host I/O pressure
was elevated; its complete cause and broader parallel-load reliability remain open.
No production transport deadline, Engine/Core behavior, qualification gate or model
default was changed. A complete ordinary-filesystem Engine and Client rerun is still
owed; the narrow passing checks do not close those gates.
