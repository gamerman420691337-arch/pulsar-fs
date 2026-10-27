# Native Handy server-clock observations

The CLI can sample the manufacturer's public server clock for an exact current controller.
The Engine brackets the complete response with Linux suspend-aware host ticks and retains a
native mapping. The JSON reply is historical data and cannot restore that mapping.

```sh
pulsar handy-sample-server-clock PROJECT --revision PROJECT_REVISION \
  --acquisition-request-id ACQUISITION_REQUEST --controller-id CONTROLLER_ID \
  --generation GENERATION --request-id SAMPLING_REQUEST --allow-online-sampling
```

Use a new sampling request ID to acquire another sample. Repeating the exact original command
returns its original timestamps and checks whether that same native mapping remains current;
it never repeats the GET or refreshes the sample's age. A failed attempt without a committed
receipt can be retried. Sampling does not renew the controller lease. A new attempt retires
the previous native sample, even if the new attempt fails. Renewal requires a new sample.

The fixed-origin `GET /servertime` carries no API key, connection key, cookies or owner proof.
It uses a ten-second deadline, a 1 KiB response limit, no proxy and no redirects. It does not
contact a device or initiate HSTP synchronization. The manufacturer's schema defines a
top-level integer `server_time`; it supplies no hard timestamp-origin or accuracy guarantee.
[Official schema](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)

## Explicit qualification contract

Qualification subject version 3 adds a required `server_clock` object alongside the operative
settings required by version 2:

```json
{
  "model": "request_bracket_monotonic",
  "timestamp_error_ns": 1000000
}
```

The number above illustrates syntax only; it is not an accepted default or a measured bound.
The contract declares that the returned timestamp was generated during the measured request
on the same nondecreasing server clock used by playback, with no discontinuity during its
validity. The error bound must cover timestamp origin, quantization and backend differences.
The profile's existing age, uncertainty and drift limits still apply. Integer milliseconds
and small round-trip times do not prove these assumptions. Actual physical evidence and an
explicit operator review remain required; software tests create no real acceptance.

Version 1 and 2 subjects remain readable. They receive no implied server-clock contract and
cannot authorize this acquisition. Adding the contract changes the qualification identity;
old acceptance does not carry forward.

## Timing and lifetime

At receipt, the interval covers the server timestamp's declared error plus the entire measured
request duration and drift. The estimate is the nearest representable millisecond midpoint;
uncertainty includes rounding to that grid. No symmetric network-delay assumption or averaging
reduces the bound. Two still-valid samples must have overlapping propagated intervals.

Freshness starts at request transmission, not receipt or later status inspection. Aging adds
the qualified drift and fractional-millisecond uncertainty. Overflow, clock reversal, excess
uncertainty, stale samples, changed controller generations and revoked dependencies refuse
publication. Database and registry locks are released during network I/O, and the native
mapping is installed only after the observation receipt commits and the lease is checked again.
Concurrent superseded acquisitions cannot overwrite newer native state. Restart never
reconstructs a mapping from a saved report.

This maps host time to server time only. The [combined timing path](PULSAR_HANDY_PLAYBACK_CLOCK.md)
separately binds native device synchronization under a version-4 contract. Browser presentation
timing, physical play/stop dispatch and real hardware qualification remain unfinished.
Physical playback remains disabled. Validation results are recorded in the
[checkpoint](evidence/roadmap-2026-10-22/handy-server-clock/checkpoint.json).

The scoped campaign passes 330 Protocol, 136 Handy Engine, 26 Handy Client (one existing
desktop test ignored), 14 setup-journal and 51 playback-runtime tests, strict default/all-feature
Clippy, app/worker build, formatting and architecture/source checks. The actual CLI preserves
seeded historical timestamps through a dropped reply and restart with external networking
disabled. No real vendor clock request, device operation, physical evidence or acceptance was
created. The initial fixture revision, scratch socket-path and compilation failures are retained
alongside the corrected results.
