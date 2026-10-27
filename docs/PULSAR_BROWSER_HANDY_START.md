# Browser-observed Handy playback

This implementation connects a prepared browser source to explicit playback of an
already prepared Handy CSV script. The browser supplies timing observations; the
Engine computes the original phase/time reference under accepted measurement
bounds. Live diagnostic model estimates are not streamed to the toy by this path.
The 2026-10-23 software checkpoint passes the checks below. No actual Handy has
been contacted or physically qualified by this work.

## Required preparation

The existing Handy setup, script hosting, playback preparation, controller and
clock workflows must already have produced current native authority for the
exact project revision. An accepted device-v6 qualification is required before
importing a separate browser/source dossier. That dossier binds the browser
environment, immutable source and placement, declared source clock, exact master
and prepared script, and measured timing bounds. Its four evidence roles cover
environment/configuration, clock/display, media progression and source
correspondence. Synthetic evidence can remain Pending but cannot receive
production acceptance.

The CLI exposes `handy-browser-start-qualification-import`,
`handy-browser-start-qualification-status`,
`handy-browser-start-qualification-accept` and
`handy-browser-start-qualification-withdraw`. Use each command's `--help` for
the required project, revision, original request and artifact identities.
Acceptance requires explicit operator acknowledgment of measured evidence.
An environment acknowledgment is an operator assertion; a User-Agent string
does not attest the browser executable or its display timing.

## Browser controls

Configure the separate prepared-play selection in Browser live options, then
launch the player and prepare the exact imported source. Video Play starts
analysis and timing only. Inspect the prepared Handy selection, explicitly
acknowledge the current environment and request Handy Start separately.
Use original-request inspection to recover a delayed or lost response. Stop,
pause, source replacement and page closure retire the exact original owner.

The configuration retains the controller acquisition, controller generation,
original clock request, accepted browser dossier revision, and independent
renewal/device-observation intervals. Native credentials and owner capabilities
stay outside JavaScript. Start/history, controller renewal/release and device
timing refresh each have their own worker, so an unanswered Start or device
read does not block Stop.

A configured controller or clock can expire while the source is being prepared.
Such a selection is refused. This version permits one original Start per
configured launch; another controller requires fresh preparation and a fresh
launch. Automatic reacquisition and a simpler setup flow remain unfinished.

## Timing and custody

The Engine maps the entire browser execution bracket with explicit timestamp,
rate, display, source and frame-phase error bounds. It computes an outward-rounded
project-phase interval at the same native instant used for the server-time
reference. Script quantization, selection of a millisecond wire position,
server/device uncertainty and physical execution error are each accounted for.
Both interval endpoints must remain within the source placement and script.

The original wire phase/server-time pair, observation and expiry are immutable.
Queued dispatch retains that pair and checks age and media-rate error again at
claim. New browser samples and controller renewals preserve the native player's
lifetime, but cannot rejuvenate the original decision. Continuing playback
requires independently fresh browser and qualified device timing while the
preceding authority remains valid.

Cancellation is reserved before database-dependent Start handling. A Stop that
arrives first fences that original request; replay cannot create another play.
After claim, retirement preserves stop and outcome custody even when the remote
result is unknown. Inactive reports retain the original timing decision as
history. They cannot recreate current authority.

Journal v9 adds exact browser-decision membership and a reader barrier. Legacy
manual records retain their serialized bytes and hashes. Browser/source evidence
has its own bounded store, and withdrawal invalidates native users even when its
durable receipt cannot be confirmed.

## Verification and remaining gates

The final default checks pass 2,538 cases: Core199, Protocol404, Engine1,275
(71 ignored), Clients558 (556 library plus two integration; 14 ignored) and
JavaScript102. Both strict workspace/all-target lint configurations, application
and worker builds, architecture, source guards and formatting pass. The ignored
native journal fixture export passes separately; it is not included in that total.

Actual executable campaigns pass the v8-to-v9 migration, v8 refusal with unchanged
existing database/WAL bytes including held nonempty WALs, and current-reader reopen.
The populated campaign preserves the original browser decision, record and exact
membership across two reopens. Its Physical, LocalHumanAcceptance and Acknowledged
labels are authored test inputs. It does not exercise successful public replay by
the original actor or restore current authority. An older reader can create a
previously absent empty WAL; that is disclosed separately from byte preservation.

The actual in-app browser runs the authored diagnostic media/model, shows 15
completed analyses and refuses Start with missing native prerequisites even after
all three acknowledgments are checked. Active SIGTERM exits zero and retires
capture, media and timing while the Engine stays alive during browser shutdown.
An earlier rendered attempt exposed a cancellation-only timing error surviving a
confirmed release. Its failing IPC regression, correction and passing rendered
rerun are retained. The rendered campaign does not exercise successful Handy Start.

The [checkpoint](evidence/roadmap-2026-10-22/browser-start/checkpoint.json) records
exact subjects, failed attempts, native results, screenshots and cleanup. The
packet directory uses the work's start date; final verification finished October 23.

Positive playback tests use authored qualification labels, injected device
observations and localhost HTTP. They cannot establish measured physical
acceptance. Useful model quality, real media, arbitrary browser-source binding,
generated HSP point delivery, the 200 ms capture-to-command target, other device
and platform modes, human acceptance and release qualification remain required.
The full architecture-first M1–M8 roadmap remains PARTIAL.

See [ADR B5-053](../research/phase-b/ADR-B5-053-browser-observed-handy-start.md),
[browser timing](PULSAR_BROWSER_TIMING.md) and
[continuous device timing](PULSAR_HANDY_CONTINUOUS_TIMING.md).
