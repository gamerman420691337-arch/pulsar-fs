# ADR B5-049 — Continuous timing for an existing Playing stream

Retain the original acknowledged HSSP play reference and its uncertainty, and admit
fresh reported script-phase observations separately. This permits a qualified timing
handoff for an existing play without rewriting its original synchronization, clock or
command history. It requires a new physical qualification contract because the
manufacturer's approximate status fields do not provide an execution-error bound.

The selected model brackets reported HSSP playtime with native observation times and
fresh server-clock samples. It does not infer ongoing execution time from device uptime
plus clock offset, advance the original synchronization timestamp, or issue effectful
clock/script-time correction. The [manufacturer research](handy-continuous-timing-and-streaming-2026-10-22.md)
records the distinction: the [draft RPC state declaration](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/constants.proto#L129)
describes advancing playtime and approximate, non-atomic state; the
[REST client](https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L1004)
exposes readback and separate synchronization effects. Neither establishes physical
accuracy, atomic acquisition or harmless clock correction during playback.

Qualification subject version 6 must explicitly cover timestamp/readback error, maximum
acquisition span, observation age, drift and reported-phase correspondence to execution
for the exact firmware, operative settings and control/profile subject. Original play
uncertainty remains in every subsequent alignment bound. Software validation and an
accepted declaration cannot manufacture the physical measurements supporting that model.

Only an already admitted Playing runtime with an acknowledged original typed play can
refresh. Its exact controller, generation, original play, program, qualification, route,
runtime and synchronization lineage remain bound. Starting a read preserves any installed
observation. Publication follows durable response persistence and a fresh eligibility
check; the predecessor must still be valid, so a late result cannot revive expired play.
Ordinary controller renewal preserves installed observation timestamps and expiry while
changing the controller generation. Old-generation or superseded read failures cannot
retire a newer valid state.

The original synchronization context may remain as lineage while a valid independent
observation references it. This does not make its expired original timing usable again.
Explicit lineage removal or replacement invalidates the dependent observation. Current
failure or expiry retires playback through the existing stop owner, preserving physical
uncertainty. Status and stored-request replay cannot resample, reconstruct native authority
or improve a historical bound.

The feature contract records software, localhost and isolated CLI validation. No Handy 2 Pro
hardware qualification or physical timing result is asserted. Browser presentation calibration and generated HSP
point streaming remain separate work; observing an existing HSSP play does not complete
the on-the-fly generation objective. See the [feature contract](../../docs/PULSAR_HANDY_CONTINUOUS_TIMING.md).
