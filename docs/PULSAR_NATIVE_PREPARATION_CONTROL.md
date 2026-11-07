# Authenticated native preparation control

Pulsar now exposes asynchronous native image preparation through its engine protocol, CLI and desktop Capabilities panel. Preparation retains verified native bytes; it does not establish inference readiness or device authority. Live capture still uses its original lease. The trusted embedding API remains available independently of the operation journal.

Start requires current authentication, Read and Generate grants, an exact current project revision and the inspected engine epoch. Options must fit the protocol bounds and independent owner preparation maximum. Linux CUDA support and a verified owner policy are prerequisites. The controller issues the preparation clock before journaling or spawning and passes the same absolute deadline into capture. Retention uses the producer's actual creation timestamp, including time spent waiting for status publication; polling cannot renew either clock.

The SQLite transaction commits the queued acknowledgement, request fingerprint, operation intent and Preparing transition before the producer starts. Repeating Start returns the frozen original acknowledgement and does not start another producer. Resolve uses the owner and original Start request ID to retrieve current status after a lost reply. Status, cancellation and release require current authentication, Read and current revision; ownership stays bound to the original session/project. A new panel can resolve an operation by entering the original request ID. Restart marks unfinished reports Interrupted and restores no retained handle.

A background monitor checks the original authenticated Start authority and project revision every 250 milliseconds, and explicit controller calls also poll. Revocation, changed credentials/revision or an expired preparation deadline cancels the producer and records failure. Cancellation commits CancelRequested before setting the cancellation flag. Late results cannot publish Retained. Retained completion must match the engine epoch, image specification digest and declared size in the durable intent. Release disposes the retained use; expiry uses the original retention deadline. Neither action terminates other workers borrowing the image.

There are at most eight active operations, eight retained uses and 4096 durable operation records. Each producer and the single weak-owner monitor admit 512 KiB of RAM and a 256 KiB thread stack. The existing retained-use watcher has its separate admission. A producer's claim survives until its thread is joined. Journal errors retain custody for retry. Image disposal runs outside controller and database locks.

Cleanup confirmation is conservative: the producer must have joined, the operation must be terminal, no retained handle may remain, the engine's reserved storage must be zero, and its worker fence must have no worker, retired, creation or staging obligations. Another operation's storage can delay confirmation. A Released or Expired report with `cleanup_confirmed: false` must never be treated as proof of physical deletion. Historical Interrupted reports do not invent cleanup receipts.

CLI examples use metadata only:

```sh
pulsar native-preparation limits PROJECT --revision 0
pulsar native-preparation start PROJECT --revision 0 --engine-epoch EPOCH --preparation-ms 180000 --retention-ms 300000 --request-id ORIGINAL_ID
pulsar native-preparation resolve PROJECT --revision 0 --original-request-id ORIGINAL_ID
pulsar native-preparation status PROJECT OPERATION --revision 0
pulsar native-preparation cancel PROJECT OPERATION --revision 0 --request-id CANCEL_ID
pulsar native-preparation release PROJECT OPERATION --revision 0 --request-id RELEASE_ID
```

Resolve and Status are current observations. Mutating acknowledgements are frozen receipts; inspect current status after a replay. Cancel is for preparing operations; Release is for retained ones. The desktop preserves pending request identities for explicit same-request retry and rejects replies from an earlier project/revision. The CLI and desktop durably record Start identities before dispatch. Reopening the panel lists saved identities for explicit resolution under the current project revision; it never automatically issues Start or Resolve. Records preserve the original epoch, revision and options. Local forgetting does not cancel or release engine work. See [client recovery](PULSAR_NATIVE_PREPARATION_CLIENT_RECOVERY.md).

Verification and outstanding gates are recorded in the [checkpoint](evidence/roadmap-2026-10-10/native-preparation-control/checkpoint.json). File-custody tests use authored non-executable bytes and do not execute CUDA. Native RPC capture, actual desktop window rendering, live Stash, useful training/held-out media quality, Handy firmware/key configuration and physical stop/sync qualification remain unverified by this increment. Global lint/format, platform/release and human acceptance remain independent gates. Full roadmap status remains PARTIAL.

Final verification: 703 engine tests pass with 79 ignored; client suites pass 188 tests with four ignored; the full protocol suite passes 228 tests. Default, WASM and all-feature checks, paired desktop/worker build, protocol/client strict Clippy, scoped formatting, 348 structural checks and 36 source guards pass. The workspace campaign fails at two portable-desktop startup timeouts; a separate serial retry passes all four cases. This does not establish that the startup failures are repaired. Engine strict Clippy still fails with 47 library and 94 test-target errors. Native RPC execution and actual desktop rendering were not run.


Production transport qualification update: the new SessionClient native campaign fails at its first 180-second preparation deadline after successful Start/replay/resolution. Live GPU inference and cleanup receipt are not reached. See [native preparation process](PULSAR_NATIVE_PREPARATION_PROCESS.md); the existing cold post-Finish case remains unchanged. Roadmap status remains PARTIAL.


Subsequent native verification: two unchanged-source production SessionClient campaigns pass retention, replay/resolution, shared-use release, three GPU estimates, Finish and checked cleanup. I/O profiling still finds long writes and fsync stalls; the earlier deadline failure and load-independent reliability remain open. See [native preparation I/O profile](PULSAR_NATIVE_PREPARATION_IO_PROFILE.md). No useful model/media quality or physical-device qualification is implied.
