# Cancel an unclaimed Handy setup

The CLI can release a setup preparation that has never been claimed for sending. First
inspect the original request:

```sh
pulsar handy-setup-status PROJECT --setup-request-id ORIGINAL_SETUP_REQUEST
```

Use the returned `history.record_id` and a separate cancellation request ID:

```sh
pulsar handy-cancel-setup PROJECT \
  --setup-request-id ORIGINAL_SETUP_REQUEST \
  --record-id RECORD_SHA256 --request-id CANCELLATION_REQUEST
```

Keep this exact selection for recovery. The current client session must be the original
setup actor and must have the private Handy owner capability and current project Read grant.
No current project revision, active connection, source file, qualification or vendor
credential is needed. This operation changes local metadata and makes no network request.

## What cancellation means

Only `prepared` history can transition to `cancelled`. Already-cancelled history can be
acknowledged again without changing another setup. The exact record pin prevents a request
from silently selecting a different preparation. `claimed`, `acknowledged` and `unknown`
attempts refuse cancellation and keep their reservations. A successful cancellation does
not send stop, retract a remote request or establish physical device state.

The response has its own `request_id`, the original setup `status`, and `settled: true`.
The nested status remains `loaded: false` and `qualified: false`. Cancelled history is
retained permanently under its original actor/request identity; it cannot be repurposed
for a new setup.

## Recover a lost reply

Repeat the same command with the original cancellation ID, original setup ID and exact
record pin. The Engine reserves the cancellation intent before changing the journal, then
records a settled receipt. A failure between these writes can leave a cancelled setup with
an unsettled cancellation receipt. An exact retry finishes that receipt without touching a
later replacement preparation. Changed selection under the same cancellation ID refuses.

A settled repeat returns its historical receipt after checking current authorization. It
does not re-run cancellation or describe the state of a replacement setup. If the original
record is missing, do not infer that a device attempt was absent; investigate the retained
history and storage failure.

## Verification scope

The 2026-10-21 campaign passes 320 Protocol tests, 11 setup-journal tests, 106 Handy Engine
tests and 22 Handy Client tests with one existing desktop case ignored. Both strict workspace
lint configurations, application/worker build, formatting and architecture/source checks pass.
Engine tests cover exact selection, attempted-state refusal, authorization and reservation/
receipt-write failures, including a replacement preparation protected from an old retry.

The actual CLI campaign uses authored journal fixtures and a local socket proxy to drop a
committed cancellation reply. The first caller reports a transport error; an exact retry
recovers the settled receipt, and another repeat after Engine restart preserves it. A claimed
setup refuses cancellation without creating a cancellation receipt. Both owned Engines are
joined. The network namespace contains only loopback; no real Handy operation or physical
acceptance is part of this evidence. The earlier interrupted validation is retained separately.
See [the checkpoint](evidence/roadmap-2026-10-16/handy-setup-cancel/checkpoint.json) and
[ADR B5-034](../research/phase-b/ADR-B5-034-exact-unclaimed-setup-cancellation.md).
