# Browser analysis into a live Handy HSP stream

Pulsar has a bounded native path from accepted CPU analysis of prepared browser
media into an HSP stream. The browser runs a visible player and a second analysis
reader ahead of it. Explicit native Bind, Play and Stop requests control the
stream. Browser Play starts media, timing and inference; it does not start the toy.

This implementation requires separate accepted source/presentation evidence and
physical Handy HSP evidence. Those production decisions have not been created by
the software fixtures. Useful model output and physical Handy 2 Pro behavior
remain unqualified. The full roadmap remains partial.

The [current rendered software campaign](evidence/roadmap-2026-10-23/hsp-continuity/rendered-browser-hsp-ui-018/completion.json)
connects the actual browser, confined CPU inference and a localhost HSP simulator.
Its public controls complete a 40-second neutral video, deliver 309 samples across
three capture epochs, preserve one stream through two source transfers, and finish
the final tail with one acknowledged Stop. Shutdown joins all workers; reopening
the Engine restores history without native authority or temporary test permits.
The model and device contract are authored fixtures. Rendered fault cases and
physical qualification remain open; see [current status](PULSAR_CURRENT_STATUS.md).

## Prepare the source and two readers

Use the [imported-source workflow](PULSAR_BROWSER_SOURCE_MEDIA.md) to import media,
create an active unit-rate placement and prepare the exact source in Browser live.
The [live-options workflow](PULSAR_DESKTOP_LIVE_OPTIONS.md) supplies an explicit CPU
model/runtime profile. This first source contract supports the 64×64 temporal
RGB-frame-pair recipe: nearest-neighbor stretch, six NCHW channels and the declared
normalization. File-picker video, cameras, shared tabs and provisional CUDA results
do not supply this connected source authority.

The existing launch command is:

```sh
pulsar browser-live PROJECT REVISION /absolute/live-options.json
```

After preparing the Engine media and explicitly declaring its clock anchors:

1. Select **Analyze ahead using prepared Engine media** and set **Requested lead
   (ms)**. The UI accepts 1–15000 ms; it requests a minimum of half the target and
   a maximum of twice the target. The accepted dossier must match this exact
   policy, and its limits must fit the accepted device policy.
2. Press the visible video's Play control. The second reader loads the same
   prepared URL and seeks ahead within the original trim. It supplies actual
   `requestVideoFrameCallback` media timestamps and RGB pixels from that reader.
   Requested lead alone does not prove that those frames were decoded ahead.
3. Wait for the visible player's current native timing observation and accepted
   CPU output. The page shows the observed analysis lead. Ahead-of-player output
   is retained separately until the visible playhead reaches its source position;
   it is not immediately displayed as the visible player's current estimate.

Visible preview retains at most 1,024 future estimates in source-time order. It
promotes the newest due, unexpired estimate while preserving later entries and all
original deadlines. Duplicate results, polling and promotion cannot refresh their
age. Pause, seek or feed replacement clears the queue. Queue exhaustion retires the
feed instead of overwriting work that is still ahead of the visible player.

Choose a lead that leaves margin within the original analysis-age budget. In the
neutral rendered fixture, 500 ms lead with a 1,000 ms age budget displayed current
positions; equal 1,000 ms settings reported insufficient freshness margin. The UI
reports waiting, expiry or insufficient margin explicitly. This is a preview
configuration example, not a qualified device timing recommendation.

The native analysis feed binds its opaque feed ID, original capture epoch, player,
timing binding, source and requested policy. The producer checks the actual current
source and timing pins. The controller independently checks lead against visible
phase; a producer declaration or JSON report cannot construct native authority.

Pause, seek, rate change, hidden-page retirement, source replacement, lost timing,
invalid lead or epoch expiry retires the feed. Changing lookahead settings requires
an explicit new playback/feed. Selecting **Continue analysis for longer videos**
admits the bounded source-transfer workflow; native checks retain the original
source, player, placement, clock and inference profile across each transfer.

To expose HSP controls, launch with an explicit HSP selection and enablement:

```sh
pulsar browser-live PROJECT REVISION /absolute/live-options.json \
  --handy-hsp /absolute/handy-hsp.json --allow-handy-hsp-control
```

The selection names the native Engine/device session, exact device and source
qualification bindings, preparation timeout, optional finite-drain policy hash
and optional continuity duration. It carries neither credential values nor
permission to bypass native qualification. The same first-party Engine session
must own the browser capture and preparation; an unrelated paired client cannot
claim another actor's capture.

After video playback supplies current timing and actual accepted analysis:

1. Click **Inspect HSP selection** and check the displayed device/source details.
2. Consent to synchronization, then click **Prepare synchronization**.
3. Confirm the displayed browser environment and online-control choice, then click
   **Bind current analysis** while the original readiness remains valid.
4. If continuity is configured, consent and click **Enable bounded HSP continuity**.
5. Give separate Play consent and click **Play HSP stream**. Video Play alone does
   not issue device Play.
6. Use **Stop ORIGINAL HSP stream**, **Cancel ORIGINAL preparation** and the
   original-status controls to operate or inspect the retained lifecycle.

The **Prepared Handy browser Start** section remains the separate hosted HSSP
workflow. For the synthetic development demo, use the
[bounded rendered campaign instructions](evidence/roadmap-2026-10-23/hsp-continuity/rendered-tools/README.md).
Its temporary test permissions do not configure or qualify a real Handy.

## Keep source phase separate from sample age

The source's explicit `DeclaredSourceClock` and active placement map each actual
analysis frame to project time. An independent visible-player timing observation,
the accepted browser timing bounds and the retained preparation clocks establish
a conservative visible phase interval. The controller compares each future point
against that interval and the dossier's minimum/maximum lead. Inference completion
and frame receipt do not set visible phase.

For this browser recipe, the native client declares its HTTP frame-receipt tick.
The capture-clock bridge retains the original tick's native correspondence; it
does not turn that producer declaration into an observed exposure timestamp. The source dossier
therefore declares a separate measured bound,
`maximum_capture_to_engine_receipt_ns`. Native age checking subtracts that entire
bound from the earliest receipt time. It also retains the actual acceptance time
and enforces the existing sample-age limit. The checked sum of this allowance and
`maximum_analysis_age_ns` must fit the accepted HSP sample-age contract.

Neither a short HTTP round trip nor fresh inference establishes the missing
capture-to-receipt bound, source correspondence, display timing or physical response.
See [browser timing](PULSAR_BROWSER_TIMING.md) and
[native capture clock correspondence](PULSAR_LIVE_CAPTURE_CLOCK.md).

## Qualify the exact source contract

The source dossier is separate from the physical
[schema7 HSP device dossier](PULSAR_HANDY_HSP_RECOVERY.md). Source subject version 1
binds that exact accepted hardware decision, control-image/context, browser
environment artifact, project revision, source/placement and clock declaration,
complete inference profile, RGB recipe, age basis, lookahead and timing limits.

It retains four bounded evidence roles: `environment_configuration`,
`clock_and_display`, `media_progression` and `source_correspondence`. Review must
cover the actual analysis/visible correspondence, advancing media and lead, the
browser/display assumptions and the capture-to-Engine receipt bound. Structural
validation and retained hashes do not establish those measurements. Model accuracy
and useful motion quality remain separate acceptance work.

Import saves a Pending dossier. Acceptance is an explicit human review action:

```sh
pulsar handy-hsp-source-qualification-import PROJECT /absolute/source-manifest.json \
  --revision PROJECT_REVISION --manifest-sha256 MANIFEST_SHA256 \
  --request-id IMPORT_REQUEST

pulsar handy-hsp-source-qualification-status PROJECT SOURCE_DOSSIER_ID

pulsar handy-hsp-source-qualification-accept PROJECT SOURCE_DOSSIER_ID \
  --revision PROJECT_REVISION --record-revision PENDING_RECORD_REVISION \
  --operator REVIEWER --request-id ACCEPT_REQUEST \
  --acknowledge-measured-evidence
```

Use actual retained IDs and reviewed evidence. Synthetic source evidence cannot
receive production physical acceptance. The test-only native permit leaves its
durable report Pending/Synthetic and disappears with the connection; it is not a
production CLI feature. Import and acceptance start no device playback.

Withdrawal preserves evidence and retires dependent native source pins:

```sh
pulsar handy-hsp-source-qualification-withdraw PROJECT SOURCE_DOSSIER_ID \
  --record-revision CURRENT_RECORD_REVISION --reason "measured applicability ended" \
  --request-id WITHDRAW_REQUEST
```

A later review cannot revive an old pin. Source, placement, project revision,
registration, device acceptance, grant and Engine-lifetime changes also invalidate
dependent use. Final admission revalidates the exact source under the already-held
Engine database guard; source reports alone are descriptive history.

## Bind, inspect, explicitly Play and Stop

First obtain current Ready from the
[original HSP preparation workflow](PULSAR_HANDY_HSP_PREPARATION.md). That operation
retains the device/server clock and its original deadline; it does not enable
playback. The browser capture, preparation and control commands must share their
original actor, project and revision.

Take the capture epoch, current inference attempt and browser timing binding from
the actual native reports for the active analysis feed. The source qualification
revision is its accepted decision revision. These are opaque selections, not values
to derive from a filename or fabricate from a saved status. The Bind request ID
becomes the stream request ID:

```sh
pulsar handy-hsp-live-bind PROJECT --revision PROJECT_REVISION \
  --preparation-request-id ORIGINAL_PREPARATION_REQUEST \
  --capture-epoch CAPTURE_EPOCH --inference-attempt-id INFERENCE_ATTEMPT \
  --browser-timing-binding-id BROWSER_TIMING_BINDING \
  --source-qualification-id SOURCE_DOSSIER_ID \
  --source-qualification-revision ACCEPTED_SOURCE_REVISION \
  --request-id STREAM_REQUEST --allow-online-control \
  --acknowledge-current-environment

pulsar handy-hsp-live-status PROJECT --stream-request-id STREAM_REQUEST

pulsar handy-hsp-live-play PROJECT --revision PROJECT_REVISION \
  --stream-request-id STREAM_REQUEST --request-id PLAY_REQUEST

pulsar handy-hsp-live-stop PROJECT --stream-request-id STREAM_REQUEST \
  --request-id STOP_REQUEST
```

Keep all four preparation/stream/Play/Stop original request IDs distinct. The client
reads the existing private owner capability; credential values are never command
arguments. Bind requires both `--allow-online-control` and
`--acknowledge-current-environment`. The latter records the operator's confirmation
that the current browser, player and display configuration matches the
`browser_environment` evidence in the exact selected source dossier and accepted
revision. This is an applicability assertion, not automated attestation or new
qualification. It is never inferred from evidence acceptance or online control
consent.

Bind explicitly authorizes online stream control, consumes current preparation
readiness once and allows Setup/prefill. It does not send Play. Play requires its
own explicit original intent and current authority; Stop does not require a newer
project revision.

Accepted CPU results pass directly from native analysis completion into the owned
controller after producer locks are released. Status polling supplies no points.
The first actual accepted sample selected at binding fixes the stream's immutable
zero and retains its real inferred position. Later source times are relative to
that original coordinate. There is no invented leading hold, replacement first
point or later rebase to make stale work appear fresh.

The discrete envelope checks the quantized millisecond/percent points against the
accepted position range, adjacent step, minimum segment and exact source continuity.
Its previous point survives across batches. Invalid or discontinuous output retires
authority; the controller does not clip, interpolate or skip a rejected edge to
continue. These checks are not a proof of continuous physical dynamics or model
quality. Setup, Append, explicit Play and Stop use the existing serialized HSP owner
and durable claim/uncertainty rules.

Status retains the selected subject and original first-sample witness, plus current
native control IDs, samples accepted into the native queue and underlying stream observation when
available. `qualified: false` remains explicit: the response itself never certifies
physical playback. A transport acknowledgment is not a physical motion or stopping
measurement.

## Original lifetime, replay and restart

This first connected lifetime is one capture epoch of at most 30 seconds. Its
effective authority may expire sooner because of original preparation clocks,
source/timing validity, sample freshness or any dependency change. Reading status,
replaying Bind or waiting for metadata does not renew any of these budgets.

Retry a lost response only with its original request ID and unchanged arguments.
Bind reserves the original actor/request/fingerprint before consuming readiness.
Replay reports that operation; it cannot redo admission or select newer samples.
Play captures its original native admission time before persistence, and replay
does not refresh it. An uncertain command is preserved under the existing owner
and journal rules rather than resent as a fresh attempt.

Stop retires the original source/controller and requests the existing owned Stop
path. Native expiry and shutdown can also initiate retirement. Stop metadata,
transport acknowledgment and physical custody resolution remain distinct. The
preparation-to-child link and first Setup claim preserve the existing journal v12
custody transfer; exact [HSP recovery](PULSAR_HANDY_HSP_RECOVERY.md) handles unresolved
history without restoring playback authority.

Restart retains request/source/origin and journal history. It reconstructs no live
source pin, browser timing, prepared clock, delivery subscription or stream owner.
Fresh work needs new explicit native dependencies after conflicting custody is
resolved. The analysis reader can reach trim end before the visible reader; tail
draining and sustained cross-epoch playback are not qualified by this increment.
There is no automatic continuation or silent refresh into a second stream.

## Evidence and open work

The [HSP live-source checkpoint](evidence/roadmap-2026-10-23/hsp-live-source/checkpoint.json)
is the authority for source hashes, focused checks, integration/native campaigns,
retained failures and final disposition. This guide does not promote passing core
or authored JavaScript tests into an integrated browser/device result.

Actual rendered-browser qualification of the connected flow, measured source and
display correspondence, useful trained motion, Handy 2 Pro firmware/device checks,
physical synchronization and stopping, sustained latency, tail behavior and a
long-running user workflow remain open. So do the remaining platform, six-axis,
human and release gates in the full roadmap.

See [ADR B5-061](../research/phase-b/ADR-B5-061-browser-hsp-live-source.md).
