# Preserve original capture time across native clock domains

Keep one immutable bracketed correspondence per capture epoch between its
`Instant`-relative ticks and the suspend-aware host clock. Preserve the entire
observation interval; do not substitute a midpoint or later polling timestamp.
Check continued overlap with the original offset and monotonic observations.
Detected discontinuity retires the epoch, using its last valid tick for cleanup.
Initial clock unavailability preserves preview but cannot later mint a native
stamp for that epoch.

Retain native timing only after accepted CPU completion and exact issued-pair
validation. Preserve both frames, original age basis, estimate and acceptance
time. A pairing discontinuity invalidates previous native acceptance. Continuity
requires the exact previous result frame and current pair to overlap, with matching
attempt/profile and result sequence. Pending replacement cannot rewrite issued work.

The retained value is metadata for the later native producer, not serialized or
restored HSP authority. Source provenance, current authorization, device clocks,
motion-envelope bounds and actual presentation remain separate checks. Provisional
CUDA output is excluded. See [the guide](../../docs/PULSAR_LIVE_CAPTURE_CLOCK.md).
