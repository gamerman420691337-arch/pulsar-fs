# Retain original unclaimed command custody across bounded contention

Scoped software verification passes; no physical qualification is implied.
Native finite-tail campaigns exposed Setup and Append rejection when a dependency
lock was briefly occupied. Preserve the exact unconsumed command gate on a local
preclaim `WouldBlock`, release every dependency guard, and revalidate before the
next acquisition attempt. Generic errors and previously claimed commands do not
provide that custody.

The gate retains the original dispatch window, effect bytes, Play reference and
expiry. One first-contention budget is capped at 100 ms and additionally bounded
by all original validity; it does not restart at each lock. Waiting occurs only
after DB, session, capture, timing, journal and runtime guards are released.
Cancellation, shutdown, owner retirement and original expiry are checked before
and after acquisition and at the existing final claim. Reacquisition revalidates
current actor, qualification, source and clock facts without renewing any original
permission. Network preflight is not repeated by this loop.

Only a local mutex `WouldBlock` before claim begins may return the gate. Poison,
identity or qualification mismatch, journal failure and expired authority remain
terminal. Once native claim has begun, later failure retains the existing unknown
outcome and never returns a gate for another send. The legacy gate interface keeps
its immediate refusal behavior for callers that do not own the acquisition loop.

The final source check uses the same held native timing claim as command admission.
Its observation comes from the current native registry entry, with exact binding,
lifetime identity and original expiry checked; a copied report cannot substitute
for that claim. This avoids independently copying a newer or contended observation
between source validation and dispatch.

Deterministic tests must hold actual dependency locks, prove no claim or PUT while
waiting, prove guard availability during waits, and show exactly one original
claim/send after valid release. Cancellation, expiry, revocation and poison must
refuse; postclaim outcomes must remain non-retryable. See the
[verified checkpoint](../../docs/evidence/roadmap-2026-10-23/hsp-preclaim/checkpoint.json).

Verification: full Engine1,534 tests (81 ignored) pass on controlled RAM; five
explicit native CPU/localhost scenarios pass in each of three unchanged-binary
runs. They hold real DB/capture/timing locks, retain one original Append after
release, and refuse it after native cancellation or the original cap. The 2 ms
visible observer cadence and original policies remain intact. Strict lint in both
configurations, format, workspace checks, architecture/source guards and app/worker
builds pass. Test-clock reentrancy, initial clock state and observer counter ordering
repairs are recorded with the failed runs. Ordinary-host timing reliability,
sustained source/media/clock transfer, rendered browser-to-HSP integration and all
actual device/model/release qualification remain open.
