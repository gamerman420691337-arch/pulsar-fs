# Browser player control and an optional Handy stop guard

The browser server now separates player control from inference and editor work. Every
page and player has a scoped identity; queued work from an old player cannot publish into,
stop or renew a replacement. An optional native guard can monitor an already acknowledged
Handy play and retire it when its selected browser player stops or loses liveness.
Integrated software and native HTTP validation are recorded below. No actual Handy or
rendered-browser qualification is established by this document.

## Use the default player

Launch the browser player through the desktop **Browser live** tab or the existing
`browser-live PROJECT REVISION OPTIONS` CLI command. Choose media and press Play explicitly.
A page load creates a page capability but does not replace an active player. A second page
must wait until the current player has stopped. Capabilities remain in memory and are scoped
to this server launch; Engine session credentials and Handy owner proof never enter JavaScript.

Pause, seek, end, unsupported rate, hidden/pagehide state and explicit Stop retire the exact
local player. Becoming visible or receiving a late response does not rearm it. An old queued
Start/frame/editor request or pagehide cannot affect a replacement player. After stopping,
the original page may inspect and recover its retained editor/recording work until another
player replaces it. Creating a page alone does not retarget existing work.

The control requests use their own fetch lane. A bounded native HTTP dispatcher parses and
writes at most 16 connections; a slow partial request or response cannot occupy the whole
server. Preview remains one owned worker with four queued jobs and at most one outstanding
frame. Scope checks occur before dispatch and before publishing results. Frame ingress time
is preserved through the queue; queue delay is not relabelled as a fresh frame.

## Attach a native Handy stop guard

This requires an already Playing controller with an acknowledged original play, current
qualification/configuration and valid device timing. It does not start the device. Prepare a
JSON file containing only the exact selection and liveness policy:

```json
{
  "selection": {
    "acquisition_request_id": "ORIGINAL_ACQUISITION",
    "controller_id": "CONTROLLER_ID",
    "expected_generation": 0,
    "play_request_id": "ORIGINAL_PLAY_REQUEST",
    "play_record_id": "ORIGINAL_PLAY_RECORD_SHA256"
  },
  "policy": {
    "heartbeat_timeout_ms": 1000,
    "media_stall_timeout_ms": 1000,
    "challenge_timeout_ms": 500
  }
}
```

Replace the placeholders with the current controller generation and original play identities.
The record ID is its actual 64-digit SHA-256. No credential values or owner proof belong in
this file. These software liveness values are not physical stopping guarantees.

In **Browser live**, enter its **Absolute guard JSON path** and check **Allow monitoring and
stopping the selected already-playing Handy controller**, then launch. The form captures
its configuration once; changing it does not retarget a running server. Alternatively:

```sh
pulsar browser-live PROJECT REVISION /absolute/live-options.json \
  --handy-player-guard /absolute/guard.json --allow-handy-player-monitoring
```

Both the selection file and explicit monitoring flag are required. Default launch has no
device linkage. Binding starts only after explicit browser playback. The page reports
**Handy stop guard** and distinguishes Pending from an accepted current guard. A pending
bind has not yet established native stop ownership. It never reports synchronized playback.

An original binding can be inspected without sampling, binding or restoring authority:

```sh
pulsar handy-player-guard-status PROJECT --binding-request-id ORIGINAL_BIND_REQUEST
```

This read uses the authenticated native client and local owner capability. Its historical
selection is separate from its recomputed `current` status. Missing history is not proof that
an already submitted request cannot still finish. A stopped/expired controller cannot be
revived through status or replay.

## Native lifetime and failure behavior

Normal binding checks current project Read/Export authority, the exact acknowledged play,
controller generation, runtime and qualification. It persists an original report before
publishing process-local state. A controller has one guard; attaching a new page does not
silently replace it.

Native challenge, observation and release operate on that exact installed guard before
DB-dependent Engine dispatch. They authenticate the captured principal/token hash and local
owner proof without project database, file or device I/O. Public authorization/configuration
mutations retire dependent guards before returning. Native controller monitoring remains
independent of both the browser worker and database lock.

Challenges are one-use and sequence-bound. Reissuing the same pending challenge returns its
original nonce and times. An accepted response anchors heartbeat freshness to challenge issue,
not delayed receipt. Separate media-progress freshness requires advancing frame count and media
time. A first sample with zero reported frames cannot extend it. Paused, seeking, ended,
hidden, reversed or unsupported-rate samples retire the exact guard. Stale identities,
nonces and sequences cannot retire or renew a newer binding.

A heartbeat never renews the controller lease or continuous Handy timing. Controller renewal
preserves guard deadlines. The same native stop owner handles expiry, explicit release and
other retirement causes; no second device transport is created. A stop response still does
not confirm physical stopping.

Release ownership is deduplicated across the binder and control worker. Only an exact matching
retired response confirms native guard release. Lost/foreign/current responses, queue failures
and unresolved binding requests retain sanitized original IDs and are reported at shutdown
after all owned threads join. Uncertain binding does not silently create a replacement request.
These bounded diagnostics are process-local and can be lost on process crash; native expiry
is the fallback. They are not a durable physical-effect ledger.

Preview transport uses one 12-second absolute operation deadline from queue admission, shared
by its RPC/bulk subcalls. Native guard calls have separate short deadlines; capture cleanup has
an explicit three-second budget. Local filesystem locks/fsync/checkpoints and CPU work remain
outside an interruptible wall-clock guarantee. Joined shutdown may wait for those operations.
Concurrent operation and cleanup failures retain each original identity in the returned error.

## Scope and remaining roadmap work

An unbound browser source-selection ID identifies one file/tab/camera selection lifetime.
[Prepared imported media](PULSAR_BROWSER_SOURCE_MEDIA.md) now supplies a separate native binding
to exact bytes, active placement and declared clock anchors; its optional stop-guard pin retires
with that media authority. It still does not calibrate presentation or grant synchronized start.
Captured tabs retain the capture stream's clock; they do
not expose the source video's pause/seek timeline. The guard observes the Pulsar-controlled
player, not arbitrary source-player state.

The [browser timing research](../research/phase-b/browser-player-timing-and-liveness-2026-10-22.md)
explains why challenge roundtrips, `performance.now()` and video-frame callbacks do not by
themselves bound actual display time. Calibrated source/presentation correspondence,
comparison with device phase, synchronized start and periodic controller/timing maintenance
remain required. Generated motion also needs HSP timed-point streaming, queue/horizon and
starvation handling, and unknown-append recovery. A stop guard does not complete that chain.

Software, native HTTP, rendered browser, physical timing/stop, model/media accuracy, platform
coverage and human release acceptance remain distinct evidence. The full roadmap stays active
and partial. See [ADR B5-050](../research/phase-b/ADR-B5-050-browser-player-stop-guard.md).


## Verified software scope, 2026-10-22

The default Engine suite passes 1,205 tests with 70 existing environment-dependent ignores;
Protocol passes 376. The final client suite passes 528 with 12 default ignores. The browser
controller test is then run explicitly in the isolated native campaign. All 55 JavaScript
player scenarios pass; both strict lint configurations, app/worker builds, architecture/source
guards and regular/included-file formatting pass. Initial compiler and lint failures remain
in the [evidence packet](evidence/roadmap-2026-10-22/browser-player-control/checkpoint.json).

Thirteen Engine guard tests cover held-DB heartbeat/release/expiry, independent monitor expiry,
public revocation/revision/configuration retirement, stale credentials/nonces/pages, zero/stalled
frames, renewal without deadline extension, timing invalidation, receipt failure, replay and
monitor failure. Native socket client tests cover lost Bind replies, exact late release,
foreign/current/dropped release replies, secret-bearing error redaction, duplicate release
suppression, unpolled Bind cleanup and closed release queues. Concurrent shutdown failures
retain each original identity. These positive device paths use synthetic/local test fixtures.

The [final native campaign](evidence/roadmap-2026-10-22/browser-player-control/native-final-campaign/summary.json)
runs the real Engine, HTTP server and client controller test with an authored CPU graph.
Five launch/stop/revision-change/drop/expiry cases pass. Independent heartbeats continue
alongside eight incomplete requests; page GET cannot replace an active player; stopped scope
retains its original recording read. The scalar output matches its independent oracle within
0.0001. All five directly supervised child processes join without forced termination, and
worker custody is empty. Device command history, ordinary jobs/attempts, candidates and exports
remain empty. This campaign does not bind a Handy guard or render a browser; it tests native
HTTP and inference plumbing, not useful learned motion or physical behavior.
