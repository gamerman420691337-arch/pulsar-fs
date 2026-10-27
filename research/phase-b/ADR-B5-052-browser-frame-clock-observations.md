# ADR B5-052 — Browser frame-clock observations before playback

Measure a browser callback's execution interval before Handy playback exists.
The existing stop guard requires an acknowledged Playing controller and cannot
provide the timing input needed to start that controller. A separate native
observation lifetime supplies this input without weakening the guard's contract.

An authenticated Open binds one exact browser player, imported source preparation,
active placement, declared source clock and project revision. The Engine persists
the original Open report before installing a process-local source pin. Replaying
that request or inspecting its history never reconstructs a native observation.
Current status requires the exact retained native entry. Fast probe, observation,
status and release operations use cached authentication and native source custody,
independently of inference and the main database.

The Engine records native BOOTTIME before releasing an unpredictable probe. After
receiving that probe, the browser waits for a fresh eligible video-frame callback
and samples `performance.now()` inside it. It sends that sample together with the
callback timestamp, media time, presented-frame count and any composition/display
metadata. The Engine consumes the probe once and records complete ingress time.
The full native issue–receive interval bounds execution of this exchange's browser
sample. A later timer callback paired with an older cached frame is insufficient.
The browser values remain reports from the selected page, not attestations of
pixel origin or physical display time.

`BrowserClockBracket` retains both endpoints. Its conditional mapping kernel uses
explicit bounds on timestamp error, relative clock rate, age and interval width.
Signed rational interval arithmetic rounds outward, including reciprocal rate
bounds; it never assumes symmetric network delay or substitutes half the round
trip. Those input assumptions require separate substantiation. The raw exchange
does not establish browser timer precision, suspend behavior or display latency.

The existing declared source clock maps the reported media timestamp to exact
project phase. This preserves source identity and rational placement offsets while
keeping source correspondence explicitly declared. Callback execution, compositor
submission, predicted display and actual screen emission remain distinct facts.
The browser specifications do not turn a predicted display timestamp into a
measured physical display event; see the [timing research](browser-player-timing-and-liveness-2026-10-22.md).

One pending probe and increasing sample/frame/phase values prevent a delayed
message from refreshing an older observation. Probe issuance alone does not renew
its lifetime. Expiry begins at issue, not response receipt. Pause, seek, hidden
state, source changes, native expiry, revocation and shutdown retire the exact
observation lifetime. Capture continuation may preserve it because the browser
player and source placement remain unchanged. Late replies cannot install into a
replacement player. Bounded workers and native expiry keep retirement independent
of a blocked preview or missing browser lifecycle notification.

This decision adds no device effect. Synchronized start still needs accepted
browser/source/display bounds, exact script correspondence and a phase/server-time
reference derived from the same native anchor. That owner must remain valid
through queued and acknowledged playback. Generated HSP streaming additionally
needs future-point admission, buffer ownership and ambiguous-append recovery.
Useful models, real media, platform, physical-device and human/release acceptance
remain requirements of the full roadmap.

Scoped software, native HTTP and rendered authored-fixture checks pass. The
[feature guide](../../docs/PULSAR_BROWSER_TIMING.md) and linked checkpoint retain
exact subjects, initial failures, live screenshots and joined process evidence.
The conditional mapping kernel preserves uncertainty even when distinct events
report the same coarse tick. Confirmed retired client records are reclaimed only
after worker and response custody settle; unknown outcomes remain visible. This
connected measurement increment leaves the full roadmap partial.
