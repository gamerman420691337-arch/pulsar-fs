# Experimental browser camera input

The local browser player accepts camera video through **Use camera**. Launch it
from the desktop's **Browser live** tab or the existing `browser-live` CLI, with
an explicit compatible model/profile or owner-selected pack. Enable recording
before choosing the camera if accepted estimates should be retained. Camera
selection starts the same bounded RGB inference path as local video and shared
tabs. It does not select a useful model or enable device playback.

The button requests video only, with an ideal 640×480 size and 15 fps, capped at
30 fps in the request. These are acquisition constraints, not measured throughput.
Exactly one video track and no additional tracks are accepted. Camera device IDs
are neither read nor retained. The preview is muted. Frames are reduced to 64×64
RGB; only one frame request may be outstanding. Existing Engine admission,
freshness, finite leases, pack selection and cancellation rules still apply.

Camera and tab permission requests share one pending selection. Stop, replacement
input or page exit invalidates a pending result; a late stream is closed without
starting analysis. Late rejection and old track events cannot stop replacement
input. Denied permission, unavailable tracks, failed playback or missing frame
callbacks cannot dispatch frames. Track mute/end, source replacement, Stop and
page exit release owned tracks and clear the displayed position. Resuming capture
requires a new explicit selection. Page-exit Stop remains a request, not a server
acknowledgement or physical-device stop.

`camera_capture_presentation` is a separate producer-declared time basis. It
describes browser video presentation, not sensor exposure, calibrated wall time
or an external video's timeline. Continuation preserves it; recording assembly
refuses mixed camera/tab/source-media domains. Encoded-media input cannot claim
browser-capture time. Recording pages and durable candidate source proof retain
the declaration. Camera candidates receive `live_capture_time_not_media_time`
review diagnostics, just as shared-tab candidates do. Review, commitment, gap
repair and export remain separate explicit actions.

This advances camera-input integration for IN-08/LIVE-01 and shared-client
behavior in B4/B6. IN-08 and LIVE-01 remain incomplete because they also require
synchronized device output. Actual browser permission dialogs, real webcam acquisition, platform
compatibility, motion accuracy, capture-to-command latency, physical Handy/six-axis
output and human/release acceptance remain unverified. The full roadmap is PARTIAL.

See [timestamp domains](PULSAR_LIVE_TIME_BASIS.md),
[quickstart](PULSAR_APP_QUICKSTART.md),
[decision](../research/phase-b/ADR-B3-214-browser-camera.md), and the
[verification checkpoint](evidence/roadmap-2026-10-14/browser-camera/checkpoint.json).


## Verification recovered on 2026-10-16

Current source passes 299 Protocol tests, 434 Client tests (eleven ignored), 31
browser-player mocks, native App/Worker builds, strict Client and workspace/all-feature
Clippy, formatting and architecture/source checks. The source inventory covers 659
Rust/HTML/JavaScript/Cargo files and is unchanged across the run. Core was not freshly
tested. Mock capture verifies explicit video-only selection, pending-request exclusion,
late consent/rejection, track loss, source replacement, failed playback and page exit.

The private native CPU/HTTP campaign supplies authored RGB pixels with camera presentation
time. The diagnostic scalar is 0.375488758, within 0.0001 of its independently computed
oracle. Prepared continuation preserves camera provenance across two recordings. Pack
withdrawal retires analysis; stale and corrupted choices refuse fresh admission. Original
Start replay remains inactive. Historical recording staging preserves camera/pack provenance
in durable source proof and emits the capture-relative review warning. One project and one
uncommitted candidate remain inside the isolated test; no ordinary jobs or exports are
created. Owned browser/Engine processes join and Worker custody is empty. This supplies
no actual webcam, rendered-browser, useful-motion or physical-device evidence.

The fresh full Engine library run **did not complete**: its supervisor raised
`subprocess.TimeoutExpired` at 900 seconds after 341 reported passes and no reported
assertion failure. A test thread was observed in `jbd2_log_wait_commit` during substantial
host I/O pressure; the complete cause is not established. Its process group is terminal.
The prior retained 882-pass run is historical, not a substitute for this incomplete rerun.
A fresh completed Engine suite is still owed. Initial page-exit feedback and earlier
suite-timeout failures remain in the evidence packet. Full roadmap and qualification
remain PARTIAL.
