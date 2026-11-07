# HSP Stop during unavailable journal writes

The native HSP dispatcher can attempt an already-issued exact Stop when its
journal prewrite or claim cannot be confirmed. The fallback uses retained native
ownership and the same device, session, mode and stream preflight as durable
dispatch. Only Stop can use it. An observed replacement journal owner retires
the transport; a busy or unavailable journal does not establish a replacement.

This is a native component prerequisite for continuous stream ownership. It does
not enable a public HSP stream, perform physical qualification, or guarantee that
a disconnected Handy receives Stop. All transport tests use localhost fixtures.

## One native attempt

Normal and emergency dispatch share the same dispatcher worker slot and a
reservation retained by the exact runtime. The reservation is acquired before
starting a worker. Cancellation does not release it, and neither a cloned effect
nor a fabricated rejected completion can retire another worker's slot. A claim
remains consumed after failure or an uncertain reply. An unwinding claim remains
uncertain until the worker joins.

Joining settles the reservation without taking the runtime or journal mutex.
A confirmed unclaimed rejection releases its reservation only after join; the
original runtime effect still needs its existing exact retirement rules. A Stop
that was attempted cannot be sent again through either dispatch path.

## Incomplete emergency history

Emergency completion is a distinct retrospective observation. It has no durable
claim or persisted emergency record. The first observed result is frozen, including
Unknown; repeated access cannot become a late acknowledgment or another send.
It retains the exact registration, device, mode and keyed credential-route snapshot
alongside the original effect. These fields contain no credential values and cannot
reconstruct native authority. The request and first observation bind the exact runtime
instance; another runtime with identical field values is insufficient.
Acknowledged Stop remains transport evidence, not physical stopping evidence.
Previously prepared journal rows remain unchanged by the emergency attempt.

The caller must retain the joined completion and observation. Abrupt process loss
can lose this unrecorded result. Journal history cannot prove that no emergency Stop
was attempted. A separate retrospective persistence path, status reporting and
qualified reconciliation remain necessary before public streaming is enabled.

## Remaining integration

This fallback requires the runtime to have already issued its Stop. It cannot
clear a still-occupied ordinary-operation slot during a metadata outage. The
continuous Engine owner must settle that work without treating rejection of a
clone as proof that the original was never claimed. Post-Setup mode acquisition,
native owner renewal/retirement, watchdog and shutdown integration also remain.

Authenticated HSP admission, useful accepted inference, browser presentation and
controls, sustained latency, actual Handy 2 Pro firmware/device measurements, and
all M1–M8 model/media/platform/human/release gates remain open. The full roadmap
is PARTIAL. See [the checkpoint](evidence/roadmap-2026-10-23/hsp-emergency/checkpoint.json)
for exact test subjects and outcomes and [the preceding durable dispatcher](PULSAR_HANDY_HSP_DISPATCH.md)
for its separate scope.

## Verification

The final default Engine suite on the normal host passes 1,367 tests with 72 explicit ignores. Thirteen
new localhost cases cover missing/poisoned/locked metadata, ordinary-effect refusal,
shared claims, exact runtime identity, known replacements, lost/malformed replies,
panics before/around/after claim, frozen Unknown and shutdown while runtime/journal
locks are held. Original journal rows and census remain unchanged by fallback.

Both strict workspace/all-target lint configurations, formatting including the
included test file, architecture/source guards and application/worker builds pass.
The initial 89-test run and expanded 93-test run passed; two completion-handler style
errors and five test style errors were subsequently corrected and retained in the
packet. Filesystem journal waits were observed during targeted runs, which resumed
without restart or changed timing limits. An earlier full ordinary-storage run timed
out at its 600-second command budget. A private RAM/mount/PID/network run then passed
1,356 cases and failed 11: its unmapped root-owned ancestor and retained capabilities
changed ownership/permission conditions, and one new HSP case failed to observe a
local write. The unchanged exact HSP test passed alone in 0.10 seconds; its original
cause remains unknown. Failure-only diagnostics were added without changing production
behavior or timing limits. The final normal-host full run passes. These observations
do not establish a production repair, physical behavior or storage latency/durability.
The failed namespace run joined its test process and supervisor; one leftover namespace
PID was signaled and reaped before unmount, and scratch was removed. The prepared
namespace-harness revision was not run and supplies no verification evidence.
No fresh full Core/Protocol/Client run or real device/browser HSP campaign is claimed.

## Subsequent owner integration

[Engine-owned HSP sessions](PULSAR_HANDY_HSP_OWNER.md) now add native-only retirement
of an exact unsent ordinary slot, post-Setup acquisition and watchdog/shutdown ownership.
This leaves journal cancellation and emergency persistence as distinct obligations.
Source inspection found that this earlier increment's dispatcher fixture hardcoded
an SSD temporary directory and therefore bypassed the RAM harness's `TMPDIR` for
HSP cases. The subsequent fixture honors `TMPDIR`; the earlier namespace result
must not be read as an all-RAM run or a proven explanation of the intermittent
write-observation failure.
