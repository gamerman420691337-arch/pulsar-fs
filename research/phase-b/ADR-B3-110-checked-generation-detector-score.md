# ADR B3-110: checked generation detector score

Status: implemented software; qualification remains separate.

The legacy CLI advertised a 0.35 confidence default while the detector used 0.25 and the client refused other requested values. Replace that misleading interface with an optional checked generation score. Omitted settings retain the actual legacy 0.25 behavior and omit the new wire property, preserving old serialized configuration identities. Explicit scores must be finite in the inclusive range 0 through 1 and require a declared non-temporal detector. Scores are uncalibrated model outputs, not probabilities or a quality measure.

Carry the setting in immutable GenerationSettings through ordinary generation and Stash batch requests to the confined detector. Filter decoded observations before tracking using the requested threshold, including the exact boundary. Record the effective threshold in the generation lineage alongside the configuration and its digest. An explicit value changes configuration identity even when it equals the default.

The CLI uses `generate --conf`; Stash uses `stash generate-missing --detector-score-threshold`. The desktop provides an optional control for declared detectors and applies it to both generation paths. Preview retains its existing 0.25 threshold, stated in the control. Temporal regression and no-model flow paths refuse an explicit detector score instead of ignoring it. A missing target remains unavailable and does not become fabricated motion.

See [the setting and evidence](../../docs/PULSAR_DETECTOR_SCORE_SETTINGS.md). No threshold establishes calibration, target/script quality, useful presets, performance, native GUI acceptance or release/device qualification.
