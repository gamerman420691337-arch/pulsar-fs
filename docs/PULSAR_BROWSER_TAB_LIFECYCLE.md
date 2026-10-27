# Experimental tab-capture loss and playback-failure handling

The tab-sharing path now treats video-track mute as source inaccessibility: it stops
owned tracks, clears the position and queues Engine Stop. Already-muted or ended
selected tracks refuse before inference. Resume requires another explicit share;
there is no automatic permission or source retry.

A failed video.play promise can arrive after its play event changed the generation.
Cleanup therefore checks both ownership of the selected stream and the original
permission generation. A selected stream that still belongs to this share is closed;
a late permission rejection without a stream cannot stop a newer local playback.
New tests demonstrate both the initial failure and the corrected behavior.

Twenty-four mock-player tests cover this lifecycle plus existing selection, frame,
recording, review and export controls. These are simulated DOM/permission/track events,
not real browser capture. Workspace browser tools remain unavailable, so actual mute,
revocation/autoplay behavior and browser/platform acceptance remain open. Shared-stream
time still differs from the source player's clock; useful motion, physical Handy sync
and model/media/human/release gates remain open. See the [tab-input scope](PULSAR_BROWSER_TAB_CAPTURE.md)
and [checkpoint](evidence/roadmap-2026-10-13/browser-tab-lifecycle/checkpoint.json).
Full roadmap PARTIAL.

## Verification scope

Clients pass 425 tests with ten ignored; twenty-four player mocks, both strict lint
configurations, App/Worker builds, formatting, architecture and source guards pass.
The existing real CPU/HTTP two-cycle recording/repair/export and three-scope index
retirement campaign still passes with empty custody and joined processes. This is
transport regression, not actual tab-capture evidence. Rust/Core/Protocol/Engine
sources remain unchanged; no fresh Core/Protocol/Engine suites are claimed. The
failing late-denial mock and preceding HTML are retained. Browser workspace tool
discovery still returned no capture-capable workspace tools. All browser/platform,
source-player timing, useful-model and physical-device gates remain open.
