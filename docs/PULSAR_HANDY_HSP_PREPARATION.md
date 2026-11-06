# Prepare Handy HSP synchronization

Pulsar exposes preparation, status and cancellation for one explicitly identified
HSP synchronization. Preparation can retain a current native server/device clock
for a later source handoff. Every preparation response keeps `armed` and
`playback_enabled` false. Public generated-stream Start, accepted-motion production
and browser playback controls remain unfinished.

## Select an existing connection and accepted qualification

Use the current Engine epoch and an existing observed device session from the
[connection workflow](PULSAR_HANDY_CONNECTION_SESSIONS.md). `pulsar handy-sessions`
returns those observations without reconnecting. The Engine must retain the
credential snapshot for that session; credential values are not CLI arguments.

Preparation requires the exact current project revision, owner capability,
registration and accepted physical HSP qualification decision. The
[HSP qualification guide](PULSAR_HANDY_HSP_RECOVERY.md) describes subject version 7
and its explicit server/device clock, settings, queue and recovery contracts.
Hosted qualification and synthetic test evidence cannot substitute for that
acceptance. The device must already report HSP mode 4 and an idle state
(`not_initialized` or `stopped`); preparation does not change its mode.

```sh
pulsar handy-hsp-prepare PROJECT --revision PROJECT_REVISION \
  --engine-epoch ENGINE_EPOCH --device-session DEVICE_SESSION \
  --qualification-id HSP_DOSSIER_ID --qualification-revision ACCEPTED_REVISION \
  --timeout-ms 10000 --request-id ORIGINAL_PREPARATION_REQUEST \
  --allow-online-synchronization
```

Replace the uppercase values with the captured selection. `--timeout-ms` accepts
1 through 60000 milliseconds and has no implicit default. The example selects a
ten-second total native preparation budget; accepted clock and settings freshness
may expire earlier. The client reads the private Handy owner capability using the
existing local client mechanism.

The Engine reserves the original actor/request/fingerprint before starting owned
work. It inspects identity, mode, operative settings and two device-time/user-offset
samples; checks the accepted subject; and durably claims the synchronization before
sending the single effectful `GET /hstp/clocksync?s=true`. The transport uses the
captured credentials and fixed vendor endpoint without redirects or automatic
retries. Preparation sends no HSP Setup, Append, Play, Stop or offset-write command.

After an acknowledgment, fresh device timing must remain consistent with the
original identity, settings, offset and synchronization. An independently acquired
server-time sample supplies the server clock. Conservative combined uncertainty,
original synchronization age and current acceptance determine whether native
readiness can be retained. A synchronization reply alone does not establish it.

## Read the original operation

```sh
pulsar handy-hsp-preparation-status PROJECT \
  --preparation-request-id ORIGINAL_PREPARATION_REQUEST
```

Status performs no remote inspection or synchronization. Its two observations have
different purposes:

- `history` retains the original record, qualification, deadline, acknowledgment or
  uncertainty, and any child-stream handoff. States are `prepared`, `claimed`,
  `acknowledged`, `unknown` and `cancelled`.
- `native.phase` describes the current process: `inspecting`, `synchronizing`,
  `checking`, `ready` or `retired`. `worker_active`, `metadata_pending`,
  `cancel_requested` and `error` expose outstanding work. A ready observation carries
  `clock_valid_until_boottime_ns` within the original deadline.

A reservation can initially have `history: null`. After restart, `native` is absent;
recorded acknowledgments cannot reconstruct a clock or restore readiness. Unresolved
claims from an earlier Engine boot become Unknown while original record bytes remain
unchanged. Startup validates persisted history witnesses against the device journal.

Retry a lost Prepare reply with the same request ID and every original argument.
Changed inputs conflict. Exact replay reports the existing operation and can retry
its pending metadata on an owned worker. It cannot repeat synchronization or acquire
new clock samples. An already captured candidate can become ready only if its original
metadata completes and its original clocks and dependencies are still valid.

Fresh Prepare samples its starting clocks before acquiring dependency locks or doing
storage work. Lock contention and native reads consume that original budget. A failed
starting-clock read prevents fresh preparation, but does not prevent historical replay.
Status and cancellation create no replacement budget. Completion metadata can remain
pending after native work ends; later persistence never renews clock authority.

## Cancel without erasing attempted effects

```sh
pulsar handy-hsp-preparation-cancel PROJECT \
  --preparation-request-id ORIGINAL_PREPARATION_REQUEST \
  --request-id CANCELLATION_REQUEST
```

Use a distinct cancellation request ID. Cancellation retires native readiness and
requests cancellation of owned work. `worker_active` may remain true until the worker
settles. Only an original preparation proven unclaimed can become `cancelled`; that
path sends neither clocksync nor Stop. A possibly committed claim is retained as
Unknown. Existing acknowledgments and Unknown outcomes remain unchanged.

If the worker has finished but `metadata_pending` remains true, replay the same
cancellation request to retry only its journal work. Status reads do not perform
that retry. Keep the original cancellation ID and arguments; a metadata retry never
repeats synchronization or renews native readiness.

If readiness has already been consumed by a child stream, cancellation retires that
child through its existing owner. It does not erase the child link, resolve physical
custody or imply that a toy stopped. Registration, connection, qualification and
project changes, and Engine shutdown, also retire dependent native preparation.
Shutdown owns the joins; blocked storage can prolong completion, so the timeout is
not a physical stopping or shutdown-latency guarantee.

## Consume readiness through the native source handoff

The Engine has a private handoff into the existing HSP owner. It requires separate
online-control consent and an independently admitted native source authority. The
source must match the actor, project/revision, device session and retained clock
domain. The [capture clock correspondence](PULSAR_LIVE_CAPTURE_CLOCK.md) is a separate
source prerequisite; preparation itself grants no source or presentation authority.

The handoff checks the exact retained server time and full combined uncertainty for
the source's original reference. The final claim gate repeats that correspondence
check before and after durable claim I/O; transport retains the original validity,
deadline and retirement checks. Fresh-looking source facts cannot replace the
prepared clock, shrink its uncertainty or extend its original expiry.

Consumption retires preparation readiness and binds one original child request.
Journal version 12 records the exact preparation-to-stream link. Route custody
transfers only in the same transaction as that child's first durable Setup claim.
A linked Prepared or Cancelled child leaves synchronization custody with the parent.
The child still needs its independent point queue, explicit Play and current source
checks; the public preparation commands do not perform that handoff.

## Preserve and recover exact custody

Version 12 adds bounded preparation records, links and resolution membership while
preserving v11 stream, effect, qualification-link and resolution bytes. Empty
preparation extensions retain earlier serialized identities. An older reader must
refuse the newer device journal before opening it for writes.

Unresolved preparations participate in the shared device, keyed-route and namespace
exclusions. Aliases on the same route cannot bypass them. The existing
[reconciliation workflow](PULSAR_HANDY_HSP_RECOVERY.md#recover-only-the-captured-custody)
now captures preparations together with their exact children, including a cancelled
linked child while its parent remains unresolved. It drains native preparation,
metadata and stream work, checks fresh samples under the accepted HSP recovery
contract, and commits one matching aggregate resolution.

Resolution preserves original synchronization uncertainty and makes old receipts
unusable. Its verified receipt releases only the exact captured native owners.
History-only recovery remains possible after restart, but never restores native
readiness or sends a synchronization request. Fresh work requires a new original
request and current dependencies after conflicting custody is resolved.

## Evidence and remaining work

The [preparation checkpoint](evidence/roadmap-2026-10-23/hsp-preparation/checkpoint.json)
records exact source identities, commands, focused preparation/HSP/protocol/client
checks and retained failures. Consult its final disposition for strict lint, full
Engine validation and the actual v11/v12 migration, refusal and restart campaign.
This guide does not independently claim those broader validation results or
storage-latency qualification.

Positive preparation and handoff scenarios use authored test evidence and local
fixtures. No actual Handy was contacted or qualified. Useful learned motion,
real-media timing, production accepted-source integration, browser lookahead and
presentation, Handy 2 Pro firmware/device measurements, sustained latency, and the
remaining M1–M8 platform, six-axis, human and release gates remain open. The full
roadmap is partial.

See [ADR B5-060](../research/phase-b/ADR-B5-060-hsp-preparation.md).
