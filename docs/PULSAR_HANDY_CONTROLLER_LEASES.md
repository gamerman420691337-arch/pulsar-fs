# Acquire and maintain a Handy controller lease

The CLI can acquire a renewable local controller for an existing
[native preparation](PULSAR_HANDY_PLAYBACK_PREPARATION.md):

```sh
pulsar handy-controller-acquire PROJECT --revision PROJECT_REVISION \
  --preparation-request-id PREPARATION_REQUEST --preparation-id PREPARATION_SHA256 \
  --lease-ms 5000 --request-id ACQUISITION_REQUEST \
  --allow-arm --allow-online-inspection
```

The Engine checks current owner capability, project Read/Export access, preparation identity,
accepted qualification and stopping uncertainty, then performs fresh program correlation.
It creates a distinct controller ID and arms the native runtime without starting playback.
The preparation is now owned by that controller; preparation status reports it as retired
instead of offering the same inputs for another controller. Controller status is separate.

The result reports `lease.state: active`, `armed: true` and `playback_enabled: false`.
This is local software arming. There is still no physical play command in this workflow.

## Renew, inspect and release

```sh
pulsar handy-controller-renew PROJECT --revision PROJECT_REVISION \
  --acquisition-request-id ACQUISITION_REQUEST --controller-id CONTROLLER_ID \
  --generation GENERATION --request-id RENEWAL_REQUEST

pulsar handy-controller-status PROJECT --acquisition-request-id ACQUISITION_REQUEST

pulsar handy-controller-release PROJECT --acquisition-request-id ACQUISITION_REQUEST \
  --controller-id CONTROLLER_ID --request-id RELEASE_REQUEST
```

Use a new request ID for each intended renewal or release. Renewal requires the current
generation and an unexpired lease. It increments generation and uses the original duration.
Keep the returned generation for the next renewal. Repeating the same renewal request returns
current state without extending the deadline again. A stale generation cannot renew a newer
lease. A renewal whose database commit finishes after the old lease expires cannot revive it.

The duration must be 100–30000 ms. This range is a host authorization policy, not a physical
stopping guarantee. The CLI commands do not run an automatic heartbeat. An application
controlling the lease must keep renewing it deliberately while its controller is live.

Release needs current owner capability and project Read access. It pins the original
acquisition and controller ID, so repeating an old release cannot affect a replacement.
An unknown reply is recovered with the same request and selection. Status and replay never
recreate missing native state. After expiry, release or restart, acquire a new preparation
and controller under current facts; old receipts remain historical.

## Expiry and retirement

A named Engine-owned monitor expires leases without waiting for API traffic or the project
database lock. Database work does not hold the expiry registry. The monitor is wakeable and
joined during Engine retirement; its worker holds no Engine reference, avoiding a self-join
through final Engine destruction. Spawn failure prevents acquisition. An absent/stopped
monitor or a failed/backward controller clock retires authority before further controller use.

The Linux implementation uses `CLOCK_BOOTTIME`, which includes suspended time. Other platform
clock backends remain unsupported for acquisition. This clock is distinct from Handy server
time, device HSTP synchronization and browser presentation timing. [Linux clock documentation](https://man7.org/linux/man-pages/man2/clock_gettime.2.html)

All preparation retirement boundaries also revoke local controller arming: project/grant,
connection, registration, qualification and hosting changes. Expiry and release cannot clear
physical uncertainty. If a runtime unexpectedly requires a physical stop, its uncertainty and
stop intent remain retained; physical dispatch/reconciliation must be implemented before play
is enabled. No host timer or successful release receipt proves that hardware stopped.

## Verification scope

The 2026-10-22 scoped campaign passes 327 Protocol, 125 Handy Engine, 25 Handy Client (one
existing desktop case ignored), 14 setup-journal and 51 playback regression tests. Both strict
lint configurations, app/worker build, formatting and architecture/source checks pass.

Engine cases cover renewal/replay/expiry, late commits, all retirement boundaries, replacement
controller protection, monitor spawn/loss/shutdown and clock reversal. Idle expiry proceeds
while the DB mutex is held. A separate SQLite-writer campaign verifies expiry while renewal
is refused or delayed; the original fixture incorrectly assumed SQLite would always wait,
and that failure is retained. No late renewal revives the expired controller.

The actual CLI recovers a dropped committed release reply, preserves exact release after
restart and refuses acquisition without consent/preparation or renewal without native state.
Native history is explicitly authored, setup/effect journals remain empty and all owned
Engines are joined. Real suspend/device behavior and synchronization remain qualification
work. See
[ADR B5-038](../research/phase-b/ADR-B5-038-renewable-local-controller-leases.md) and
[checkpoint](evidence/roadmap-2026-10-22/handy-controller-leases/checkpoint.json).
