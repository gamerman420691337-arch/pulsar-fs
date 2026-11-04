# Owned Handy HSSP transport

`playback_runtime::dispatch` connects prepared coordinator effects to an owned
native HTTP worker. It implements conditional HSSP play/stop transport; public
Engine playback is still disabled. Native registration, qualification intake,
hosting/setup, controller leases and combined timing exist separately; their owned
play/stop integration and streamed/BLE modes remain open.

The owning Engine constructs a transport for an exact loaded-program admission,
host device session and remote stream ID. Credentials are read by distinct
environment-variable names into native memory. The production origin is fixed to
the manufacturer HTTPS v3 endpoint. No caller URL, proxy, redirect, upload, mode
change or protocol-family fallback is accepted. Custom authentication headers are
marked sensitive; errors and completion reports contain no credential or remote
error text. A decoded response echoing either credential, including a numeric value,
is withheld. Constructing a transport does not contact the device or grant authority.

One dispatcher owns one worker with no queue. Starting another job, even after the
worker finishes, requires reaping its completion first. A refused submission returns
the original request, including a prepared stop token. Before transport handoff, a
bounded, cancellation-aware Engine callback must freshly verify engine boot ownership,
grants, qualification and target identity. It runs outside the coordinator lock. The
authorizer invokes the supplied single-use `ClaimGate` while retaining its dependency-mutation
guard. The gate takes control ownership without waiting and checks the current effect,
exact receipt and transport binding. Play also checks current admission, controller
and clock. Stop remains available for its
original device configuration when the desired project revision changes.

The returned `ClaimedHandoff` is bound to this dispatcher invocation. A substituted handoff
cannot bypass its claim, and a callback error after a consumed claim remains Unknown.
The worker releases callback/control guards before HTTP. The Engine now has a
[retained stop/completion owner](PULSAR_HANDY_STOP_OWNER.md); native play authorization and
submission remain separate unfinished integration.

Play effects retain their original clock facts. The same age/drift/uncertainty kernel
used at issuance is checked again after journal preparation. The request's already
recorded bytes are never rewritten. The dispatcher samples the same suspend-aware
host clock as controller leases; it never advances that clock with `Instant` elapsed
time. The earlier original source, command-age and controller-lease deadline caps
the HTTP wait. Clock failure, reversal or expiry before claim refuses without a send.
After claim, the worker rechecks source age/error and the absolute window before
request construction, immediately before sending, and after receiving the body.
Loss of timing validity after claim produces Unknown and never reopens the claim.
Stop uses its captured route and configuration without requiring a live play clock
or lease. Linux uses `CLOCK_BOOTTIME`; other platforms retain the existing refusal
until a suspend-aware clock is supplied and qualified.
The caller supplies an absolute local I/O deadline, limited to ten seconds; this
operational ceiling is not a qualified device timing default. Authority lookup cannot
extend that deadline. The authorizer must cooperate with its supplied deadline and
cancellation flag; this interface cannot preempt arbitrary blocking caller code.

The coordinator consumes a separate one-use claim for each play/stop effect. Claims
are never reopened after uncertain I/O, cancellation or a lost result. Two dispatcher
instances cannot resend the same effect. The registry must still own one dispatcher
per device/session: the claim check does not serialize different effects submitted
through different instances or establish external controller exclusivity.

The worker sends the original PUT/body exactly once, reads at most 64 KiB plus an
overflow byte, and requires a valid response for the expected stream, play/stop state,
normal rate and disabled looping. Other HTTP statuses, malformed/error responses,
wrong stream/state, oversize data, deadline expiry and post-claim cancellation yield
an unknown outcome. No automatic retry occurs. Cancellation cannot retract bytes
already sent or prove remote cancellation/stopping.

`poll` reaps a finished worker. `shutdown` requests cancellation, joins and returns
its completion. The owner can apply the completion through the journal-independent
[observation interface](PULSAR_PLAY_COMPLETION_RECONCILIATION.md), release control
ownership, and persist its metadata separately. Unknown play outcomes retain a stop
intent. A callback retired by revocation/reconnect cannot restore authority. Its
`historical_observation` may still retain the exact attempted outcome for metadata-only
persistence, without changing live state or consuming a newer stop. A pre-claim
Rejected result cannot create such an observation. This history path is not a
substitute for observing a current callback and handling its stop result. Dropping
the dispatcher joins its worker but discards an unread result; its original prepared
journal row stays unresolved and physical uncertainty remains. Normal ownership
teardown must use `shutdown` and handle that result and any stop obligation.

## Contract basis and verification

The 2026-10-22 [dispatch clock checkpoint](evidence/roadmap-2026-10-22/handy-dispatch-clock/checkpoint.json)
tracks the shared-clock and late-history change. Regression scenarios include a
synthetic suspend between authorization and claim, clock loss/reversal before a
send, original source/lease limits on the actual HTTP wait, expired responses,
uncertainty growth after claim, stop availability without a play clock, numeric
credential echo withholding, and actual delayed play followed by revocation and
failed/retried history persistence while the newer stop stays current.

The scoped run passes 61 playback-runtime tests (20 dispatch cases), 166 Handy
Engine tests and 19 journal tests, with no failures or ignored cases. Strict
workspace/all-target Clippy in default and all-feature configurations, App/Worker
builds, formatting and architecture/source guards pass. The packet retains the
commands, logs and source identities. No new whole-workspace test or CLI playback
claim is made; no public playback command is enabled by this change.

These clock jumps are injected; the tests do not suspend this workstation or qualify
hardware. The final check precedes the HTTP library call, not each kernel write.
A process can still be preempted after its last check, and a transport timeout or
cancellation cannot retract an already-sent request. The Engine's owned
dispatch/completion/stop pump, fresh final authorization, public play/recovery commands,
browser correspondence and actual device acceptance remain open.

The following records the earlier 2026-10-16 transport checkpoint.

The [manufacturer v3 OpenAPI](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)
was fetched again on 2026-10-16. SHA-256
`9ad90e385ff1bf5f454a9af5f02862ca6cb064654b30df873d7d7389198cb684`
matches the prior codec pin. It specifies PUT `/hssp/play`, bodyless PUT
`/hssp/stop`, and remote state responses. A matching response is a transport
observation, not physical stopping evidence.

Eleven local HTTP/lifecycle cases pass, including exact requests and metadata
observation, duplicate play/stop claims, busy/unread ownership, denied/stale authority,
clock/binding/reconnect refusal, malformed/error/oversize/wrong-state responses,
timeout, redirect refusal, cancellation before/after claim, revocation during actual
I/O, authorizer panic, owner drop and absolute-deadline preservation. All mock server
threads are joined. All 51 coordinator tests and the public Engine playback-refusal
case pass. Strict Engine and workspace/all-feature Clippy, App/Worker builds, format
and architecture/source guards pass. Initial lint findings are retained: submission
errors now box their owned request envelope, and the test server has a named result
type. No assertions or production timing bounds were relaxed.

These are native localhost HTTP tests with synthetic credentials and qualification
facts. No real device was contacted. Manufacturer TLS/DNS/network conditions, actual
Handy 2 Pro firmware/response/stop/order behavior, capture-to-command latency and
physical synchronization remain unqualified. Full Engine/Client/storage suites were
not rerun here; earlier ordinary-storage failures remain open. Full roadmap PARTIAL.
See [checkpoint](evidence/roadmap-2026-10-16/handy-dispatch/checkpoint.json) and
[decision](../research/phase-b/ADR-B5-021-owned-hssp-dispatch.md).
