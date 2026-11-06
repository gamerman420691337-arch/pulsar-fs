# Durable HSP commands and native dispatch

This increment connects the [generated-stream coordinator](PULSAR_HANDY_GENERATED_STREAMS.md)
to original command history and a bounded native sender. Authenticated HSP stream
admission, an Engine-owned continuous stream pump, accepted-inference delivery and
browser controls remain unfinished. No real Handy has been contacted. The scoped
software and executable checks below pass; the earlier startup timeout remains
recorded and does not establish ordinary-storage startup reliability.

The subsequent [native Stop fallback](PULSAR_HANDY_HSP_EMERGENCY_STOP.md) adds
shared runtime reservations and an emergency path for an already-issued exact
Stop. Its retrospective observation is not a durable claim; continuous ownership
and complete recovery integration remain required.

## Original command custody

Journal v10 stores exact stream bindings, original actor/request, device and
registration snapshots, keyed credential route, referenced qualification/profile,
wire bytes, timing deadlines and append samples. Content-addressed history cannot
be deserialized into a native owner, runtime effect or dispatch claim.

Preparing an original request is replayable. Claiming it is single use and
durable before network I/O. Unknown results stay unknown; one later observation
may annotate history without replacing the original judgment. Restart preserves
unresolved custody and creates no new playback authority or automatic retry.
Acknowledged Stop still leaves physical stopping unresolved.

The store bounds streams, rows and retained bytes, with independent space reserved
for Stop. Its validated cache is tied to the SQLite snapshot; external writes,
schema changes, unknown local mutations and failed operations invalidate it.
Snapshot stamps are captured before releasing the transaction, avoiding adoption
of an unvalidated intervening write. Full validation checks exact schema, records,
membership, counters, append continuity and shared device/route custody. Historical
row limits are not evidence that sustained journal latency meets the live target.

HSP and hosted-script setup, synchronization, play and reconciliation share
exclusion rules. A matching device or credential route is a conflict; unresolved
history in another credential namespace is not presumed unrelated. Legacy
reconciliation cannot clear an HSP stream by leaving it out of its snapshot.
Registration changes also observe HSP custody. Cancellation releases a stream
only when no command was ever claimed.

## Native dispatch and cancellation

The sender requires separate online-control consent, keeps credentials in native
memory and uses the fixed vendor origin with no redirect, proxy or retry. Before
the durable claim it checks device session, mode generation, expected stream and
the relationship between captured credentials and the journal's keyed route.
Final current authority is supplied under the caller's dependency guards; a
receipt or callback cannot forge another worker's claim handoff. I/O runs after
those guards and database/runtime locks have been released.

Original and current timing bound the entire operation. Suspend-aware clock
checks apply before and after I/O. A blocked request remains owned until its
worker is joined; a cancellation request alone does not free the slot. Lost or
malformed replies after claim produce Unknown. Retrying persistence uses the
retained completion and has no transport handle.

A rejected request is not, by itself, proof that a cloned receipt was never
claimed elsewhere. Explicit abandonment therefore requires an atomic journal
cancellation matched to the exact sealed effect. Only that proof can retire the
same runtime slot. An unsent Setup ends without requesting an impossible Stop;
an unsent Append or Play retains earlier stream custody and permits Stop. An
explicitly abandoned unsent Stop remains an unresolved stopping obligation.

The native component requires observed HSP mode 4. Setup can change its mode
generation, so subsequent operations need a separately observed post-Setup binding;
an old transport cannot adopt it silently. Stop retains its original owner after
ordinary source/clock retirement and refuses known replacement identities.
The vendor has no atomic expected-stream precondition. Read preflight cannot
manufacture a remote exclusive lease or eliminate the replacement race.

## Remaining product integration

The product still needs HSP-specific qualification and reconciliation, mode
preparation and post-Setup acquisition, a continuous native stream owner with
journal-failure emergency stopping, authenticated public commands, checked live
inference delivery and explicit browser/application controls. Large-history
latency, buffering/starvation behavior, real media/model quality, browser timing,
the 200 ms target, physical Handy 2 Pro behavior and all other M1–M8 platform,
six-axis, human and release gates remain open.

## Verification and limits

The current full default suites pass Core215 and Engine1,354 (72 Engine ignores).
This increment adds 50 executed cases: 23 journal, six record, 17 localhost sender
and four coordinator tests. Both strict workspace/all-target lint configurations,
formatting, architecture/source guards and application/worker builds pass. The
ignored native fixture exporter passes separately and its exact producing test
image is retained. No fresh full Client or Protocol test run is claimed.

Actual current and preceding-v9 executables pass clean-v10 startup, populated-v10
old-reader refusal, unchanged existing database/WAL bytes including held nonempty
WALs, and current startup/shutdown/reopen. Original stream, effect and outcome
bytes remain unchanged; the unknown Append stays unknown after acknowledged Stop.
The history contains authored bindings and outcomes. It supplies no restored HSP
authority or physical/human acceptance. All 12 direct application/CLI children in
the final campaign joined; the five children from the earlier failed fixture
attempt also joined, and the two namespace supervisors are counted separately.

Two initial legacy-record tests used an invalid review revision and were fixed;
the first strict run's five style errors were also corrected. The first native
reader attempt exhausted its 256 MiB RAM fixture before reaching the format check.
The corrected 1 GiB allocation passes without changing production admission limits.

The aggregate gate's ordinary-filesystem JS boot check initially timed out at its
15-second readiness budget. Inspection found a temporary worker snapshot before
journal/database creation. The unchanged test subsequently passed on ordinary
storage and in a bounded private RAM fixture. This observation does not establish
a root cause or a production repair; the failed run remains retained and startup
reliability/durability qualification stays open. No test timing threshold was changed.

See [ADR B5-055](../research/phase-b/ADR-B5-055-durable-hsp-command-custody.md) and
the [checkpoint](evidence/roadmap-2026-10-23/hsp-dispatch/checkpoint.json).
