# Browser playback of an imported source

2026-10-22. The integrated implementation connects Browser live to an exact imported
media source, its active project placement and an explicitly declared clock mapping.
It can prepare and serve verified source bytes, then carry that binding through RGB
capture, recordings and motion-candidate provenance. Software, actual native HTTP and
rendered-browser fixture checks pass. This does not establish synchronized Handy playback
or useful learned motion.

The [ordinary browser-only cutover repair](/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-28/native-timing/verification.json) commits native source and timing lineage after actual worker join and shared-pair acceptance. The confined parent-release regression passes, and real rendered40s playback now spans three captures/383 estimates with clean server exit and unchanged limits. Whole-playback recording assembly still refuses source/profile/captured-mode correspondence. Earlier expiry and HSP019 failures retain their separate unresolved causes. See the execution dashboard for current scope.

## Launch and use

Use the development app described in [the quickstart](PULSAR_APP_QUICKSTART.md):

```sh
/home/brianklam/.cache/pulsar-roadmap-validation/debug/pulsar gui
```

This workflow requires a build containing the source-media controls below. An older
installed executable may not contain them.

1. Open a project. Enter a media path and choose **Import immutable source**. In
   **Source placements**, add an active placement with the intended trim and
   source/project anchors. Use **Speed 1/1** for browser-bound capture. Import alone
   does not add a placement. See [placement semantics](PULSAR_SOURCE_PLACEMENTS.md).
2. Open **Browser live**. Capture an explicit compatible **Generator model/profile**
   or **Owner-selected pack**, or select an existing **Options JSON file**. The
   launcher still requires RGB inference with a 64×64 temporal script-position
   profile and immediate continuation; it is not a model-free media viewer.
   Review the finite server lifetime, choose **Launch browser player**, then
   **Open local browser player**. Leave the optional Handy guard unconfigured for
   this workflow. [Live-options setup](PULSAR_DESKTOP_LIVE_OPTIONS.md) describes
   the model/runtime prerequisites.
3. Under **Engine media and placement**, choose **Refresh Engine sources**, then
   select the imported source and placement. The catalog is pinned to the launch
   project/revision and displays its source version, placement ID, hash and size.
4. Enter **Browser presentation anchor (microseconds)** and **Native source-elapsed
   anchor (microseconds)**. Check **I declare this anchor mapping; defaults are not
   calibration or synchronization evidence**, then **Prepare selected source**.
   Zero values are editable defaults, not a measured correspondence. Changing the
   selection or anchors requires a fresh acknowledgment; release an existing
   preparation before replacing it.
5. Wait while the original preparation is inspected. Ready installs its local
   media URL, loads metadata and seeks to the declared trim start. Wait for
   **Press Play explicitly**, then press the video's Play control. Preparation
   starts neither inference nor device motion. Playback must be visible, at
   normal speed and inside the declared trim.
6. Pause, seek or **Stop preview** retires the current player/capture while keeping
   this prepared source loaded. Seek within the trim and press Play explicitly to
   start another playback. Trim end stops playback. **Release prepared source**
   detaches the URL and requests release of the original preparation. After an
   uncertain response, use **Inspect original preparation** or release that same
   selection; do not assume a replacement has started.

The existing CLI launcher is also available:

```text
pulsar browser-live PROJECT REVISION /absolute/live-options.json
```

Here `PROJECT`, `REVISION` and the options path must identify the user's existing
project, current revision and configured options. The default server lifetime is
300 seconds; `--duration-seconds` accepts 1–600. These arguments do not configure a
device connection. On Unix, SIGINT/SIGTERM request normal cleanup and joined exit;
the desktop keeps its own cancellation path. Cleanup failures remain explicit errors.

## What the clock declaration means

For browser presentation timestamp `B`, declared browser anchor `Ba` and declared
native elapsed anchor `Ea`, Pulsar computes exactly:

```text
source elapsed E = Ea + (B - Ba)
project time P   = active placement.project_at_source(E)
```

Native elapsed time is the source's elapsed timeline used by the placement, not
an assertion that browser zero equals a file's first native PTS. Nonzero PTS,
container edit lists, variable frame timing and browser decoding can affect that
correspondence. This feature does not measure those effects. The first accepted
frame uses the declared mapping to establish its exact project anchor; it does
not silently redefine the declaration or reset the project to zero. Leading gaps
therefore remain gaps. Integer project time is rounded down only at the boundary
where that representation is required.

The browser uses frame-callback `mediaTime`, rounded to microseconds. Native
capture independently checks the exact mapping and half-open source trim. Bound
capture currently permits only forward unit-rate placements. File-picker video,
camera and shared-tab input remain unbound; a captured tab stream's timestamp
does not identify the original player's timeline. See [time domains](PULSAR_LIVE_TIME_BASIS.md)
and [browser timing research](../research/phase-b/browser-player-timing-and-liveness-2026-10-22.md).

## Integrity, lifetime and retained evidence

Preparation requires current project Read and Export authority. The Engine opens
the imported regular file without following a final symlink, retains that
descriptor and source hold, and verifies the complete expected hash and length.
It records 64 KiB block hashes and checks complete affected blocks before returning
a range. Holding an open descriptor alone would not detect same-inode byte edits.
Preparation and reading do not modify the imported file or transcode it.

The page receives an opaque process-local `/media/…` URL. Native paths and Engine
credentials remain outside the page. A bounded media worker lane handles GET,
HEAD and single byte ranges independently of Preview and player control. Responses
use exact lengths, a strong content ETag and same-origin restrictions; a failed
partial delivery closes. This is an ownership and scheduling separation, not a
measured responsiveness guarantee.

Read authority survives ordinary pause/seek, but source replacement, explicit
release, launch shutdown, expiry or loss of native authority retires it. Public
revision/placement/session/grant changes invalidate the old binding; revoke then
regrant cannot revive it. Page close requests release, with finite native expiry
as a fallback. Retirement cannot erase bytes already buffered by the browser.
Status and repeated original requests inspect retained metadata;
they do not restart verification or recreate a lease. Historical Ready can remain
visible with `current:false`, including after restart.

Capture pins the current native binding, uses the imported source version in its
identity, and retains that binding across continuation and recording pages.
Assembly refuses mixed bindings or bound/unbound pages. Candidate provenance
retains the binding and adds the review reason `browser_source_clock_declared`.
The existing [record, review and commit workflow](PULSAR_BROWSER_RECORDING_CANDIDATES.md)
remains explicit. The recording page displays browser presentation timestamps;
candidate assembly uses the retained project clock. Supplied RGB bytes are still producer input: source-byte
verification does **not** attest that each submitted RGB frame came from that file
or was displayed at the reported time.

An optional [Handy player stop guard](PULSAR_BROWSER_PLAYER_CONTROL.md) can retain
the same source pin for an already-playing, separately authorized controller.
Source retirement invalidates that guard through its existing stop owner. This
does not start playback, renew Handy timing, prove physical stopping, or confer
synchronized-start authority.

## Bounds and validation scope

The descriptor reader is implemented on Unix; non-Unix capture returns
`Unsupported`. Its source-size ceiling is **64 GiB**, additionally constrained by
the Engine's configured snapshot limit and resource admission. Preparation has
a 120-second verification deadline. A source lease is at most **600 seconds**;
the browser server's independent 1–600-second lifetime can end access earlier.
These are byte/lifetime limits, not a 600-second media-duration limit. The catalog
admits at most 256 active available media placements. The browser adapter currently
admits recognized MP4 and WebM headers; that does not prove codec support in a
particular browser.

Core **187**, Protocol **388**, Engine **1,231** (70 default ignores), Clients
**539** (13 default ignores), and JavaScript **84** tests pass, alongside both strict
lint configurations, builds and architecture/format checks. The ignored source-media
test runs separately against a private Engine: exact range reconstruction, more than
18 seconds of authored RGB inference across a prepared handoff, retained clock/source
provenance, and control during two slow media responses pass. Its measured maximum
heartbeat response was 4.127 ms and stop response 93.473 ms; these are local fixture
observations, not physical stopping bounds or general responsiveness guarantees.

The actual in-app browser rendered the imported gray fixture and retained **297**
accepted estimates across two captures through the 30-second trim. The first accepted
browser frame at 0.1 seconds mapped to project 0.85 seconds, preserving the declared
0.75-second placement and leading gap. The successor retained 145 estimates. Explicit
seek/resume produced another live estimate. Both native CLI SIGINT/SIGTERM cases and
the rendered session's SIGTERM exited zero with an active capture, retired the original
capture/media authority and joined owned processes. No real Handy was contacted.

The rendered run exposed a readiness race: the successor could become Ready between
status reads while the client displayed the superseded predecessor. A deterministic
IPC regression fails before the bounded reconciliation fix and passes afterward;
failed identity, clock, binding and retirement proofs still refuse adoption. Queued
programmatic pause events also no longer overwrite the primary stop reason or retire
a later explicit playback. The earlier failed campaigns remain in the
[checkpoint and evidence packet](evidence/roadmap-2026-10-22/browser-source-media/checkpoint.json).

The gray video and diagnostic graph establish this software path, not useful learned
motion, generic RGB attestation or calibrated presentation. There is no new physical
qualification, synchronized start, HSP generated-motion streaming or measured 200 ms
capture-to-command result. Full roadmap completion still requires those implementation
and real model/media/platform/device/human acceptance gates.

Implementation: [clock math](../crates/pulsar-core/src/declared_source_clock.rs),
[binding DTOs](../crates/pulsar-protocol/src/browser_media.rs),
[Engine lifecycle](../crates/pulsar-engine/src/browser_media.rs),
[verified reader](../crates/pulsar-engine/src/browser_media_reader.rs),
[native browser adapter](../crates/pulsar-clients/src/browser_source_media.rs),
[player UI](../crates/pulsar-clients/src/browser_live.html).
Decision: [ADR B5-051](../research/phase-b/ADR-B5-051-browser-source-media-binding.md).
