# Combined native playback timing

Pulsar can bind a fresh controller to a successful current-process synchronization and a new
server-clock sample. The resulting native clock carries both error components and the original
source expiry. This prepares timing facts; physical playback remains disabled.

```sh
pulsar handy-playback-clock PROJECT --revision PROJECT_REVISION \
  --acquisition-request-id ACQUISITION_REQUEST --controller-id CONTROLLER_ID \
  --generation GENERATION --sync-request-id SYNC_REQUEST --sync-record-id SYNC_RECORD \
  --request-id CLOCK_REQUEST --allow-online-timing
```

The required sequence is: accepted version-4 qualification, setup/preparation/controller,
explicit clock synchronization, fresh preparation/controller for the same setup, then this
clock acquisition. Synchronization retires its initiating controller. The later controller
must match the underlying setup, connection, registration, qualification, owner, project
revision and hosting; it deliberately has a different controller/preparation identity.

Acquisition checks fresh device time and user-offset observations before sampling public server
time. Reported device clock offset and latest-sync RTT must still match the original synchronous
response, device uptime must not move backward, and both user-offset reads must match the
accepted contract. No clock-offset sign convention or accuracy estimate is inferred from RTT.
The command does not write offsets or issue another synchronization.

## Explicit version-4 contract

Qualification subject version 4 adds required `device_clock` alongside the version-3
`server_clock` and operative settings. The following illustrates fields only. These numbers
are not defaults, measured bounds or an accepted profile:

```json
{
  "model": "synchronous_response_effective_clock",
  "user_offset_ms": 0,
  "device_error_ns": 2000000,
  "maximum_sync_age_ns": 30000000000,
  "maximum_sync_round_trip_ms": 100,
  "drift_ppm": 20
}
```

The model declares a bound on the effective device playback clock against the same server
domain at synchronous response completion, including the declared user offset, with bounded
subsequent relative drift and no unobserved clock corrections during validity. Real evidence
and operator review must substantiate that declaration. The RTT ceiling limits applicability;
it does not derive the device error. Zero reported RTT does not establish zero error.

The total must fit the existing profile's `maximum_clock_uncertainty_ns`:

```text
combined error = aged host/server uncertainty + aged device uncertainty
```

Both static error allowances must fit that single budget, and acquisition checks measured
request duration, grid rounding and accumulated drift as well. The runtime uses the sum of
the two qualified drift rates when aging a combined token. Versions 1–3 stay readable but gain
no implied device contract or native synchronization authority. A new contract changes the
qualification and configuration identities; old acceptance does not carry forward.

## Lifetime and recovery

A native synchronization context is reserved before handoff and completed only from the actual
callback after its journal acknowledgment commits. It is bounded, not deserializable, and
separate from the consumed controller. Up to 64 pending/completed contexts are retained.
Controller release, expiry or renewal does not reset synchronization age. Renewal and a new
server sample discard the derived combined clock, which must be acquired again if needed.

Dependency retirement also covers pending synchronization when its original preparation is
already gone. Session close, registration changes, qualification decisions, hosting retirement,
project revision and grant loss invalidate it. Revocation followed by regrant cannot revive a
pending callback. New synchronization invalidates preceding timing, including when its outcome
is unknown. Changed timing/offset observations invalidate the native synchronization rather than
silently treating them as a fresh calibration.

Synchronization freshness starts at its original request transmission. Server freshness starts
at its own request transmission. The native clock token preserves the earlier absolute source
expiry even when its estimated server time is reanchored. The playback runtime checks that
deadline as well as command age and drift; reanchoring cannot extend synchronization validity.
The dispatcher now reads the same suspend-aware host clock at claim and around HTTP,
and limits play by original source, command and lease expiry. Its future Engine
integration must also recheck current native dependencies and own completion/stop work.

The clock receipt commits before native publication, followed by another expiry/dependency
check. Original retry returns the same timestamps and reports whether that exact native binding
remains current; it does not repeat online acquisition. After restart, a saved report is only
history. Neither a JSON report nor a journal acknowledgment can reconstruct native completion.

## Verification scope

The [checkpoint](evidence/roadmap-2026-10-22/handy-playback-clock/checkpoint.json) records the
synthetic positive sequence through synchronization, controller replacement and combined
acquisition; arithmetic and single-budget checks; retirement, changed-offset, storage, replay,
revocation/regrant and restart cases. The native CLI campaign uses seeded history with external
networking disabled to check lost replies, unchanged timestamps and refusal to restore state.
No real synchronization, timing qualification, device playback or human acceptance occurred.

The final scope passes 338 Protocol, 166 Handy Engine, 29 Handy Client (one existing desktop
case ignored), 19 journal and 52 playback-runtime tests, strict default/all-feature Clippy,
app/worker build, formatting and architecture/source checks. The corrected administrator-scope
fixture failure and earlier results preceding the runtime expiry reinforcement are retained.

Owned play/stop dispatch and completion handling, uncertain-effect reconciliation, browser
presentation mapping and actual device/model/media/platform/human/release qualification remain
open. A current combined clock is not a complete playback or release qualification.
