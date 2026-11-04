# Automatic browser live-capture continuation

2026-10-13. Scoped CPU native handoff evidence PASS; full roadmap PARTIAL.

The [local browser preview](PULSAR_BROWSER_LIVE_PREVIEW.md) now prepares a successor
at half of the current capture's admitted lease, once the predecessor is Running.
This removes the normal manual restart at each 30-second lease boundary. Individual
leases are unchanged; each successor requires fresh Engine admission, the original
model/profile/policy and current actor/project/revision authority. The overall
browser server duration remains bounded by its original command options.

The adapter derives the successor's exact project phase from the predecessor's
checked `continuation_options`, opts into `when_ready`, and keeps at most one
pending successor. The Engine owns Ready and atomic retirement. The Client adopts
only a Running successor with the exact predecessor identity and an independently
observed Superseded predecessor. It never changes the old scope's frame identity,
lease or clock into the successor's authority.

The browser retains its original playback identity until pause/seek/Stop. Separate
backend channels and frame sequences feed current and prepared captures. A body
received before a channel's completed clock handshake cannot be stamped through
that channel: it is deferred to a later callback or explicitly dropped during a
switch. A normal predecessor-retirement refusal may be reconciled only after the
Engine confirms the matching prepared handoff; ambiguous transport failures fence
the run. The adapter does not use absence, a timed wait or a locally guessed Ready
as handoff evidence. Old estimates never acquire the new backend's identity.

Stop attempts retirement of both pending and current scopes and preserves any
unconfirmed ownership. Pause/seek, input stall, page stop and normal server expiry
share that behavior. Preparation, authority, resource or lease failures are visible
and stop the preview; no alternate model/provider or silent new Start is chosen.
Two overlapping workers require sufficient admitted capacity. This is automatic
handoff, not a guarantee of uninterrupted estimates or latency under every model.

## Verification

Final Client suites pass 410 tests with ten ignored. Both strict workspace Clippy
configurations, Windows source check (existing warnings), App/Worker/controller
builds, formatting, architecture and 36 guards pass. Four mock-DOM player tests
pass, including unchanged browser playback identity when the backend identity
changes. The initial copy-state lint failure is retained with the correction.
Engine/Core/Protocol sources match the preceding browser checkpoint; no fresh
library suite claims extend that evidence.

The real native loopback server feeds the authenticated Engine and confined CPU
worker for 50 seconds at paced input cadence. It observes three pending successors,
three confirmed handoffs and four backend captures with the same browser playback
identity. Each derived source/project anchor is checked against the original
mapping; estimates retain the current backend identity and match the independent
constant-RGB scalar graph oracle within 0.0001. This authored graph is not a trained
motion model or real-media quality test. Historical captures stay inactive; current
Stop, new seek epoch, stale-playback refusal, three-second input stall and normal
server exit during continued input pass. Worker custody is empty, both owned
processes are joined, and no ordinary jobs/candidates/exports are created.

No rendered browser, real browser video decoding, arbitrary-tab integration,
streamed funscript recording, actual CUDA overlap, high-latency/failed preparation,
all resource/authority races, calibrated browser/device timing or physical Handy
qualification is asserted. The receipt-time clock still excludes browser capture
and transfer delay. Useful model/media, six-axis, platform/edition, human and
release acceptance remain open.

Launch syntax is unchanged; see [the preview instructions](PULSAR_BROWSER_LIVE_PREVIEW.md).
See [ADR B3-186](../research/phase-b/ADR-B3-186-browser-live-continuation.md)
and [checkpoint](evidence/roadmap-2026-10-13/browser-live-continuation/checkpoint.json).


## Subsequent rendered in-app browser evidence

[Rendered browser QA](PULSAR_BROWSER_RENDERED_QA.md) now exercises selected local
video/controls/feedback/handoff scenarios through the real in-app browser and
Engine on unchanged source. The original checkpoint's unverified rendered-browser
scope remains historical; broader browser/native-GUI/model/device/platform gates
remain open.
