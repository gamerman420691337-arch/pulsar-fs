# Paired export: real lease acknowledgement loss

2026-10-11. Scoped owned Linux IPC recovery **PASS**. Full roadmap **PARTIAL**; physical, model, media, platform and human qualification remain open.

`scripts/qualify-motion-evidence-interruption.py` runs an owned Engine and an authored two-second triangle project in temporary private state. A bounded frame proxy forwards authenticated control requests without retaining their authentication or lease tokens. Once the actual Engine returns a download lease, the proxy drops its acknowledgement. All three automatic attempts use the same Begin identity and refer to the same operation. This is an actual accepted-lease response loss, rather than a manually written interruption marker.

The export fails with its durable private intent and transfer marker retained, no completed project member, and no published destination. A subsequent paired resume refuses ambiguous Begin replay before another Begin reaches the Engine. After the proxy is stopped and the owned Engine endpoint restored, explicit `package-download` with the original Begin identity recovers the unconsumed lease. Paired resume retains the original intent bytes and publishes the checked association. Independent script readback matches all 17 authored actions; project master/revision remain unchanged. The Engine package copy is not released.

The owned Engine, proxy accept thread and every connection handler join. The retained [checkpoint](evidence/roadmap-2026-10-11/motion-evidence-interruption/checkpoint.json) contains the safe receipts, exact executable/source identities and campaign script. Temporary Engine state and authentication are not retained. No production Rust behavior changed and no Cargo checks were rerun for this Python campaign/documentation increment; preceding checks remain attached to their original subject.

The probe accepts `--app ABSOLUTE_EXECUTABLE --output NEW_EVIDENCE_DIRECTORY` and `--fault lease-ack` (default) or `--fault bulk-chunk`. Run it with an explicit process timeout and private temporary storage. It refuses an existing evidence directory. It operates only on its own temporary Engine sockets and never redirects a user Engine. Failed assertions return failure while its cleanup still joins owned processes and retains diagnostic receipts.

## Actual first-chunk interruption

The later [partial-chunk checkpoint](evidence/roadmap-2026-10-11/motion-evidence-bulk-interruption/checkpoint.json) exercises the authenticated bulk connection against an owned Engine. A proxy forwards the lease handshake and real download header, then delivers only 128 bytes of each declared 12,337-byte package chunk. Both the original connection and its one automatic reconnect fail at offset zero. The client removes its private incomplete member and abandons the lease; the staging directory retains only the original intent, transfer marker and script. Paired resume refuses without another Begin reaching the Engine.

Explicit old-Begin recovery receives the Engine's abandoned/expired/prior-epoch refusal. A deliberate fresh `package-download` succeeds against the same retained package operation; paired resume then preserves the original intent, verifies the package/association and publishes all members. The independent 17-action script and unchanged master checks pass. All owned processes and threads join. The final updated probe also reruns the lease-acknowledgement scenario; production Rust identities and application binary remain unchanged.

These probes cover lost lease acknowledgements and an interrupted first chunk. Interruption after fully accepted earlier chunks, client/Engine crashes, crashed-client temporary-member recovery, lease expiry, filesystem faults and real power-loss durability remain separate work. Synthetic triangle evidence is not real-media motion accuracy or device playback qualification.

## Client process death after an accepted prefix

The subsequent [client-death recovery work](PULSAR_EXPORT_CLIENT_DEATH.md) addresses a different fault: SIGKILL after a fully accepted 256 KiB chunk leaves a private download stage that normal destructor cleanup cannot remove. Bound local receipts let paired resume reconcile matching inactive stages after explicit complete-package recovery. Its current evidence and remaining crash/durability limits are recorded separately; the earlier probes retain their original executable subjects.
