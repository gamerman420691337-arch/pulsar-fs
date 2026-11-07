# Experimental consent-based browser-tab input

The local `browser-live` player now has **Share a browser tab**. Clicking it requests a
fresh browser-selected screen-capture permission. The display-surface preference is a
hint; Pulsar separately checks the returned video track and refuses non-browser or
unidentified surfaces before Engine Start. Audio is not requested. A compatible browser,
secure context and user gesture are required by the [Screen Capture specification](https://www.w3.org/TR/screen-capture/).
Actual permission-dialog acceptance and browser/platform compatibility remain unverified.

Launch the existing player with a project/revision and explicit temporal model/profile.
Enable recording beforehand if desired, then choose the tab through the browser dialog.
The acquired MediaStream becomes a muted preview source and uses the existing 64×64 RGB,
one-outstanding-frame transport and Engine leases. Permission denial and late selections
cannot start analysis. Stop, local-file switch, tab-track revocation, page exit, decoding
error and expired/failed analysis stop owned sharing tracks and clear motion. There is no
automatic permission retry. Credentials remain native; no device commands are issued.

Shared-stream presentation timestamps describe capture time, not the source tab's video
player time. The feature cannot currently detect source-player seeking, pause, speed or
media identity, cannot map a recorded script back to that video's timestamps, and does
not synchronize a toy. Whole-tab compositing and protected/blank content can affect model
input. Do not interpret this as qualified browser-watching generation. Useful trained
motion, target selection/cropping, source-player timing, frame/capture correspondence,
physical Handy synchronization and human/release gates remain open.

The isolated workspace browser tool family was unavailable in this session; no real
`getDisplayMedia` picker or browser tab capture was exercised. Player mocks and existing
native Engine transport checks establish scoped implementation evidence only. See
[checkpoint](evidence/roadmap-2026-10-13/browser-tab-capture/checkpoint.json).
Full roadmap PARTIAL.

## Scoped verification

Twenty-one player mocks pass, including explicit video-only selection, returned browser
surface validation, normal bounded RGB delivery, permission denial, late consent after
Stop, revocation, source switch, page exit and unsupported API refusal. An initial
post-play generation check incorrectly closed its own selected stream; matching stream
identity after play fixes that mock while late permission replies remain fenced.

Clients pass 425 tests with ten ignored; strict Client and workspace/all-feature lint,
native App/Worker builds, formatting, architecture and guards pass. The unchanged native
CPU/HTTP pipeline still passes two recording/repair/export cycles and three-scope index
retirement/recovery with empty custody. This is regression evidence for the shared
transport, not evidence of real browser screen capture or source-video synchronization.
Core/Protocol/Engine sources are unchanged and no fresh suites for them are claimed.
Actual picker/MediaStream decoding, browser/OS compatibility, source-clock mapping,
useful models and physical-device acceptance remain open. Full roadmap PARTIAL.

The subsequent [capture-loss lifecycle](PULSAR_BROWSER_TAB_LIFECYCLE.md) stops muted
tracks, refuses unavailable selections and fences post-play/late permission failures.
Its scoped tests do not add real browser or source-player-clock acceptance.

[Declared live time basis](PULSAR_LIVE_TIME_BASIS.md) now travels through Engine recording
pages and candidate lineage, with a capture-time review warning. This retains the
distinction without assigning source-player identity or synchronization.
