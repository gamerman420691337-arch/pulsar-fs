# ADR B3-033: custody precedes registration and spawn

Status: custody implemented; engine fence scope and reader barrier pending.

Registration can fail after inserting a roster entry, and process creation can fail before returning a child. Constructing ownership only after spawn leaves these transitions outside the retirement contract. Own the cgroup, reservation and optional lease before registration, permit one spawn attempt, and retain all ownership while durable retirement is unconfirmed.

Represent an unspawned tree explicitly. Only that state can retire a missing roster entry after actual empty-group observation; spawned retirement additionally requires direct-child exit. Clear the roster durably before removing the cgroup or releasing its resource claim, and record successful clear to make repeated retirement observation safe.

Use the existing reaper for registration, spawn, execution and clear failures. Never convert persistence failure into apparent retirement. The lease remains held through reaper custody and the fork-to-exec gap, with CLOEXEC unchanged.

Ordinary launch adopts pre-spawn custody without a fence. Activate the optional durable path only after engine state is bound to its scope and older database readers are barred. See the [contract](../../docs/PULSAR_WORKER_PRE_SPAWN_CUSTODY.md); native fault cases establish local custody behavior, not complete crash recovery or product acceptance.
