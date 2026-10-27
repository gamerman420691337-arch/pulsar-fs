# Actual client death during pending metadata retirement

The [acknowledgement-covered retirement service](PULSAR_LIBRARY_PENDING_RETIREMENT.md)
requires an exact original path/digest retry after uncertain move confirmation.
This campaign exercises that behavior through the actual CLI and Engine rather
than restoring journal bytes or fabricating an acknowledgement.

A genuine pending file first arises from actual CLI death before the publication
record rename. Engine restart and original-request replay recover its accepted
Sway export without duplication; remaining channels complete. The resulting
pending file is covered by the completed current journal.

The retirement CLI is then traced with only `renameat2` metadata. Its single
no-replace original-to-retained rename is delayed at entry. The exact application
PID, private fixture namespace and traced stop are checked before SIGKILL. The
original file remains, the retained name is absent, and both current journal bytes
and database counts remain unchanged. Its tracer joins and confirms SIGKILL.

A second original-path retirement attempt delays the same syscall at exit. The
original name is now absent and the retained name holds the exact original bytes.
The client is killed while still stopped before the application's post-rename
verification/directory-sync acknowledgement. The tracer confirms SIGKILL and
joins. Current journal bytes and database counts again remain unchanged.

After an owned Engine restart, the exact original path and digest return a
synced `already_retired: true` receipt. Actual desktop-controller and further CLI
repeats inspect the same retained name without new generation/export effects.
No journal/request bytes are rolled back or rewritten by the harness. The ordinary
negative cases and desktop constructed-duplicate retirement regression also pass.
Seventeen outputs across four jobs retain the independent authored PCM/timestamp
oracle; existing scripts and unknown owner members remain unchanged. Reader floor
34 is directly queried. Three deliberately killed clients/tracers and three owned
Engines are joined.

All 538 recorded production source identities and three native App/Worker/controller
hashes match the preceding retirement checkpoint. Its passing 409 Client tests
(ten ignored), both strict lints, Windows source check, builds, formatting,
architecture and 36 guards retain that exact scope; no fresh Cargo suites run in
this campaign. Fixture credentials/database/journals exist only in owned private
RAM. Retained syscall traces contain rename metadata, not IPC credentials.

This is process-interruption recovery on Linux private RAM. It does not establish
power-loss survival, native Windows/Android execution, hostile same-owner writer
resilience, all filesystem durability, rendered GUI acceptance or actual desktop
writer-death coverage. The two interrupted retirement clients are CLI processes;
the desktop controller verifies recovery afterward. Unknown effects, missing
positive receipts, general orphan disposal, useful model/media quality, Handy,
human and release qualification remain open. Full roadmap PARTIAL.

See [checkpoint](evidence/roadmap-2026-10-12/library-retirement-client-death/checkpoint.json).
