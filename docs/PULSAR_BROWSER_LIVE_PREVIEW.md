# Local browser live-motion preview

Current successor, 2026-10-22: [scoped player control and optional Handy stop guard](PULSAR_BROWSER_PLAYER_CONTROL.md). Independent heartbeats, exact page/player identity and native retirement supplement this preview workflow. Default launch has no device linkage; the guard does not start or synchronize playback.

2026-10-13. Implemented Client adapter and native HTTP/Engine evidence; rendered
browser scenarios now have scoped QA; physical playback remains unverified. Full roadmap PARTIAL.

The new `browser-live` command serves a loopback-only player for a locally selected
video and sends packed 64×64 RGB frames to the existing authenticated Engine live
capture interface. It displays provisional Stroke estimates. [Experimental explicit tab sharing](PULSAR_BROWSER_TAB_CAPTURE.md) now has a player input path; actual browser/platform acceptance and source-player timing remain open. It sends no toy commands. Subsequent [committed script download](PULSAR_BROWSER_SCRIPT_DOWNLOAD.md) adds explicit gap-free master export. Optional [recording controls](PULSAR_BROWSER_RECORDING_CONTROLS.md) now save accepted estimates as JSON.
An explicit checked temporal model/profile remains required; no default model is
selected and the authored diagnostic graph is not a useful trained model.

## Launch

Use the current development executable, not the older installed shortcut:

```text
/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar browser-live PROJECT REVISION /absolute/live-options.json
```

`PROJECT` is an existing project ID and `REVISION` its current revision. The
options file has the same `LiveCaptureOptions` JSON shape as `live-capture start`:
explicit `inference.model_path` and pinned `inference.profile`, normal RGB input,
immediate continuation, freshness policy, clock anchors and a lease up to 30,000
ms. Use `pulsar model profile MODEL --width 64 --height 64 --class-count 1
--temporal-script-position` to inspect an actual compatible model against the
Engine runtime. A detector profile is not interchangeable with this temporal ABI.
The Engine must already have the declared ONNX runtime available. Production
model/profile and CUDA owner admission remain authoritative.

Open the printed `http://127.0.0.1:PORT/` URL. Select a local video and press Play.
The player currently supports normal playback speed. Pause, seek, video change,
rate change, end and decoding errors clear the position preview and stop its
capture. After seeking, press Play to start a fresh capture at that source time.
The page retains no media file on the server; frames remain bounded live inputs.

The server runs for 300 seconds by default (`--duration-seconds 1..600`); `--port`
can select a fixed port. Individual Engine captures retain their original lease,
up to 30 seconds. [Automatic prepared continuation](PULSAR_BROWSER_LIVE_CONTINUATION.md)
now admits a successor at half-lease, keeps the current worker until confirmed
Ready, and retains the same browser playback identity. Each successor has fresh
admission and its own unchanged deadline; insufficient capacity or failed
preparation stops the preview. The original initial checkpoint below did not
include this later continuation behavior.
Closing the page requests Stop; three seconds without a received frame also
requests Stop, and normal server expiry stops the held capture. Lost Engine
Start/Stop outcomes fence replacement rather than silently creating new work.
Engine lease expiry remains the fallback after abrupt server death.

## Timing and state boundaries

The page uses browser video-frame callbacks and their `mediaTime` presentation
stamp, rounded to microseconds. The API is a best-effort browser/compositor
observation, not physical-device clock calibration. See the [browser API
reference](https://developer.mozilla.org/en-US/docs/Web/API/HTMLVideoElement/requestVideoFrameCallback).
The player stretches the selected video into the declared 64×64 RGB input with
image smoothing disabled and keeps at most one outstanding frame request. Busy
frames are skipped rather than accumulating a backlog, while their presentation
timestamps still update the display freshness check. It suppresses late replies
from an old playback generation. [Preview freshness](PULSAR_BROWSER_PREVIEW_FRESHNESS.md)
also accounts for elapsed time since pixel sampling and expires the displayed
estimate without waiting for another frame or network response. Browser timer
scheduling is best effort; this is not a physical-device deadline.

The Client owns the native pixel channel and Engine credentials; credentials
never enter the page. Frames are hashed by the native bulk client. The local
monotonic stamp describes receipt by the adapter, not original browser capture.
The displayed HTTP round trip is not capture-to-command latency. HTTP accepts
only the exact loopback Host, same-origin binary POST routes, bounded headers and
bodies, no chunked transfer, and bounded input/output waits. Foreign origins and
ambiguous lengths refuse before Engine work; early unread-body refusal may close
the connection. This is one local player per server, not multi-tab isolation.

## Live CPU launch correction

The first native inference attempt failed with `cgroup configuration exceeds
admitted worker claim`. Live staging reserved one CPU thread, then transferred
that claim into a worker configured for two. The newly enforced cgroup guard
correctly refused the mismatch. Preparation now takes a named budget and reserves
the eventual CPU launch memory/thread limits before copying. CPU inference and
encoded-source callers carry their admitted worker limits; GPU preparation keeps
one CPU thread and retires that claim before its separate GPU handoff. Resource
limits and the cgroup guard are not weakened.

The original failing native HTTP/Engine loop is retained with exact preceding
source/binary identities and WorkerFailed readback. The final native loop succeeds
against the rebuilt Engine. Fresh native encoded-source/GPU campaigns were not
run here; their source and all-feature compilation checks do not establish runtime
qualification.

## Verification and remaining work

Final checks pass 878 Engine library tests (70 ignored), 410 Client tests (ten
ignored), both strict Clippy configurations, Windows source compilation (existing
warnings), App/Worker/controller builds, formatting, architecture and 36 guards.
Protocol/Core sources are unchanged; no fresh suites are claimed. Three mock-DOM
player tests pass for RGB/timestamp delivery, bounded requests/late-reply fencing,
and stale-estimate suppression. The initial stale-preview regression failed and
its preceding page plus passing fix are retained. These are not rendered-browser
or actual browser video-decoding evidence.

The native Linux/private-RAM campaign uses the real loopback server, Engine,
confined CPU model and pixel transport. A 302-byte authored scalar graph estimates
0.375488758 against the independent 0.375490196 oracle with tolerance 0.0001.
Normal Stop, a new source anchor/epoch after seek, stale-epoch refusal, three-second
input stall and normal server exit during active frame delivery all stop their
captures. Worker custody is empty. The campaign creates one project and no
ordinary jobs/candidates/exports. Browser and Engine processes are joined.
Initial harness name-shadowing and early-origin-refusal connection-reset failures
are retained separately from the Engine resource-claim defect.

Rendered browser QA was unavailable because isolated browser MCP tools were
absent. No host browser/desktop or physical device was operated. Arbitrary tab
capture/player integration, broader warm-continuation failure/performance coverage, motion recording/export,
measured browser/presentation/device correspondence, real-model usefulness and
Handy playback remain required. Browser development preview does not close live
six-axis, 200 ms latency, model/media/platform/edition/human or release gates.

See [ADR B3-185](../research/phase-b/ADR-B3-185-browser-live-preview.md)
and [checkpoint](evidence/roadmap-2026-10-12/browser-live-preview/checkpoint.json).


## Subsequent rendered in-app browser evidence

[Rendered browser QA](PULSAR_BROWSER_RENDERED_QA.md) now exercises selected local
video/controls/feedback/handoff scenarios through the real in-app browser and
Engine on unchanged source. The original checkpoint's unverified rendered-browser
scope remains historical; broader browser/native-GUI/model/device/platform gates
remain open.
