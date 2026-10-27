# Handy HSP sustained timing and finite-tail drain

Research date: 2026-10-23. Current manufacturer sources were fetched again from the official public GitLab repositories; their observed heads match the earlier [HSP contract note](handy-hsp-stream-contract-2026-10-23.md). The parent task independently re-fetched the official REST specification during this research. Only documentation and public repository endpoints were accessed. No credentials, device/control endpoints, SDK execution, build, or physical device were used.

## Decision

Keep the original Play script/server reference immutable. Treat a later HSP state as an approximate signed script-clock observation, with separately retained request timing and identity, not a replacement Play origin or proof of physical position. Raw HSP has no documented end-input command that seals the final appended point. A finite-tail implementation therefore needs its own exact input seal and must still issue the separately owned Stop. These are application design conclusions from the contracts below, not newly established firmware guarantees.

## What the HSP state clock means

The owning RPC declaration types `current_time` as `int32`: a playback clock sampled when the response is generated and advancing during play. `current_point` is separate buffer bookkeeping and can move backwards after synchronization/resume. Buffer extent/count describes stored points, not consumed motion. State construction is not atomic across fields; paused readings may drift. This approximate snapshot is distinct from actuator position. Preserve negative script time; do not substitute a point index, last-point timestamp, uptime or receipt time. [RPC state semantics and fields][rpc-state]

The RPC state contains no clock-sample timestamp, accuracy bound, request echo, or calibrated physical-position measurement. A complete response can therefore support identity/options checks and approximate clock consistency, but cannot by itself establish the exact sample instant, a maximum phase error, or whether the final physical stroke finished. [RPC state fields][rpc-state]

The SDK's state typedef uses a shorter point-time description, while its numeric play-state declarations include Playing, Stopped, Paused and Starving. Prefer the more explicit owning RPC sampling explanation when interpreting that shorthand; still qualify the actual REST/firmware combination. [SDK state type][sdk-state]

## Original Play and the separate clock relationships

RPC Play uses signed `start_time`, optional unsigned `server_time`, positive playback rate, looping and starvation policy. The server value represents the estimate at transmission for latency adjustment; omitting it removes that adjustment in the RPC description. RPC clock-offset messages separately describe server UNIX milliseconds versus device milliseconds since boot. Their stored round-trip value is supplied by the client. `RequestHspCurrentTimeSet` changes playback time and has a correction filter; it is not an observation. The request list provides separate Setup, Add, Flush, Play, Stop, Pause, Resume and state/time controls, without an EOF-seal operation. [RPC clock fields][rpc-clock], [RPC HSP requests][rpc-hsp]

The SDK rounds Play's script/server fields and defaults the server estimate to zero. Its client helper removes high-RTT samples, averages offsets, and estimates server time as local wall time plus that average. It exposes no measured error interval, expiry or suspend proof. Client/server estimation and device/server synchronization remain distinct. [SDK Play serialization][sdk-play], [SDK client sync calculation][sdk-sync], [SDK server estimate][sdk-estimate]

The SDK also distinguishes `ClockSynchronized.offset` from the separate configurable `OffsetState.offset` returned by `/hstp/offset`. The inspected SDK/RPC comments do not define exactly where that user offset enters HSP Play/current-time algebra. Its applicability, sign and error contribution need qualification; an unchanged value does not establish a freshly calibrated phase. [SDK HSTP interfaces][sdk-hstp]

The manufacturer demo pairs a Play at script zero with the client helper's server estimate. It recommends occasional client resynchronization when drift matters, but supplies no numerical renewal interval or accuracy guarantee. The same demo stops explicitly after a chosen delay rather than proving finite-tail completion. [SDK finite-script example][sdk-demo]

For a unit-rate, nonlooping session without pause or timing correction, the application may model its original command as:

```text
predicted_script_ms(T) = original_start_ms
                       + estimated_server_ms(T) - original_server_ms
```

This is an application assumption, not a published exact equation or error bound. Preserve the original pair. Keep browser phase, host monotonic time, device uptime and server estimate distinct. A later observation needs qualified uncertainty; it cannot replace the origin, conceal a mismatch, or renew command/source authority.

## REST cross-check and stream-end scope

The freshly retrieved REST specification describes device `time` as milliseconds since boot, `clock_offset` as the server/device difference and `rtd` as belonging to the latest synchronization; that object supplies no synchronization timestamp or accuracy bound. Its omitted Play server estimate uses current server time with reduced external synchronization accuracy. `/hsp/synctime` changes stream time; `/hsp/stop` is a separate bodyless operation. The HSP path list has no close/end operation. [REST specification][rest]

REST does expose `stream_end_reached`, but documents it for closed streams, including HSSP scripts and closed STREAM-protocol streams. Its generic HSP-state payload is not a terminal-index receipt for raw `/hsp/add`. Starving notifications apply when automatic pause is disabled; paused/resumed-on-starvation events cover the enabled policy. Neither event description establishes a physical completion or Stop bound. This paragraph and the preceding one are the complete REST-derived summary. [REST SSE contract][rest]

## EOF, starvation and stopping

The SDK groups `stream_end_reached` with HSP events and exposes a listener. That grouping does not broaden the REST event's finite closed-stream contract into an EOF handshake for arbitrary Add calls. [SDK event declarations][sdk-events]

The public RPC notifications provide HSP state-change, threshold, starvation and automatic-pause/resume observations. Their payload is HSP state; these declarations provide no exact byte membership, terminal-seal identity, delivery guarantee, or physical stopping certificate. [RPC notifications][rpc-notifications]

The SDK stream helper is an open producer: append adds points, threshold/starvation callbacks try pushing more, and an empty next slice returns the cached state without declaring completion. Its exposed API has no finish/seal method. `closeConsumer` removes local subscriptions and stops SSE; it does not send HSP Stop. Thus helper closure, an empty queue and temporary starvation are not interchangeable with deliberate source EOF. [SDK stream helper][sdk-stream]

The large-script example describes starvation as playback time exceeding the final buffered timestamp and uses the automatic-pause option to stop the playback clock until more data arrives. This behavior can diverge from a visible player that continues advancing; copying the example does not establish synchronized drain. [SDK streaming example][sdk-stream-demo]

## Application contract to implement and qualify

The following is proposed Pulsar policy, not a vendor feature:

1. Distinguish input closing from cancellation. Freeze new input against the exact original feed/epoch; settle the already admitted final analysis and its delivery. Seal the exact final accepted sequence, point and unchanged timestamps. A dropped frame, stale result, gap or failure cannot masquerade as EOF.
2. Keep pending/unacknowledged points separate from device coverage. Every final Append still needs original sample-age/lead checks and an exact acknowledged outcome. Unknown writes never become drained or retryable.
3. Only a completely acknowledged sealed tail may enter a finite-drain state. It has no right to append more samples, renew clocks or generate an endpoint/hold. The terminal coordinate is the last real accepted point, not an extrapolated trim end.
4. Retain independent visible-player, source, grant/revision, media, qualification, clock and shutdown guards. Deliberate EOF can stop requiring another input sample; it cannot prolong any native authority deadline. Unexpected starvation or authority loss still requests immediate owned Stop.
5. Define the phase edge explicitly. Waiting for the earliest permitted phase to reach the final point is a conservative software passage criterion; the latest permitted phase may already have passed it. Stopping when only the latest edge reaches it may cut the tail short. Neither establishes physical completion. Device-specific behavior during that interval remains a qualification input.
6. Send Stop through the existing single-owner/journal/custody path and retain unknown outcomes. Do not substitute Flush, pause, consumer closure, an SSE event or a status read. Restart must restore history only, without reconstructing the closed source's native authority.

Any new HSP observation qualification should explicitly bind the maximum sample age relative to native request send/receive, signed phase error, drift and quantization, user-offset applicability, exact device/session/mode/settings/stream identity, discontinuity policy and the immutable original Play reference. The RPC's generation-time description alone does not prove a fresh uncached REST sample inside that request bracket. Model the end-of-buffer interval and Stop execution residual separately. No values for these assumptions were established by this research.

One conditional native bracket can be stated precisely. Let `send <= receive <= n` be native monotonic times, `A` a qualified maximum sample age preceding send, `E` a qualified timestamp error including integer-millisecond quantization, and `0 <= r < 1` the qualified fractional rate error around unit rate. Assume the reported clock was sampled at some `s` in `[send - A, receive]`, with no pause, seek, correction, clock discontinuity or identity change. For signed `q = reported_current_time_ms * 1_000_000`, the phase interval at `n` is:

```text
lower = q - E + (n - receive) * (1 - r)
upper = q + E + (n - send + A) * (1 + r)
```

This is an application interval deduction under those assumptions. Use checked signed arithmetic and outward rounding, preserving negative phase. Reject underflow in the assumed sample-time range, overflow, reversed brackets, expired observations or excessive interval width. Setting `A` to zero requires evidence of fresh generation within the request, not just a completed HTTP exchange. If no bounded sample-age model is justified, the observation cannot supply this bounded phase.

A proposed `HandyHspObservationQualification` would therefore name its model and carry `admitted_play_states`, `maximum_sample_preceding_send_ns`, `timestamp_error_ns`, `rate_error_ppm`, `maximum_observation_age_ns`, `maximum_round_trip_ns`, `maximum_phase_uncertainty_ns` and explicit user-offset applicability. Restrict the initial model to `Playing`; a fresh `Starving` or `Paused` reading cannot renew that model or establish drain. Other states require a distinct measured contract. A native observation separately retains send/receive, signed reported phase, exact device/session/mode/settings/stream bindings and the original Play reference. A distinct drain qualification must describe nonlooping terminal behavior and the permitted Stop execution residual. These are proposed typed fields, with no default values or inherited HSSP acceptance.

## Current Pulsar comparison

Current source already preserves signed `HspState.current_time` and documents its approximate/non-atomic meaning. `NativeDeviceClock::error_at` measures age from the original synchronization send and drift from receipt. `check_observation` checks uptime progression and unchanged reported clock offset, RTT and user offset; it does not reset age. HSP preparation clips initial readiness to both the original operation deadline and settings-observation freshness. Those limits are implementation behavior, not evidence that a longer observation model has been qualified. [HSP wire](../../crates/pulsar-engine/src/handy_hsp_wire.rs), [device clock](../../crates/pulsar-engine/src/handy_playback_clock.rs), [HSP preparation](../../crates/pulsar-engine/src/handy_hsp_preparation.rs)

The existing `NativeContinuousTiming` consumes qualified HSSP playtime observations. Schema-7 HSP qualification rejects that `continuous_timing` field, so it cannot be reused as an HSP phase contract by changing names. The HSP runtime preserves original Play facts, while Core currently treats tail time at or below `phase.latest_ms + min_lead_ms` as starvation and has no input-seal state. [Continuous timing](../../crates/pulsar-engine/src/handy_continuous_timing.rs), [qualification contract](../../crates/pulsar-protocol/src/handy_qualification.rs), [HSP runtime](../../crates/pulsar-engine/src/handy_hsp_runtime.rs), [Core queue](../../crates/pulsar-core/src/handy_stream.rs)

Open qualification questions include the device/REST sampling delay, allowable clock mismatch, drift and offset changes, behavior before script zero, exact nonlooping buffer-end behavior with automatic pause disabled, last-stroke completion and Stop timing, event loss, remote ownership races, and long-session signed-clock range behavior. The published signed clock versus unsigned point timestamps does not define a wrap policy. A physically measured model/firmware/transport profile is still required; authored tests and successful network responses cannot supply it.

## Retrieval identities

Public repository heads were queried at 2026-10-23T18:39:42Z, followed by exact-commit file retrieval. Notifications were fetched at 18:41:10Z. The GitLab HTML pages exposed only their loading shell; bounded unauthenticated public repository APIs supplied the source bytes.

- RPC observed `main`: [`2398008c10d5f43903ec150e654968501f0ebc83`][rpc-head], committed 2026-10-11T08:47:23Z. Its README still labels the public protocol a draft unsuitable for use at that stage. [RPC README][rpc-readme]
- SDK observed `main`: [`190784911d0ef920a38decb52e1cf5594be6338e`][sdk-head], committed 2025-09-22T13:39:48+02:00.
- REST mutable specification: SHA-256 `9ad90e385ff1bf5f454a9af5f02862ca6cb064654b30df873d7d7389198cb684`, 340,118 bytes, independently retrieved by the parent during this task. Its HSP/HSTP documentation shortlinks were inaccessible to that retrieval. [REST specification][rest]

| Exact-commit source | Bytes | SHA-256 |
| --- | ---: | --- |
| RPC `README.md` | 839 | `3a9bc4186d47902a9b8d52b69a10d88d53d55e1745db9354a2348f63f83cfafe` |
| RPC `public/proto/constants.proto` | 41,346 | `b3467c72e3e759f18560bb7ca76b1dd6031283c88ac8731100f9ec905346b3ff` |
| RPC `public/proto/messages.proto` | 63,218 | `d041b92669e65eebbd9292b0ad9f83cb8f0c089415dbeb968e2a5b8474a04822` |
| RPC `public/proto/notifications.proto` | 7,363 | `867770f8d095629d960701cb001a3fd813299dbbaa79c8c343f2a430d012ec3c` |
| SDK `handy-rest-api-v3-client/src/index.mjs` | 77,347 | `adc545657dbe3f1e86a2c52660ab77271f08dbe6eba5fcad22d3e002d43997ac` |
| SDK `ohdoki-sdk/src/hsp_utils.mjs` | 14,113 | `4a300127f129b747e400c94339feef675fb159bc8a5fbf91b67b476876e0f660` |
| SDK `ohdoki-sdk/src/device_client.mjs` | 117,205 | `9a07654886d74d9d4a42d399b3310e9da0f1076beda519a4e04815374c6bb4c2` |
| SDK `sdk-examples/examples/utils/device-client/demos.mjs` | 54,000 | `3ec2a9ab661f99a3f538d16a4e3cc99906f766325f9fb40d88648c084257246e` |

Temporary source downloads and retrieval metadata were removed after checking this note. This Markdown file is the only retained artifact from this research subtask. No direct quotations were used.

[rpc-head]: https://gitlab.com/sweettechas/platform/handy-public-rpc/-/commit/2398008c10d5f43903ec150e654968501f0ebc83
[sdk-head]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/commit/190784911d0ef920a38decb52e1cf5594be6338e
[rpc-readme]: https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/README.md#L1-3
[rpc-state]: https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/constants.proto#L129-174
[rpc-clock]: https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/messages.proto#L149-171
[rpc-hsp]: https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/messages.proto#L757-857
[rpc-notifications]: https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/notifications.proto#L76-97
[sdk-state]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L182-220
[sdk-play]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L1883-1921
[sdk-sync]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L2410-2459
[sdk-estimate]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L2480-2505
[sdk-hstp]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L1022-1118
[sdk-events]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L673-687
[sdk-stream]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/ohdoki-sdk/src/hsp_utils.mjs#L101-415
[sdk-demo]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/sdk-examples/examples/utils/device-client/demos.mjs#L1132-1179
[sdk-stream-demo]: https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/sdk-examples/examples/utils/device-client/demos.mjs#L1201-1285
[rest]: https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml
