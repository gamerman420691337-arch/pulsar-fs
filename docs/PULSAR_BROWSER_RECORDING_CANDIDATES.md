# Browser recording candidate controls

2026-10-13. Implemented with native HTTP/Engine and rendered in-app browser evidence.
Full roadmap PARTIAL. These controls stage and inspect. Subsequent [reviewed commitment](PULSAR_BROWSER_RECORDING_COMMIT.md), [gap proposals](PULSAR_BROWSER_GAP_PROPOSALS.md), [script download](PULSAR_BROWSER_SCRIPT_DOWNLOAD.md) and [original staging recovery](PULSAR_BROWSER_RECORDING_CANDIDATE_RECOVERY.md) are implemented with their own scoped evidence.

## Use

Launch the [local browser player](PULSAR_BROWSER_LIVE_PREVIEW.md) with an existing
project and checked temporal model. Enable recording before playback, select a
local video, play, then **Stop preview** and **Refresh recordings**.

Select one stopped capture. **Stage selected playback** includes all retained
continuation captures belonging to that playback. Enter the complete project
range in seconds and an explicit maximum interpolation interval in milliseconds
(up to 1000 ms). The current defaults are 0..60 seconds and 200 ms; they are not
accuracy measurements or automatic knowledge of the video duration. Recorded
points outside the selected range refuse rather than being clipped.

The page first prepares a retained request identity without creating a candidate,
then dispatches that exact intent to the existing Engine staging operation. It
shows the candidate ID, point count, first 64 estimates, evidence labels and
unavailable intervals. **Inspect retained staging intent** reads the original
identity and confirmation state. **Retry original staging request** repeats that
same request; it never prepares a replacement. **Review candidate diagnostics**
reads shared diagnostics without recording a review disposition or committing.

For the separate current CLI review/commit/export workflow, see
[Engine recording candidates](PULSAR_LIVE_RECORDING_CANDIDATE.md). Unresolved gaps
continue to block normal funscript export. Staging and numerical review do not
qualify model accuracy or physical timing.

## Scope and recovery

Prepare selects only records in the private owner/session/project index and derives their
original boot, revision, playback membership and recording-intent IDs from the
Engine. The page supplies no credentials or arbitrary candidate ID. Capture must
be stopped before preparation/staging. The Engine independently checks current
authority, original actor ownership and coverage. Candidate/review reply scope is
checked before display.

One candidate intent is retained per owner session/project. Changed options refuse
replacement; preserve or reconcile the original first. Preparation itself has no
project effect. The request identity is returned before staging dispatch; a lost
staging reply remains unconfirmed until exact retry. The [private staging journal](PULSAR_BROWSER_RECORDING_CANDIDATE_RECOVERY.md) now preserves identity/options across server loss; explicit retry uses the original request. A null candidate ID means unconfirmed outcome, not proof of no
effect. Read/stage errors do not issue Stop against unrelated live work; active
preparation refuses without stopping preview.

These controls do not commit, export, fill gaps, acquire arbitrary browser tabs or
send toy commands. [Original recording commit recovery](PULSAR_BROWSER_RECORDING_COMMIT_RECOVERY.md) has its own scoped evidence. [Confirmed candidate retirement](PULSAR_BROWSER_RECORDING_CANDIDATE_RETIREMENT.md) enables another recording stage on the same project. Recording-index capacity, broader crash/platform qualification and useful-model/device acceptance remain follow-up work.

## Evidence

Clients pass 412 tests with ten ignored, eight mock-DOM player tests pass, and both
strict Clippy runs, Windows source check, native/controller builds, formatting,
architecture and 36 guards pass. Engine/Core/Protocol source identities match the
preceding candidate checkpoint; no fresh suites are claimed for them.

A 50-second native HTTP/CPU campaign records 491 estimates across four captures and
three handoffs. Prepare has zero candidate effects; stage/retry preserve one
candidate, shared diagnostics report missing coverage, and changed preparation
refuses replacement. Active preparation refusal leaves capture running. Existing
seek/stale/stall/server cleanup, actual Engine restart replay, explicit CLI fixture
commit and unresolved-export refusal pass. Final counts are one project, zero jobs,
one candidate and zero exports; staging custody is empty and all owned processes
are joined.

Actual Codex in-app browser QA selects the authored gray WebM, records 253 accepted
estimates across two captures, stops, refreshes, stages one playback, retries and
inspects the same request/candidate, then reads seven diagnostic issues for three
unavailable intervals. Full accessibility snapshots and a screenshot are retained.
The Engine's captured source bytes contain all 253 inferred/unqualified estimates
and match the authored scalar oracle. Direct SQL inspection through the process
root failed to open the database; no independent rendered SQL-count claim is made.
Native HTTP SQL counts above are separately verified. Browser warnings/errors are
empty, staging custody is empty, and the owned tab/server/Engine are closed/joined.

The first native campaign exposed omitted HTTP allow-list entries despite compiled
handlers. Its failure and retirement are retained; the routes were added, a new
same-origin route test was added, and validation plus the full native campaign were
rerun. This uses an authored diagnostic scalar graph, not useful trained motion
inference. Broader browsers/platforms, timing/accuracy, CUDA candidate campaigns,
Handy hardware and human/release qualification remain open.

![Rendered candidate review](evidence/roadmap-2026-10-13/browser-recording-candidates/rendered/review.jpg)

See [ADR B3-191](../research/phase-b/ADR-B3-191-browser-recording-candidates.md)
and [checkpoint](evidence/roadmap-2026-10-13/browser-recording-candidates/checkpoint.json).
