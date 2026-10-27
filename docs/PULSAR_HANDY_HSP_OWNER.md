# Engine-owned HSP streaming lifecycle

The subsequent [HSP qualification and recovery increment](PULSAR_HANDY_HSP_RECOVERY.md)
adds owned asynchronous metadata, retrospective emergency persistence and exact
recovery fences. The verification counts below describe the preceding owner
checkpoint; the linked guide records the newer combined scope.

Pulsar now owns the native HSP session through Setup, post-Setup observation,
prefill, explicit Play, later appends, retirement and Stop. The production owner
code is attached to Engine lifetime and shutdown. Its current admission and
sample feed are private and exercised with injected native facts and localhost
devices; public Start, accepted live-inference delivery and physical qualification
remain unfinished.

## Ownership and stopping

One independent pump owns a bounded registry of 16 retained sessions and a four-item
admission mailbox. Each admission creates one fresh runtime. Samples enter the
existing bounded point queue directly, with no second accumulating sample queue.
Play is explicit and keeps its original timing reference and expiry.

The pump joins completed writers before observing their results, checks current
source/clock authority independently, and prioritizes Stop over further motion.
Metadata retries have no send handle. Unknown delivery stays unknown; neither
Stop acknowledgment nor a completed software shutdown clears physical custody.

An exact native proof can retire an ordinary operation that was never submitted,
or whose worker joined before claim, even when its journal write is unavailable.
It cannot retire a reserved, attempting, claimed or settled writer. This proof is
separate from database cancellation: original journal state remains until a later
metadata operation can establish and record cancellation. An unsent Setup ends
without issuing a gratuitous Stop; an unsent Append or Play preserves earlier
stream custody and enables the same owner's Stop.

After an attempted Setup is joined and observed, the owner performs bounded
read-only device, mode and stream observations. Installation requires the exact
runtime, originating transport and Setup, an unretired native owner, and the
original probe deadline. Rebinding keeps the same credentials, route and retirement
flags and invalidates older sealed emergency envelopes. A known replacement,
cancelled probe or expired result cannot authorize continuation.

## Software bounds

The pump polls every 20 ms. A transient runtime-lock conflict defers new issuance;
only a successful runtime watchdog tick clears its fixed 100 ms contention window.
Current authority still expires independently. Persistent contention retires
ordinary authority without waiting for the mutex; a poisoned runtime reports
blocked stopping. These bounds are additional to scheduling delays and are not
physical Stop or end-to-end latency qualification.

Each runtime mutation samples its evaluation time and current facts while holding
that runtime's mutex. A separate earlier watchdog observation may check expiry
during contention, but cannot become the timestamp of a later mutation. This
prevents a concurrent producer's newer accepted sample from making an older owner
observation look like clock regression. Original sample timestamps and pending
Play references remain unchanged; the Core regression check remains enforced.

Journal reads and durable claim commits run without holding the runtime mutex.
The exact native reservation remains held while the watchdog can observe current
authority or retire the session. After commit, the sender releases the journal
lock, acquires the runtime again, samples fresh time/facts and revalidates the
original effect before sending. This only narrows the original validity window.
A post-claim refusal stays Unknown and cannot be retried as an unsent command.

Native command and probe budgets are three seconds within the sender's ten-second
ceiling. Shutdown closes admission, retires ordinary ownership and drains for up
to twelve seconds before final owned joins. Final joining may exceed the drain
budget; no thread is detached to manufacture successful shutdown. Unrecorded
emergency outcomes, blocked stopping and unknown work remain reported as unresolved.

The Engine object is not retained by the worker. Test-only admission and runtime
inspection are not application control capabilities. Narrow unused-code allowances
cover only internal admission/feed seams awaiting genuine qualification and a producer.

## Read existing status

```sh
pulsar handy-hsp-status PROJECT --stream-request-id ORIGINAL_STREAM_REQUEST
```

This requires current session authentication, the existing local Handy owner
capability and Read access to the project. It selects the authenticated actor's
original stream request. A read does not create an owner, refresh a lease, run a
watchdog, replay a command or contact the device. Without a retained native session
the Engine returns NotFound; this does not prove absence of historical attempts.

The snapshot includes the original source identity, queue counters, current effect,
latest joined observation, metadata and stopping flags, and post-Setup binding.
`native_active` means an owned command or probe worker is active; it does not mean
physical motion or overall session liveness. The latest observation is not complete
history: earlier unknown effects remain in their original journal records.
`qualified` remains false for the current private admission path. Emergency history
durability is now reported only after its exact retrospective commit. Source retirement, native
unsent retirement, durable cancellation and acknowledged Stop remain distinct.

## Remaining work and evidence

Public HSP admission still needs its own accepted qualification and reconciliation
contract; the existing hosted-script contract cannot authorize it unchanged.
Actual accepted inference needs a native clock-domain bridge because capture-relative
Instant timestamps cannot simply become suspend-aware device timestamps. Browser
controls/presentation, sustained queue/journal latency,
useful learned motion, actual Handy 2 Pro firmware/device measurements and all M1–M8
model/media/platform/human/release obligations remain open.

The [checkpoint](evidence/roadmap-2026-10-23/hsp-owner/checkpoint.json) records exact
software subjects, checks and limitations. Localhost tests use authored source,
device, qualification and clock data and do not establish physical behavior.

Source inspection during this increment also found that the earlier HSP dispatcher
test fixture hardcoded an SSD temporary path, bypassing the prior RAM harness's
`TMPDIR`. It now honors `TMPDIR`. This clarifies the earlier failed namespace run's
scope; it does not establish the cause of its intermittent write-observation failure.

## Verified software scope

The final default suites pass 1,398 Engine tests with 72 ignores, 409 Protocol tests,
and 563 Client tests with 14 ignores. The 41 added Rust cases comprise 17 native
lifecycle/claim tests, 14 actual Engine-owner cases, five protocol cases and five
client/CLI cases. Both strict workspace/all-target lint configurations, formatting
(including included test files), architecture/source guards and application/worker
builds pass. No fresh Core or browser suite is claimed.

The actual application and CLI pass capability discovery, project creation, repeated
missing-stream status refusal, unchanged empty HSP rows/census and no HSP thread
creation. All five directly supervised processes join without forced termination,
the Engine socket retires, and private fixture state is removed. This checks the
public read-only refusal route; it does not establish successful public streaming.

An initial check reported a deprecated atomic helper, replaced by checked counter
updates under the runtime mutex. The first focused test run retained an obsolete
cross-runtime equality assertion; it now proves equal wire values have distinct
native identities. The first strict run required boxing the large probe task.
Those failures and intermediate sources remain recorded. The final broader run
passes on normal host storage; no timing threshold or physical acceptance gate
was changed to obtain it.

The paused-claim regression reproduces a second independent hazard: holding the
runtime mutex across a durable claim makes the watchdog retire its own sender.
The repaired sender keeps exact reservation custody but releases that mutex around
journal I/O, then rechecks fresh facts and runtime state after commit. The focused
HSP suite passes 124 cases with one ignore, including Stop/expiry/fact-change refusal.
This demonstrates the structural repair; it does not retrospectively prove the
cause of every earlier Setup failure. Failure-only diagnostics retain bounded
claim duration/error, peer paths and watchdog contention/progress counters.


The clock-order regression reproduces false stream retirement before the fix and
passes with the corrected binary, including exact retained original sample fields.
An initial attempted post-fix check reused the old binary because source copying
preserved an earlier modification time; it is invalid as post-fix evidence. After
forcing recompilation, one run failed during Setup before the regression hook:
Setup was Unknown and the localhost peer observed no PUT. The exact unchanged
binary then passed the regression in 0.38 seconds. Host I/O pressure was observed,
but the Setup failure's cause remains unknown. Final full checks pass; no timing
budget was relaxed and no repair for that separate failure is claimed.

Protocol and Client totals come from the preceding full run in this increment;
their source is unchanged by the clock fix. The final Engine suite, both strict
configurations, structural/format checks, application/worker builds and actual
native read-only CLI campaign were rerun against the corrected owner.
