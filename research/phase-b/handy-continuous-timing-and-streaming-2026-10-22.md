# Handy 2 Pro: continuous timing and generated motion

Research date: 2026-10-22. Public documentation and manufacturer source only; no device endpoints, credentials, firmware changes, builds, or hardware commands were used. This follows the [October 1 integration research](handy-integration-research-2026-10-01.md) and [October 9 FW4 refresh](handy2-pro-fw4-contract-refresh-2026-10-09.md). The selected device is Handy 2 Pro; its actual firmware remains unobserved here.

## Decision

**HSP is the documented route for adding generated timed positions during playback.** The manufacturer explicitly describes adding points while playing and says FW4 implements HSSP through HSP. HSSP remains a useful hosted-script interface. HDSP targets individual moves without a server-time schedule. [FW4 functionality](https://intercom.help/ohdoki/en/articles/9457599-what-s-new-in-firmware-4), [HDSP guide](https://ohdoki.notion.site/Handy-Direct-Streaming-Protocol-HDSP-7be2c2a11a544da4bf70ce7a947b00f9)

**Reading timing while playing is a plausible observation seam, but the retrieved sources do not establish a qualified effective-clock renewal contract.** They expose device-time reads without a stated stopped-mode precondition. They do not promise readback freshness/atomicity, nondisruptive clock correction, or a bounded correspondence between those numbers and physical movement. Keep the current synchronization deadline until a separately accepted readback model supplies those missing bounds.

## Timing operations and their meaning

The production base remains `https://www.handyfeeling.com/api/handy-rest/v3`. The OpenAPI identifies itself as v3-beta/3.0.0 and requires FW4 or newer. [OpenAPI](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)

Handy 2 supports only Firmware 4, but the exact installed release and its capabilities must still be observed; draft references to FW4.4 are not evidence of deployment. [Handy 2 firmware guide](https://intercom.help/ohdoki/en/articles/12161275-updating-your-handy-2)

| Operation | Observed wire contract |
| --- | --- |
| `GET /servertime` | Credential-free `{server_time}` sample. |
| `GET /hstp/info` | Device response containing `time`, `clock_offset`, `rtd`. |
| `GET /hstp/offset` | Separate user adjustment `{offset}`. |
| `GET /hstp/clocksync?s=true` | Starts synchronization; synchronous result also appears through SSE. Default `s=false` completes asynchronously. |
| `PUT /hstp/offset` | Writes `{offset}`; a separate effect. |
| `PUT /hssp/synctime` | Writes `{current_time,server_time,filter?}` to adjust script playtime. |

The pinned [REST client](https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L1004) implements these operations. It renames wire `clock_offset` to SDK `offset`; that normalized field must not be confused with the separate user-adjustment response. Its HSTP wrapper neither changes mode nor sends stop, but wrapper behavior is not a firmware guarantee.

`time` is device uptime in milliseconds; `clock_offset` relates server and device clocks; `rtd` describes the latest synchronization, not the present HTTP read. The newer [RPC clock messages](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/messages.proto#L148) identify server UNIX time versus device uptime and say the client stores RTD. They do not provide an error bound. Thus `time + clock_offset` is a candidate interpretation requiring an accepted device contract; user-offset sign/application and HSSP scheduler correspondence must also be established.

No retrieved owning source says `/hstp/clocksync` stops HSSP, switches its mode, preserves uninterrupted playback, slews instead of stepping, or has a bounded disturbance. Absence of a documented prohibition establishes none of those guarantees. Treat it as effectful even though the HTTP method is GET. A read of `/hstp/info` does not perform that synchronization or reset the age of the original synchronization.

Script-time correction is separately documented. The current [RPC time-set declaration](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/messages.proto#L822) describes a filter in `[0,1]`, default `0.5`: `1` forces time; lower values converge. This is draft RPC evidence, not an independently verified REST-to-firmware transfer function. The manufacturer's [media synchronizer](https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/ohdoki-sdk/src/media_sync.mjs#L30) periodically sends HSP `synctime` using `HTMLMediaElement.currentTime` and a client/server estimate. Its executable defaults are 2 seconds/0.9 initially, then 10 seconds/0.5; a comment says 15 seconds instead. These are implementation defaults, not qualified error limits.

Do not substitute HSP status for exact presentation evidence. The updated [RPC state contract](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/constants.proto#L129) says `current_time` advances during playback, while `current_point` can move backward and is not physical position. Fields are assembled at different instants; paused time may drift. It explicitly treats the snapshot as approximate status. This supports continuity diagnostics with measured tolerance, not exact-frame synchronization.

## Streaming contract

The core REST operations are below; the [pinned client implementation](https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/handy-rest-api-v3-client/src/index.mjs#L1815) supplies the request bodies.

| Operation | JSON body |
| --- | --- |
| `PUT /hsp/setup` | Optional `{stream_id}` |
| `PUT /hsp/add` | `{points:[{t,x}],tail_point_stream_index,flush?,tail_point_threshold?}` |
| `PUT /hsp/play` | `{start_time,server_time?,playback_rate?,pause_on_starving?,loop?,add?}` |
| `PUT /hsp/stop`, `PUT /hsp/flush`, `GET /hsp/state` | No body |
| `PUT /hsp/synctime` | `{current_time,server_time,filter?}` |

REST limits: setup ID `1..4294967295`; at most 100 points per Add; `t` integer milliseconds ≥0; tail/threshold ≥1. Capacity is reported by the device. A full buffer evicts oldest points. Flush clears points; it is not a documented stop. Additional controls are bodyless `PUT /hsp/pause`, `PUT /hsp/resume {pick_up?}`, and PUT setters `/hsp/threshold {tail_point_threshold}`, `/hsp/pause/onstarving {pause_on_starving}`, `/hsp/loop {loop}`, `/hsp/playbackrate {playback_rate}`. [OpenAPI](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)

FW4.0.12 explicitly added playing before points arrive, supporting live input. Its changes also include a gentler start and buffer-end handling; startup behavior therefore belongs in the exact firmware profile. [Release notes](https://intercom.help/ohdoki/en/articles/10055192-update-v4-0-12)

The [HSP helper](https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/ohdoki-sdk/src/hsp_utils.mjs#L207) batches/serializes pushes, follows tail indices and drops changed-stream consumers. This suggests an implementation structure, not deduplication. No retrieved contract guarantees Add idempotency, an expected-stream precondition, or duplicate/late/out-of-order-point behavior. An ambiguous Add cannot safely become a blind retry merely because its tail index is known.

Draft [RPC HSP messages](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/messages.proto#L757) use uint32 point times/positions and signed int32 start time. `pause_on_starving:true` freezes stream time and resumes automatically when points arrive, which can diverge from continuing media. FW4.4 annotations add configurable jitter filtering and script-speed behavior; earlier filtering was always enabled. Those settings are absent from the current REST schema. Their availability on this particular Handy 2 Pro is unknown.

HDSP's six PUT operations are `/hdsp/xava`, `/xavp`, `/xpva`, `/xpvp`, `/xat`, `/xpt` (each under `/hdsp`), carrying the named pair plus optional `stop_on_target,immediate_rsp`. They have no server-time field. The [device-client API declarations](https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/blob/190784911d0ef920a38decb52e1cf5594be6338e/ohdoki-sdk/src/device_client.mjs#L458) expose individual moves, not timed append chunks. The legacy-linked HDSP guide's travel/speed examples do not establish Handy 2 Pro limits. FW4 advertises HDSP stopping, but this REST schema has no `/hdsp/stop`; [RPC command 746](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/public/proto/handy_rpc.proto#L96) is a different interface.

Unresolved schema contradictions: REST PointPosition describes 0–100 but caps at 50; XPT defines `xp,t` but requires `xa,va`; HSP pause/resume use bare-state responses while other operations wrap `result`. Do not silently repair these by treating SDK behavior as the normative server contract. [OpenAPI](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)

## Practical Pulsar choices and qualification boundary

Current Pulsar [device timing inspection](../../crates/pulsar-engine/src/handy_device_timing_control.rs) admits an Active controller and matches its prepared device state; [NativeDeviceClock](../../crates/pulsar-engine/src/handy_playback_clock.rs) retains the original synchronization age. A playing-state readback model is therefore a new authority contract, not a change to the existing maximum-age calculation.

1. Keep the existing bounded HSSP path for prepared scripts. Implement a distinct playing-state timing observer that retains native send/receive brackets, exact boot/socket/mode/stream identity, settings and user offset. Publish no renewed execution authority unless an accepted firmware-specific contract bounds report acquisition, uptime/offset arithmetic, scheduler correspondence, drift and error. A new read cannot inherit the current `SynchronousResponseEffectiveClock` model's proof by changing its timestamp.
2. If that readback contract cannot be established, stop before the original validity window expires and use fresh synchronization/reconciliation/resumption. Adding effectful `synctime` or `clocksync` requires a separately admitted, journaled operation and measured discontinuity behavior; it is not a heartbeat.
3. For generated motion, introduce a separate HSP stream identity and bounded append queue under the existing native owner. Retain exact chunks and unknown outcomes. Admit only fresh, feasible future points; do not silently turn missing observations into motion or replay a late chunk. Buffer horizon, starvation policy and stop/cancellation behavior need their own measured contract.
4. Bind browser media time to native time independently. The manufacturer [functionality guide](https://ohdoki.notion.site/Handy-functionality-2a5b14198a6c4204b270c47521fb5da9) describes pairing media position with estimated server time and acknowledges player-time, video-processing and refresh uncertainty. Pulsar's current browser adapter sends media timestamps but stamps native receipt time; a captured tab's stream clock does not expose its source player's pause/seek state. Use the controlled local player first. Preserve stop on stale observations, pause, seek, end and owner loss.

These are implementation choices, not acceptance findings. The actual Handy 2 Pro model/firmware, operative filtering, range/dynamics, browser-to-native timing, cloud jitter, buffer exhaustion, disconnect/Engine death, and physical stop behavior remain unqualified. Public endpoint availability, RTD, RPC acknowledgments, and reported stopped state cannot supply those measurements. No universal physical latency or stop bound follows from this research.

## Retrieved identities and limits

- [REST v3 specification](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml): SHA-256 `9ad90e385ff1bf5f454a9af5f02862ca6cb064654b30df873d7d7389198cb684`, unchanged from October 9.
- [Manufacturer SDK](https://gitlab.com/sweettechas/platform/handyfeeling-utils/-/tree/190784911d0ef920a38decb52e1cf5594be6338e): current observed HEAD `190784911d0ef920a38decb52e1cf5594be6338e`; linked by the official [SDK page](https://ohdoki.notion.site/SDK-v2-2f77eee0c66d80f9bb8de2ba4ebdeb6d).
- [Public RPC](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/tree/2398008c10d5f43903ec150e654968501f0ebc83): current observed HEAD `2398008c10d5f43903ec150e654968501f0ebc83` (2026-10-11), newer than the previous note's pin. `messages.proto` SHA-256 `d041b92669e65eebbd9292b0ad9f83cb8f0c089415dbeb968e2a5b8474a04822`; `constants.proto` `b3467c72e3e759f18560bb7ca76b1dd6031283c88ac8731100f9ec905346b3ff`. Its [README](https://gitlab.com/sweettechas/platform/handy-public-rpc/-/blob/2398008c10d5f43903ec150e654968501f0ebc83/README.md) still labels the protocol an initial draft, not ready for use.

GitLab browser rendering returned 403 for several files; public GitLab API retrieval at exact commits succeeded. HSP documentation shortlinks failed, so the OpenAPI, owning SDK source, RPC declarations and firmware help articles supply the cited evidence. No raw upstream code was vendored. Only this note was written.
