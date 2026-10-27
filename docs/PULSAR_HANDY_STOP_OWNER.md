# Engine-owned Handy stop and completion work

The native preparation now shares one runtime and captured transport with an independent
Engine-owned stop worker. Controller retirement retains its stop intent for that worker.
This connects the stop side of the lifecycle; public physical playback remains disabled.

The expiry monitor still uses its own thread and registry, with no project database or
network dependency. Retirement clears controller/timing authority once. Later expiry sweeps,
status checks or repeated retirement do not construct another stop after an attempted stop
has completed. Native entries retain physical uncertainty and pending execution custody.

The stop worker takes a retained intent from the registry, releases that lock, and captures
its current runtime correspondence without journal I/O. The detached intent is bound to the
original Engine boot. It writes the intent before attempting transport, then rechecks the
current stop and exact receipt. A late write cannot restore a retired runtime. SQLite writes
and HTTP never hold the registry or runtime lock; no join occurs under those locks. Pump
writes also refuse SQLite writer contention immediately and restore the journal's ordinary
busy timeout afterward, so one external writer cannot monopolize the shared journal.

The worker submits one captured stop through its owned dispatcher. Stop remains independent
of revoked play grants, expired controller leases and unavailable play clocks, while retaining
the original device/session/configuration and credential snapshot. No user-provided URL is
accepted. Transport rejection retains the original request and uncertainty; an attempted stop
is never automatically sent again. Acknowledged and Unknown outcomes both enter
`StopUnconfirmed`. Metadata-write failures retain their exact observation and retry only that
write. Repeated retirement cannot turn these retries into more network requests.

Journal unavailability can prevent the initial stop handoff. The original play history and
physical uncertainty remain; this is not an emergency-stop guarantee. A separately qualified
emergency path and reconciliation for unresolved effects remain open.

## Claim ordering and lifetime

The dispatcher now supplies a single-use `ClaimGate` to the authorization callback. The
callback must apply that gate while retaining the same guard that serializes dependency
mutations. Only this invocation's gate can produce its `ClaimedHandoff`. Returning a handoff
from another invocation refuses, and failure after consuming a claim is Unknown. HTTP starts
after the callback has returned and released its guards.

The stop worker uses captured stop custody. The future native play authorizer must still
recheck current authentication, grants, qualification evidence, source/controller/clock pins
and synchronization custody under the original project DB mutex, then invoke the gate before
releasing that mutex. The continuation supplies the ordering mechanism; it does not implement
those missing Engine play checks by itself.

The Engine owns the expiry monitor and stop worker. The worker captures the preparation
registry and journal directly, never an Engine `Arc` or `Weak`. On ordinary Engine destruction,
the monitor joins first; the stop worker retires remaining controllers, drains work within a
local grace budget and explicitly joins active dispatcher workers while retaining their
outcomes. A failed final metadata write leaves durable history unresolved. This is not a
guarantee after process kill, machine failure or suspend. The daemon currently lacks a graceful
signal/listener shutdown path, so direct Engine destruction does not qualify daemon shutdown.

## Verification and remaining work

The [checkpoint](evidence/roadmap-2026-10-22/handy-stop-owner/checkpoint.json) records synthetic
schema-4 setup/synchronization/controller/combined-clock fixtures using the real Engine and
private SQLite stores. An isolated local HTTP endpoint receives the stop. Cases cover idle
expiry with the project DB held, a blocked effect-journal writer without blocking control,
acknowledged and unknown replies, failed/retried completion writes without a second stop,
and Engine destruction waiting for the in-flight stop and releasing all owned runtime handles.
Dispatcher cases check claim ordering, failure after claim, invocation substitution, and
detached-intent retirement/boot fencing. No real Handy or hardware acceptance is used.

The final run passes 66 playback-runtime tests, 170 Handy Engine tests and 20 journal tests,
with no failures or ignored cases. Strict workspace/all-target Clippy in default and
all-feature configurations, App/Worker builds, formatting and architecture/source guards pass.
The journal contention test verifies prompt refusal and restoration of the prior timeout on
both error and success. Intermediate runs before invocation binding and nonblocking journal
writes are retained separately; the final packet identifies the tested source and binaries.

Native play submission and final authority checks, controller custody during Playing, public
operation/status/recovery commands, daemon shutdown, emergency stopping, uncertain-effect
reconciliation, browser presentation/heartbeat and actual device/model/media/platform/storage/
human/release qualification remain open. Full roadmap PARTIAL.
