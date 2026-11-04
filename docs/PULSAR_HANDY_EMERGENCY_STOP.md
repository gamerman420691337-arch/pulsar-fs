# Handy stop fallback and incomplete history

An outstanding native Handy stop can now proceed when its journal prewrite cannot be confirmed.
The Engine first tries the ordinary durable path. If the journal mutex is unavailable or the
write fails, it can dispatch the same captured stop through the same runtime and transport.
A positively observed foreign journal owner refuses dispatch. This is a bounded software attempt;
it is not a guarantee that a disconnected device receives the stop or physically stops.

## Authority and observations

Only Engine code can construct the fallback envelope from an exact current stop effect and its
captured immutable intent. Play cannot use it. The final claim checks the same native effect,
session, configuration and transport as the ordinary stop. Both paths share one consumed stop
sequence, so neither a duplicate dispatcher nor journal recovery can send that stop again.
No synthetic receipt is created. The fallback uses the existing fixed HSSP stop endpoint,
captured credentials, bounded I/O and response validation; it requires no play clock or lease.

An attempted stop retains Acknowledged or Unknown transport outcome. A refusal before the claim
remains Rejected, without inventing an outcome. An observed callback retires the host token to
`StopUnconfirmed`; acknowledgment does not clear physical uncertainty, restore a controller or
resume playback. A late callback may retain history without changing a replacement session.

The owner retains a pending outcome in memory and retries only its metadata write. Journal
recovery never reenters dispatch. Shutdown joins the stop and reports an error if stop dispatch
or outcome metadata remains unresolved when its cleanup budget ends. Abrupt process loss can
lose an unrecorded emergency observation. Existing play history and unresolved custody must not
be interpreted as evidence that no stop was attempted, or as permission to resume.

## Read stop status

```sh
pulsar handy-stop-status PROJECT --acquisition-request-id ORIGINAL_ACQUIRE_ID
```

This requires the local owner capability and current Read permission for the project. It selects
the saved original controller acquisition. It reports known stops from the original Engine epoch,
device session, project and revision within the device journal's latest 128 observations. It also
reports retained activity for that exact native controller when available:

- Pending: the stop intent is retained.
- InFlight: dispatch is submitted; no transport outcome is asserted.
- Observed: Acknowledged or Unknown is retained, with `metadata_recorded` identifying whether
  its outcome reached the journal.
- Rejected: the local handoff was refused; no transport outcome is invented.

`emergency` identifies the fallback without a confirmed prewrite. Native activity may lag the
worker. `history_complete` is always false: the query is bounded and unrecorded attempts can be
lost on restart. After restart, native activity is absent. Empty history therefore proves neither
absence of an attempted stop nor physical stopping. This command performs no device I/O.
`handy-play-cancel` retains its separate meaning: cancellation of exact unclaimed play work.

## Journal compatibility

Version 7 adds an immutable emergency-dispatch marker and a bounded observation census. The
late write commits the exact generic stop effect, actual terminal outcome and marker together.
An identical pre-existing prepared stop is permitted: a failed prewrite may have committed before
its error was reported. The marker means dispatch lacked confirmation of the prewrite, not that
no prewrite existed. Exact metadata retry succeeds; contradictory outcomes, play intents and
foreign epochs refuse. Original effect bytes and IDs do not change.

Startup checks marker/effect correspondence, terminal stop shape, provenance digest and census.
Missing or corrupt markers and missing linked effects refuse instead of being reconstructed.
The main project schema is unchanged. Earlier v6 readers require explicit migration rather than
silently ignoring these records.

The [checkpoint](evidence/roadmap-2026-10-22/handy-emergency-stop/checkpoint.json) identifies the
source, executable hashes, checks and native compatibility campaign. Qualification remains open:
composed tests use localhost HTTP and synthetic timing, and native CLI history fixtures are
explicitly synthetic. No actual Handy, private media, real synchronization or human acceptance
was involved. Attempted-effect reconciliation, physical stop disposition, continuous timing and
browser presentation/heartbeat integration remain required. Full roadmap PARTIAL.

## Verified software scope

The final default Engine suite passes 1,138 tests with 70 explicit environment-dependent ignores.
Focused checks pass 47 journal, 69 playback-runtime, 187 Handy Engine, five emergency-owner and
five shutdown cases; those Engine counts overlap. Protocol tests pass 346; Handy client tests pass
44 with one existing desktop case ignored. Both strict lint configurations, App/Worker builds and
architecture/format guards pass. The original enum-size lint finding and its boxed-envelope repair
remain in the evidence packet.

Composed tests exercise journal mutex contention, a simultaneous held project DB and SQLite writer,
failed stop inserts, dropped replies, metadata recovery without resend, mixed durable/emergency
single claims, foreign-epoch refusal and shutdown with unrecorded metadata. The native CLI campaign
preserves synthetic ordinary/emergency history across restart and refuses wrong subjects/owner
proofs. The actual v6 reader refuses v7, preserving database and existing WAL bytes, including held
nonempty WALs. Current startup refuses corrupt/deleted markers and missing linked effects. An absent
WAL may be created empty by SQLite; whole-directory or sidecar-presence identity is not claimed.
The corrected generic device-journal native regression also passes.
