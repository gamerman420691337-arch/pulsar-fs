# ADR B5-001: timing-preserving device-motion proposals

Date: 2026-10-01. Status: software implemented; physical execution unqualified.

## Contract and decision

[MOT-02 through MOT-05](../../docs/PULSAR_SOFTWARE_SPECIFICATION.md) require an unchanged neutral master, preserved action times, relative travel mapping, applicable dynamics constraints, and explicit limitations. The owner selected Handy as the device target; this does not supply a particular firmware, calibrated profile, stopping bound or permission to actuate it.

`pulsar-core::adapt_device_motion` maps normalized positions into each declared physical range. One gain, at most one, compresses all tracks about their range midpoints. It never expands each shallow movement independently. The selected minimum gain is explicit profile input; an infeasible proposal fails instead of silently flattening the program or retiming actions. Missing mappings, unavailable axes used by coupled constraints, unresolved gaps, invalid values and arithmetic overflow fail explicitly.

Each segment uses rest-to-rest quintic interpolation, `s(u) = 10u³ - 15u⁴ + 6u⁵`, at its original timestamps. The proposal requires stopped entry at the first position. A driver must execute this curve: transmitting only the endpoints to a player that interpolates linearly would invalidate the dynamics claim. Stopping at each anchor changes the intermediate trajectory, and is an explicit proposal limitation rather than inferred source motion. Entry/exit transitions, quantization, transport tracking error and physical stopping remain separate work.

## Analytic bounds and independent checks

For segment displacement `d` and duration `T`, velocity is bounded by `1.875 |d| / T`, acceleration by the conservative bound `6 |d| / T²` (the exact maximum is `10√3/3`), and jerk by `60 |d| / T³`. First and second derivatives vanish at segment boundaries; third derivatives can change there but remain bounded on either side. Coordinates do not overshoot either endpoint. These derivative facts apply to the declared curve, not unknown device behavior.

Per-axis maxima and weighted L1 coupled envelopes constrain displacement about each midpoint, velocity, acceleration and jerk. Summing axis maxima is conservative even for asynchronous axes. A global gain scales every bound linearly. The implementation leaves numerical headroom when compression is needed, rejects unrepresentable computations, and cannot assume a missing coupled axis rests at zero or midpoint.

The tests use the specification's 20–40 into 10–90 example, independent dense polynomial derivative evaluation, asynchronous coupled-axis samples, immutable timestamps/evidence/master checks, unavailable inputs and extreme values. Those checks are software evidence; no supplied profile has been physically qualified.

## Authority and export

CLI `export-device-motion PROJECT OUTPUT --profile PROFILE` and the desktop export panel invoke `ExportDeviceMotion` through the engine. The engine requires export authority and an expected revision, reserves memory, computes profile/master identities, publishes a new destination without overwriting, and retains an idempotent outcome. An unresolved prior dispatch remains unknown rather than being blindly repeated. Proposal serialization is bounded before output allocation exceeds the export limit.

The JSON format is `pulsar-device-motion-v1`, with `qualification: "unqualified_proposal"` and `admitted: false`. It includes the selected profile, exact master identity, gain, physical action times/positions/evidence, interpolation/entry requirement, and conservative dynamics bounds. Exporting this artifact does not arm, transmit to, qualify, or approve headless use of a device. Standard funscript exports continue to contain the neutral master.

## Remaining B5 obligations

Connect a qualified profile to immutable playback admission; implement a driver that actually executes the admitted curve, with configuration-bound transport/error/quantization evidence; preserve controller/revision/disconnect/stop contracts; obtain separate physical stopping and synchronization evidence. Handy BLE/online implementation, consent and compatibility remain roadmap work. This ADR closes neither GB-07 end-to-end admission nor B5.
