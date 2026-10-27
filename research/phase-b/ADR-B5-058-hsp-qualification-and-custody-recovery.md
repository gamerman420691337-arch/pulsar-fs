# Keep HSP qualification and recovery distinct from restored authority

Extend the existing evidence and recovery stores with an explicitly selected HSP
transport and subject version 7. Keep hosted versions 1–6 byte-identical. Bind the
common physical envelope, settings, clock and recovery contracts to the HSP policy
and supported fixed playback options; never pass HSP acceptance into hosted timing
or program authority.

Journal version 11 retains immutable original qualification links and separate
retrospective emergency observations. A missing old link stays missing. Aggregate
exact stream/effect/emergency membership separately from legacy hosted membership,
preserving old hashes and original Unknown outcomes. Resolution blocks old claims
and releases only its captured route/device custody.

Keep every journal operation off the independent HSP pump. Metadata tasks have one
owned worker per retained session, exact retries and a fixed contention episode.
Native Stop does not wait for that worker. Final shutdown owns every join rather
than detaching blocked storage work.

Capture installs an exact native admission fence. Drain produces a sealed proof
only after native and metadata work joins. The Engine obtains fresh qualified
samples, rechecks scope and immutable history, commits resolution, then releases
only the matching native proof using a verified receipt. Failure, cancellation,
replay and historical reconstruction never reopen the old stream.

These are prerequisites for fresh admission. Native preparation, inference clock
bridging, public controls and actual device qualification remain unfinished. See
[the implementation guide](../../docs/PULSAR_HANDY_HSP_RECOVERY.md).
