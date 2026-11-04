# Read-only Handy device timing inspection

The CLI can inspect device uptime, the reported server/device offset, the RTT from the latest
synchronization and the user-adjusted timing offset for an exact active controller:

```sh
pulsar handy-inspect-device-timing PROJECT --revision PROJECT_REVISION \
  --acquisition-request-id ACQUISITION_REQUEST --controller-id CONTROLLER_ID \
  --generation GENERATION --request-id INSPECTION_REQUEST --allow-online-inspection
```

This requires an existing current preparation and controller, its accepted preparation
qualification, owner authority and project Read/Export access. It does not require or grant
server/device synchronization acceptance. The command is read-only and every call performs
fresh inspection; an old report is never returned as a substitute for missing native state.
It does not renew the controller or create a persistent request receipt.

The Engine checks identity, mode, operative settings and stopped HSSP state before and after
two timing samples. Each sample contains two separate GETs: `/hstp/info`, then `/hstp/offset`.
It records an overall host-clock bracket for each pair, not an atomic device snapshot. The
whole operation shares a thirty-second network deadline and remains subject to the controller's
earlier expiry. Database, controller-registry and effect-journal locks are released during I/O.
Credential snapshots remain Engine-owned; response text cannot echo their values.

The final result rechecks current project, registration, connection, qualification and hosted
references. Context mismatch, backward device uptime or a failed read retires only the selected
controller generation. A late failure from an old generation cannot retire a renewed controller.
Observed context equality does not exclude unseen changes between reads or establish continuous
device liveness. Changing reported offsets/RTT remain visible in the two samples.

## What the numbers establish

`device_uptime_ms` is time since boot. `reported_clock_offset_ms` is the manufacturer's reported
clock offset, and `last_sync_round_trip_ms` is the RTT from its latest synchronization. The user
offset is a separate setting. Pulsar preserves signed integral offsets and accepts only exact
numeric magnitudes up to 2^53−1. These are application bounds, not manufacturer accuracy limits.

The official GET offset schema references a nonnegative timestamp type, but its own examples
include `-125`. Pulsar follows those signed examples while retaining the GET's integer
requirement: fractional values are refused, never rounded or clamped. The offset's algebraic
sign convention is not inferred. [Official schema](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)

The API supplies no last-sync time, synchronization age or accuracy bound. The report therefore
always has `synchronization_verified: false` and `playback_enabled: false`. A low RTT or matching
samples cannot establish synchronization quality. The preparation qualification reference is
not an acceptance of these new timing fields. No default device error budget is invented.

The inspection broker excludes `/hstp/clocksync`, including its synchronous variant. Although
that endpoint uses GET, it initiates synchronization and requires a separate explicitly
authorized workflow. The inspector also never writes the user offset, plays or stops a device.

The [evidence packet](evidence/roadmap-2026-10-22/handy-device-timing/checkpoint.json) records the
scoped software campaign. Real device timing, qualified synchronization and combined error
budgets, owned play/stop dispatch, browser correspondence and actual acceptance remain open.

The scoped campaign passes 333 Protocol, 144 Handy Engine, 27 Handy Client (one existing desktop
case ignored), 14 journal and 51 playback-runtime tests, both strict Clippy configurations,
app/worker build, formatting and architecture/source checks. Local HTTP fixtures verify exact
GET ordering, signed fields, context changes and credential-echo withholding. The native CLI
campaign uses only seeded history with external networking disabled; valid old reports remain
unusable after restart. The original incorrect fixture fingerprint and its corrected run are
both retained. No real timing inspection or synchronization was performed.
