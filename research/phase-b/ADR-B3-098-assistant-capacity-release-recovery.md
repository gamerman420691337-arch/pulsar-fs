# ADR B3-098: retry owned assistant recovery after capacity release

Decision: after authenticated asynchronous completion retains pending recovery, schedule one weak Engine recovery thread. Register a bounded resource-release watcher before its first retry to fence the scheduling race. Coalesce external CPU/RAM/GPU/storage retirement notifications into one queued wake-up; the watcher ignores releases originating on its own thread to avoid self-induced retry loops. Watchers have bounded capacity and remove their registration on drop.

Retry the existing completion and ordinary authenticated ResumeJob path after a release. Preserve current grants, original ownership, checkpoints, native/GPU admission and normal candidate review. Do not repeat inference or claim resources released before accounting custody retires. A timeout on notification checks only Engine lifetime and does not poll the database. Reset the singleton flag and recheck pending completion on exit to fence concurrent insertion. Metadata refusal retains the existing explicit observation-driven fallback.

Consequences: unrelated capacity retirement can resume owned paused generation without client status polling. The recovery wait does not keep an Engine alive or persist credentials. Authored accounting tests and native learned assistant execution are separate evidence; neither alone qualifies full learned offline/live contention, physical playback, ordinary storage reliability or device acceptance.

See [recovery contract](../../docs/PULSAR_ASSISTANT_CAPACITY_RECOVERY.md).
