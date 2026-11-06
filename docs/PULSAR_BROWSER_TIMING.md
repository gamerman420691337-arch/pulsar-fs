# Browser frame timing observations

Browser live can measure a fresh frame callback before Handy playback exists.
It retains the complete native issue-to-response interval and the frame's exact
declared project phase. This is an input to later synchronized-start planning;
it does not start a device or qualify display, source or physical timing.

## Use it

1. Launch Browser live with an explicit model/profile and prepare an imported
   source and placement using the [source-media workflow](PULSAR_BROWSER_SOURCE_MEDIA.md).
2. Press the video's Play control. Timing measurement starts for that prepared
   player independently of inference and optional Handy monitoring.
3. Read **Measured callback execution bracket width** and **Declared project phase**
   below the video. The width includes the entire native challenge exchange.
   The phase is shown as an exact fraction; native clock values are not rounded
   through JavaScript numbers.
4. Use **Inspect original timing status** to inspect the original operation.
   Inspection does not restart a measurement loop or renew an observation.
5. Pause or seek to retire that player's timing. Press Play explicitly to create
   a fresh player. Preparing a replacement source, closing the page, native
   expiry and shutdown also retire the original observation.

File-picker, camera and shared-tab sources remain unbound and do not acquire this
source-specific timing observation. Capture continuation preserves timing when
the browser player and prepared source remain the same. A failed exchange shows
timing as unavailable; it does not create a substitute calibrated clock.

## What is measured

The Engine timestamps issue of a one-use probe using BOOTTIME. After receiving
the probe, the browser waits for an advancing video-frame callback and samples
`performance.now()` inside it. The response retains that sample, the callback
timestamp, media timestamp and presented-frame count together. Composition
submission and predicted-display metadata remain distinct optional values.

The Engine timestamps complete ingress and retains both endpoints. It never
halves the round trip or assumes symmetric delay. The exact prepared source's
declared clock and active placement map the reported media timestamp to project
phase, including fractional and negative project anchors and half-open trim.

`BrowserClockBracket::bounds_at` can map another browser timestamp only under
explicit external timestamp-error, clock-rate, age and interval-width assumptions.
It rounds interval bounds outward. Even equal reported timestamps retain both
timestamp errors: two distinct events can have the same coarse tick. The separate
`sample_bounds` accessor exposes only the known execution's retained raw interval.
Neither API infers those assumptions from a successful exchange.

## Ownership and recovery

The native Open requires Read and Export access to the exact prepared source.
Its original report is saved before a process-local source pin is installed.
Original-request replay and `BrowserTimingOpenStatus` inspect that history; after
retirement or restart they cannot recreate an observation or source pin.

Probe, observe, current status and release use a separate bounded client worker
and the Engine's fast control path. They do not wait for Preview or the project
database. One current probe and increasing observations prevent stale replies
from renewing or retiring a successor. Expiry starts at issue, not receipt.
Clock reversal retires timing; public source/grant/revision retirement reaches
the retained source pin. Native expiry remains effective without a page-close
message. Confirmed client retirements can be reclaimed; unresolved outcomes stay
bounded and visible by original request ID.

Current reports carry `qualified: false`. They do not authorize Handy playback,
attest the submitted pixels, establish browser/source calibration, or measure
physical display/device response. A synchronized start still needs accepted
timing bounds and exact script correspondence. Generated HSP streaming, useful
model quality, real-media/platform/device checks and human/release acceptance
remain open in the full roadmap.

## Validation

The 2026-10-22 scoped checkpoint passes Core 194, Protocol 396, Engine 1,241
(70 default ignores), Clients 549 (14 default ignores), and 95 JavaScript checks.
Both strict lint configurations, builds, architecture/source guards and normal
and explicitly included formatting checks pass.

The separately executed native HTTP campaign observes 43 timing exchanges across
a prepared capture handoff and three player lifetimes. Exact phase and the full
bracket match the native reports; pause preserves prepared reads, history remains
inactive, replacement rejects old scopes and all owned processes join. This
campaign supplies authored callback facts, not rendered-browser evidence.

Actual in-app-browser playback observes 230 timing exchanges across four player
lifetimes, two capture handoffs and 683 accepted diagnostic estimates. The two
full 30-second trims preserve the declared three-quarter-second project offset.
Explicit pause followed by history inspection remains inactive. A final active
playback receives SIGTERM: the browser exits zero, the Engine stays alive during
that shutdown, and all six captures, four timing bindings and the single media
preparation are confirmed inactive. The temporary browser tab is closed.

The [checkpoint and retained attempts](evidence/roadmap-2026-10-22/browser-timing/checkpoint.json)
include initial harness/fixture failures and the final subjects. The
[live screenshot](evidence/roadmap-2026-10-22/browser-timing/rendered-campaign/live-final.png)
shows the authored scalar diagnostic, not useful model accuracy. These results
do not establish the 200 ms capture-to-command target, calibrated display/source
correspondence, physical Handy behavior or full roadmap completion.

See [ADR B5-052](../research/phase-b/ADR-B5-052-browser-frame-clock-observations.md)
and the [current roadmap status](PULSAR_CURRENT_STATUS.md).
