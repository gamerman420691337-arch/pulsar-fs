# ADR B5-039 — Explicit server-clock bounds and native custody

Require a versioned, accepted timestamp-origin/error contract before turning a server-time
response into a native host/server mapping. The vendor's integer timestamp and round-trip
measurement do not establish accuracy or symmetric delay. Cover the whole request bracket,
declared error, drift and millisecond rounding; preserve acquisition-start age across retries.

Bind each mapping to one Engine, controller generation and sampling attempt. Commit a bounded
historical observation before native publication, then recheck expiry. Exact replay never
resamples or reconstructs authority. Device clock synchronization and browser presentation
correspondence remain distinct obligations; this mapping alone cannot enable physical playback.
