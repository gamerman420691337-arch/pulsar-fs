# Setup history recovery and private route identity

Resolve history using the original setup request ID:

```sh
pulsar handy-setup-status PROJECT --setup-request-id ORIGINAL_SETUP_REQUEST
```

The current client session must be the original actor. The command also requires the private
Handy owner capability and a current project Read grant. It does not need the old receipt ID,
source files, active device connection or vendor credentials. A wrong project refuses; a
missing record returns `history: null` for this actor/project/request selection. Absence is
not proof that no operation occurred elsewhere and does not authorize a retry.

The result includes the historical boot/session, original revision, script/download identities,
qualification reference and journal state. `loaded` and `qualified` are always false. The
new journal also returns [retained setup response fields](PULSAR_HANDY_SETUP_OBSERVATIONS.md)
when they exist; legacy acknowledgments keep that field absent. The
command performs no network request, creates no mutation receipt and changes no setup state.
It cannot reconstruct a claim, replay setup or establish current device state. [Guarded setup sending](PULSAR_HANDY_SETUP_SEND.md) is now available; attempted-effect
reconciliation and loaded-program/playback integration remain unfinished.

## Private routing

The native route helper uses HMAC-SHA256 from pinned `ring` 0.17.14 under the existing private
Handy owner capability. The exact captured connection-key bytes identify the credential route;
API-key changes and different registration names do not change that identity. No case folding
or other equivalence across distinct key strings is inferred. The helper returns keyed route and namespace
identifiers for private journal use, not credential values or unkeyed credential digests.
The setup sender takes both identifiers from the captured native helper;
the journal has no credentials with which to recompute them.
The status response does not expose those routing identifiers.

New setup metadata must include a valid route namespace. All unresolved records must agree
with it before a preparation can proceed. Replacing the owner capability cannot bypass active
history. A new connection key also cannot bypass an outstanding setup for the same logical
device. Identifying one physical device across both a changed key and a changed registration
remains a separate transport/qualification responsibility.

Legacy records without a namespace remain readable with their original identities. They
block new preparations until safely reconciled; a provably unclaimed record may be cancelled
through [exact unclaimed cancellation](PULSAR_HANDY_SETUP_CANCELLATION.md).
Claimed or uncertain attempts cannot be cleared that way.
Cancelled history cannot be rebound to a different actor/request payload. The route-metadata
extension originally retained layout version 2. Current
[version-3 storage](PULSAR_HANDY_SETUP_OBSERVATIONS.md) additionally retains acknowledged
response data; preceding readers refuse the newer version.

See [journal behavior](PULSAR_HANDY_SETUP_JOURNAL.md),
[ADR B5-032](../research/phase-b/ADR-B5-032-private-routes-and-setup-recovery.md) and
[scoped checkpoint](evidence/roadmap-2026-10-16/handy-setup-recovery/checkpoint.json).

## Scoped verification

Protocol checks pass 318; final checks pass 11 setup-journal tests, 93 Handy Engine tests
and 20 Handy Client tests with one existing desktop case skipped. Strict workspace lint
configurations, application/worker build, format and architecture checks pass. HMAC output
matches an independently calculated fixture. Native CLI checks use an explicitly authored
claimed record to verify original-request lookup without a receipt ID, read-only repeats,
wrong-project refusal and history after restart/owner-capability replacement. The preceding
reader rejects the new field without changing database/WAL bytes. This is seeded recovery
evidence; no actual setup sender, real device contact or physical acceptance is exercised.
