# ADR B5-020 — Preserve play observations independently of stop and journal failures

The asynchronous Handy dispatcher needs to retire a matching callback promptly
without losing either its observation or the stop required by an unknown play
outcome. Returning one error for journal or stop-construction failure would discard
one of those obligations after the play callback has already been retired.

Return `PlayCompletion` with independent observation and stop-construction results.
Share the existing stop metadata-retry implementation as `EffectCompletionRecord`,
retaining the old stop name as an alias. Add observation-only play/stop methods that
use an immutable, nonserializable receipt binding with no database handle. This
allows control-state ownership to be released before journal persistence. Existing
synchronous completion methods compose observation and persistence for compatibility.

Receipt binding establishes correspondence only. It does not certify current
database ownership, live dispatch authority, qualification or physical stopping.
Persistence retains existing boot, integrity and outcome checks. Observations cannot
be reconstructed from unknown durable rows, and metadata retries cannot replay
commands. Stop-construction failure remains explicit alongside the accepted play
observation. No schema change or default timing allowance is introduced.

This is a dispatcher prerequisite, not dispatcher or device qualification. See
[contract and evidence](../../docs/PULSAR_PLAY_COMPLETION_RECONCILIATION.md).
