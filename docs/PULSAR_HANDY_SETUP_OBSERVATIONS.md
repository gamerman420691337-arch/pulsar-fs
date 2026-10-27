# Retained Handy setup observations

Successful guarded setup now retains the validated response in the device-effect journal.
Read it with the existing command:

```sh
pulsar handy-setup-status PROJECT --setup-request-id ORIGINAL_SETUP_REQUEST
```

For newly acknowledged setups, `history.acknowledgment` contains the sanitized HSP fields:
stream ID, point count/capacity, current point/time, first/last point times, tail indices,
play state, rate, loop and starvation-pause flags. Values must satisfy the existing bounded
non-playing, non-looping normal-rate setup response rules. The sender still requires a
different stream ID from preflight and withholds responses that echo captured credentials.
It retains only the declared fields, not the raw vendor body, URL or credentials.

These are observations from the setup reply. They are not a fresh state query or a
device-reported content hash. Partial-capacity and not-initialized responses can satisfy the
existing acknowledgment contract; they do not establish a complete playable program.
The [program inspection](PULSAR_HANDY_PROGRAM_CORRELATION.md) now checks a fresh stopped-stream
association when the original mode generation was also captured.
`loaded` and `qualified` remain false. Recovery never restores a dispatch claim or live
playback authority and does not release an attempted setup's route reservation.

## Storage and recovery

Journal version 3 adds bounded observation rows with a checksum bound to the original setup
record ID. Response and acknowledged state commit in one transaction. Either write failing
leaves the setup claimed with no recorded response; it does not authorize resending. Exact
metadata completion retry accepts only the same observation. Changed observations refuse.

New setup intents explicitly require retained acknowledgment data. Deleting that data cannot
turn a new acknowledged attempt into a legacy acknowledgment. Startup checks required data,
checksums, original-record correspondence and layout before writable initialization.

Migration preserves version-1 play/stop history and version-2 setup intent bytes. Old
acknowledgments remain readable with the acknowledgment field absent; no stream ID or other
response value is invented. An older reader refuses version 3. Restore the compatible
application and complete retained history together; do not downgrade a journal by changing
its version number.

## Verification scope

The 2026-10-21 campaign passes 321 Protocol, 14 setup-journal, 106 Handy Engine, 22 Handy
Client (one existing desktop case ignored) and 51 playback regression tests. Both strict lint
configurations, app/worker build, formatting and architecture/source checks pass. A missing
import and the playback test's old future-version fixture were corrected; failed runs remain
in the evidence packet.

The actual CLI preserves legacy acknowledgment without fabricating response data, returns an
authored response by original request ID and retains identical history across restart. The
preceding reader refuses version 3 with unchanged project/journal database and WAL bytes;
SQLite `-shm` sidecars change. Initial assertions that conflated these files are retained with
the corrected file-by-file comparison. This does not claim all application storage is unchanged.

Positive setup transport tests use private HTTP fixtures and Engine tests use injected
observations. Native CLI history is explicitly seeded. All owned Engines are joined. No real
Handy or physical acceptance is part of this campaign. See
[ADR B5-035](../research/phase-b/ADR-B5-035-retained-setup-observations.md) and
[checkpoint](evidence/roadmap-2026-10-21/handy-setup-observation/checkpoint.json).
