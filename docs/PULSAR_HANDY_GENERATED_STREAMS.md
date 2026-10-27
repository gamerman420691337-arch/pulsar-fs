# Generated Handy point streams

This work builds the streaming path needed to send generated stroke positions to
Handy as analysis produces them. It is separate from
[prepared-script browser Start](PULSAR_BROWSER_HANDY_START.md), whose device input
is a complete hosted script. The codec, bounded point queue and supplied-fact
coordinator pass the initial component checks below. The subsequent
[durable/native dispatch increment](PULSAR_HANDY_HSP_DISPATCH.md) adds command
history and a bounded sender. Authenticated Engine admission, continuous ownership,
inference delivery and browser controls remain open; no real device was contacted.

## Wire contract

The [manufacturer research](../research/phase-b/handy-hsp-stream-contract-2026-10-23.md)
supports explicit Setup, Add, Play, State and Stop operations. Points retain
integer milliseconds and canonical integer percent in 0–100. The REST schema's
contradictory position maximum of 50 remains a compatibility gate; the codec does
not halve the intended motion. Buffer indices are cumulative across appends.
The first chunk has at least two points so its zero-based tail also satisfies
REST's minimum tail of one; later chunks can contain one point.

The active-state decoder preserves signed script times and sentinel indices.
It accepts the documented numeric states and refuses undocumented string aliases,
contradictory responses and malformed bounds. Approximate buffer state is neither
an exact point-content digest nor physical position or clock-renewal evidence.

## Point and operation ownership

Each stream has a fresh native generation and exact live-source identity. A
bounded queue retains one immutable append in flight. Admission and claim check
the original sample age, both ends of the independent playback-phase interval,
future horizon and source coverage. Skipped analysis sequences are permitted
only when the Engine independently establishes continuity of the retained edge;
ordinal adjacency or a short timestamp gap is not that proof.

Acknowledged points stay counted against capacity until an independent lower
playback-phase bound proves earlier endpoints can be retired. The preceding
interpolation endpoint remains retained. An unanswered append freezes further
motion and retains the exact chunk. The cumulative tail is not a deduplication
key, so an uncertain append is never automatically resent.

The coordinator separates setup, prefill and an explicit Play request. Play
retains its original script/server-time pair and validity deadlines. Stop is
latched independently of blocked generation or append work. A transport
completion can free the single operation slot only after the underlying I/O has
settled; requesting cancellation is insufficient. Late observations remain
historical and cannot reopen the stream. Stop acknowledgment remains a transport
observation, not proof of physical rest.

## Required integration

The enabled product path still needs all of the following:

1. Integrate the implemented durable records, single-use claims and native sender
   into a continuous Engine-owned stream pump, including metadata-failure stopping.
2. Extend the implemented journal exclusion rules into continuous shared
   credential-route/device ownership, HSP recovery and Engine shutdown.
3. Accepted HSP-specific configuration, dynamics, buffering, timing and stopping
   evidence. Existing HSSP qualification cannot silently authorize a new driver.
4. Connect mode preparation and separately observed post-Setup acquisition to
   the implemented sender's exact native identity, deadline and cancellation checks.
5. A producer attached to the Engine's checked accepted-inference branch, with
   exact sample provenance and an independently verified source-to-device clock.
   Polling the browser's latest estimate would lose accepted outputs and is not
   a delivery channel.
6. Explicit application controls and controlled-player presentation buffering,
   including source replacement, capture continuation, pause/seek/end and owner
   loss. Receipt timestamps cannot be substituted for media phase. The optional
   added playback delay remains bounded by the specification's one-second limit.

Native composed and rendered tests must then exercise actual streaming, pressure,
starvation, blocked append, ambiguous completion and joined shutdown. Useful
learned motion, real-media correspondence, arbitrary-tab synchronization, the
200 ms capture-to-command target, physical Handy 2 Pro behavior, six-axis devices,
other platforms, human acceptance and release gates remain open. Full M1–M8
completion is unchanged.

## Verification

The initial component checkpoint passed 45 new cases: 16 queue, nine wire and 20 coordinator tests. Its
full default Core suite passes 215 with no ignores; the Engine library suite
passes 1,304 with 71 existing qualification/optional cases ignored. Both strict
workspace/all-target lint configurations, application/worker builds, architecture,
source guards and formatting pass. This checkpoint does not claim a fresh full Client or Protocol
test run, a native HSP HTTP campaign, rendered streaming or physical behavior.

The source check exposed an overly strict initial assumption: Setup does not
promise the playback options that Play later explicitly writes. Pre-Play
correlation now permits well-formed observed options, including a zero state
rate; successful Play and active append observations still require the requested
rate-one, nonlooping, nonpausing policy. Regressions cover the complete transition,
changed active options, fresh facts with expired original deadlines, skipped
analysis sequences with separately checked continuity, and late results after Stop.

Exact source subjects, earlier checks, the corrected test-style lint failure and
cleanup are recorded in the
[checkpoint](evidence/roadmap-2026-10-23/hsp-stream/checkpoint.json).
See [ADR B5-054](../research/phase-b/ADR-B5-054-bounded-generated-handy-streams.md).
