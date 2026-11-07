# Continuous timing for an existing Handy play

Pulsar's continuous timing path observes the reported phase of an already admitted,
acknowledged HSSP play. It retains the original play reference and admits a separate,
bounded native observation when the required physical qualification is currently accepted.
It does not restart play, replace the script, synchronize device clocks or change offsets.
Local software validation is recorded below; no actual Handy timing qualification is
claimed here.

## Original reference and current observation

The original play retains its exact request, program/configuration selection, initial
clock, acknowledgment and synchronization history. Its script start time, server start
time and original combined uncertainty form the immutable reference for later observations.
Fresh reads cannot retroactively reduce that uncertainty or reset the age of the original
synchronization.

One acquisition collects public server time, a Playing-state device inspection, and public
server time again. The inspection brackets device identity, mode, operative settings,
HSSP state and two device-time/user-offset reads. The device must remain Playing in the
same mode generation and stream, at rate 1 with looping disabled and the original
pause-on-starving setting. Identity and operative settings must match the admitted subject.
Reported clock offset, latest synchronization round-trip delay and user offset must remain
consistent with the original synchronization; uptime must not move backward. These checks
establish continuity conditions, not an uptime-plus-offset calculation of execution time.
The latest-sync round-trip value neither dates this read nor determines its error bound.

The new model uses intervals for reported script phase and server time. It covers the
complete measured request brackets, declared timestamp error, aging drift and representable
millisecond rounding. Before/after intervals must agree at a common host time. It does not
assume symmetric network latency or an atomic multi-field response.

The alignment calculation retains all three uncertainty components:

```text
expected phase = original script start + current estimated server time - original server start
alignment bound = absolute observed/expected phase difference
                + original play-reference uncertainty
                + current phase uncertainty
                + current server uncertainty
```

The implementation uses checked integer arithmetic and compares the total against the
accepted profile's `maximum_clock_uncertainty_ns`. A reported script phase is not physical
motor position, proof of complete remote script contents, or the presentation time of a
browser frame.

## Required physical qualification

Qualification subject version 6 adds `continuous_timing` to the existing operative-settings,
server-clock, device-clock and recovery declarations. Versions 1–5 remain historical subjects
and gain no implicit continuous timing contract. The added declaration participates in the
subject/configuration identity; existing acceptance does not transfer to changed bounds.

| Field | Meaning |
| --- | --- |
| `model` | `hssp_playtime_request_bracket`: a physically substantiated interpretation of reported HSSP playtime under bounded request brackets. |
| `timestamp_error_ns` | Nonzero bound covering report accuracy and its correspondence to execution under the accepted model. |
| `maximum_sample_span_ns` | Maximum permitted complete Playing inspection span and each server-sample round trip. |
| `maximum_observation_age_ns` | Maximum age from the original inspection start, never from a later status query or renewal. |
| `drift_ppm` | Accepted bound for phase progression error as the observation ages. |

These fields have no physical defaults. The protocol limits observation age to 30 seconds
and requires positive, internally consistent spans; those software limits are not measured
device guarantees. The actual Handy 2 Pro firmware, operative settings, report acquisition
behavior, cloud delay, phase-to-execution correspondence and error/drift limits require real
evidence and explicit operator acceptance for the exact subject.

The [integrated manufacturer research](../research/phase-b/handy-continuous-timing-and-streaming-2026-10-22.md)
records the source limits. The [REST specification](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)
exposes timing and state operations but supplies no qualified physical error budget. The
[pinned draft RPC state contract](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/constants.proto#L129)
treats state as approximate and assembled at different instants; its current point is not
physical position. The research also finds no guarantee that effectful clock synchronization
or script-time correction preserves uninterrupted playback with bounded disturbance. This
path therefore observes playtime and performs neither operation.

## Validity, handoff and retirement

Eligibility requires a current Playing controller/runtime and an acknowledged original typed
play. Prepared, claimed or Unknown initial play cannot be rescued by a timing read. The
selection includes the original acquisition, exact controller generation and exact original
play request/record. Native checks additionally retain the runtime, program, qualification,
credential route, hosted bytes and synchronization lineage.

Starting a refresh does not clear an installed observation. Reads and hosted-byte rechecks
run without holding project, native registry or runtime locks over network I/O. Completion
reauthorizes current grants, project revision and all captured dependencies, checks the latest
attempt and exact controller generation, commits the response, then checks eligibility again
before publishing native state. A stored response alone cannot publish that state.

The first handoff must finish while the original Playing timing remains valid. Subsequent
observations must agree with a still-valid predecessor. Expiry cannot be bridged by a late
response. Observation validity ends no later than the earlier of inspection-start plus the
accepted observation age and the fresh server-clock expiry; error growth may invalidate it
earlier. Controller lifetime remains an independent requirement.

Controller renewal extends the ownership lease, not observation validity. It preserves the
installed observation's timestamps, expiry and original reference. A new refresh uses the
renewed generation; a late result captured under the old generation refuses. An old or
superseded failure must not retire a newer valid observation. A historical report continues
to show its captured controller generation, even when that exact native observation remains
applicable after renewal.

While a valid continuous observation references the original synchronization context, that
context may be retained as lineage after its original timing age expires. The old acknowledgment,
error model and timestamps remain unchanged, and cannot supply fresh initial timing authority.
Explicit synchronization replacement/removal, controller retirement, session/configuration
changes, qualification withdrawal, hosting retirement, revision/grant changes and Engine restart
invalidate dependent native state.

Failure of a current refresh or loss of timing eligibility can retire its exact target and
leave a stop obligation with the independent execution owner. A stop acknowledgment still
does not confirm physical stopping. Failed or unrecorded reads install no native authority;
missing history is not an invented Pending measurement. Existing reconciliation and explicit
resumption requirements remain in force when stopping uncertainty persists.

## CLI

Choose an already Playing controller and its acknowledged original play. Use its current
generation and current project revision for a new observation:

```sh
pulsar handy-refresh-timing PROJECT --revision PROJECT_REVISION \
  --acquisition-request-id ACQUISITION_REQUEST --controller-id CONTROLLER_ID \
  --generation CURRENT_GENERATION \
  --play-request-id ORIGINAL_PLAY_REQUEST --play-record-id ORIGINAL_PLAY_RECORD \
  --request-id REFRESH_REQUEST --allow-online-inspection

pulsar handy-refresh-timing-status PROJECT --refresh-request-id REFRESH_REQUEST
```

The refresh needs its own request ID, explicit online-inspection consent, authenticated
project Read/Export authority and the host-local Handy owner capability. The client reads
that owner capability locally; no vendor key values enter the command. Status requires the
original actor/project selection, local owner capability and current Read authority; it
does not take a revision or perform online sampling.

Retrying a stored refresh request returns its original observation and timestamps, with
`current` reflecting whether that exact native observation is still installed and eligible.
Status has the same historical boundary. Neither path extends expiry or reconstructs native
state after restart. A successful receipt may report `current: false` if final publication
lost eligibility. `playback_enabled` remains false in the report; neither field establishes
physical readiness or new permission to play. A missing response means no retained observation
is available, not proof that no read occurred.

## Remaining work and validation

This interface does not add a periodic browser heartbeat or calibrate browser media time to
native time. Source-file/placement identity, pause/seek/end/stale-event handling, browser session
replacement and presentation uncertainty remain separate integration work. Existing browser
inference, recording and motion-authoring paths remain relevant to the full on-the-fly objective.

Generated timed-point delivery also needs a separate HSP stream and bounded append queue,
durable chunk/unknown-outcome handling, buffer-horizon and starvation rules, and physical
qualification. The manufacturer's [FW4 guide](https://intercom.help/ohdoki/en/articles/9457599-what-s-new-in-firmware-4)
describes adding points while playing, but the retrieved contracts do not establish Add
idempotency or safe blind retries. Continuous HSSP timing observation does not implement
that streaming path.

Software validation passes 15 focused Engine/math cases and 20 localhost online-read cases,
including five new Playing-read cases. The default Engine suite passes 1,192 tests with 70
existing environment-dependent ignores; Protocol passes 359, and Handy clients pass 59 with
one existing desktop ignore. Both strict lint configurations, app/worker builds and
architecture/source guards pass. The full suite preceded a test-only return-type alias cleanup;
all 15 timing cases and explicit included-file formatting pass again on the final source.
The [exact alias diff](evidence/roadmap-2026-10-22/handy-continuous-timing/test-helper-alias.diff)
and original failures are retained with the [check receipts](evidence/roadmap-2026-10-22/handy-continuous-timing/checks.json).

The [actual CLI campaign](evidence/roadmap-2026-10-22/handy-continuous-timing/native-campaign/summary.json)
uses network isolation and Pending synthetic dossiers. It verifies consent/selection/status
refusals, repeated refusal inertness, subject import replay, current restart and actual preceding
binary refusal of schema 6. Existing database and WAL bytes remain unchanged on refusal;
creation of an absent empty WAL is reported separately. All 25 owned native processes joined
without forced kills. No device effects, worker jobs, physical acceptance or successful native
online refresh were created. Positive Engine timing cases use injected clocks/observations;
HTTP-read cases separately exercise localhost responses.
Positive fixtures may label a simulated subject `Physical` to exercise acceptance gates;
that label does not supply real device evidence.
No real Handy, private media, measured physical execution timing or human acceptance is
established by these documents. The roadmap remains partial.

See [ADR B5-049](../research/phase-b/ADR-B5-049-continuous-playing-timing.md) for the decision.
