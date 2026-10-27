# Browser accepted-estimate recording controls

2026-10-13. Implemented and exercised for the local browser player. Full roadmap
PARTIAL; physical playback and useful trained-model quality remain unqualified.

## Use

Launch the [local browser player](PULSAR_BROWSER_LIVE_PREVIEW.md) with an existing
project, revision and explicitly checked temporal model/profile. Before Play,
select **Record accepted estimates for the next playback** and choose a limit
from 1 to 4096 samples per capture. The default limit is 1024. The choice is frozen
for that playback; automatic successors begin their own recording before pixels
are submitted. Recording remains optional and off by default.

After Stop, click **Refresh recordings**, select a capture, and use **Read first
page** to inspect its first 64 accepted estimates. **Save inference record (.json)**
downloads the complete stopped capture, preserving profile, clock, original
request, pixel/attempt bindings and inferred/provisional evidence labels. Each
continuation capture is listed separately. This download is not a funscript or a
committed project candidate and does not send toy commands.

## Boundaries

The Client retains original recording intents before dispatch, including unknown
outcomes. The [durable recording index](PULSAR_BROWSER_RECORDING_INDEX.md) now retains these identities across adapter restart and admits at most 64 scopes per owner session/project. Reads accept only
retained scopes under the current Engine actor. Unavailable reads remain unknown;
they do not invent empty recordings. Active or discarded capture downloads refuse.
Read/save errors do not stop ongoing playback. Archives page in groups of 64,
with at most 64 pages and 32 MiB per capture; stable metadata and advancing cursors
are checked throughout. Engine recording memory and overflow behavior remain
[Engine-owned](PULSAR_LIVE_RECORDING_BUFFER.md).

Downloads contain no raw media/pixels and preserve qualified:false. Native
recordings remain ephemeral to their original Engine boot. An unsigned downloaded
JSON record cannot reconstruct native authority after boot loss. Candidate
assembly must preserve missing coverage and use shared review/commit/export
controls rather than silently promoting these estimates.

## Evidence

The exact 542-path source subject and three binaries were reverified. Client tests
pass 410 with ten ignored; six mock-DOM player tests, both strict Clippy runs,
Windows source check, native/controller builds, formatting, architecture and
36 guards pass. Engine/Core/Protocol sources are unchanged from the preceding
recording checkpoint; no fresh suite is claimed for those components.

The 50-second native HTTP/Engine campaign records 493 samples across four captures
and three handoffs, checks authored scalar and time oracles, repeated archive
identity, foreign-scope refusal and active-save refusal without stopping playback.
Stop/seek/stale/stall/server-exit scenarios pass. Ordinary jobs/candidates/exports
remain zero and staging custody is empty.

Actual in-app browser QA selected an authored gray WebM, enabled recording, played,
stopped, refreshed, inspected a page and downloaded a 152-sample, three-page JSON
record. The two stopped captures contain 152 and 132 accepted samples. The download
was moved from the owned Downloads file into retained evidence. Both server and
Engine were joined; the owned browser tab is absent on final inventory. The final
screenshot was not captured before automatic tab cleanup; retained accessibility
text is only an accessibility diff and does not independently prove the full
control/table state. The actual downloaded bytes and Engine observations are
retained; full visual QA evidence must be captured in a follow-up.

This is an authored diagnostic scalar model, not evidence of useful motion
inference. Arbitrary-tab acquisition, recording-to-candidate/export integration,
broader browser/native GUI/platform/CUDA coverage, physical Handy synchronization
and human/release acceptance remain open.

See [ADR B3-188](../research/phase-b/ADR-B3-188-browser-recording-controls.md)
and [checkpoint](evidence/roadmap-2026-10-13/browser-recording-controls/checkpoint.json).
