# Explicit Handy clock synchronization

The CLI can issue one explicitly authorized `GET /hstp/clocksync?s=true` for an exact current
controller. This GET changes device state. It is separate from the read-only inspection broker.
The original request identity is reserved before an independent journal intent, its main-database
witness and its single-use dispatch claim. The controller and previous server-clock mapping are
retired before the claim; acknowledgment never restores them.

```sh
pulsar handy-clock-sync PROJECT --revision PROJECT_REVISION \
  --acquisition-request-id ACQUISITION_REQUEST --controller-id CONTROLLER_ID \
  --generation GENERATION --request-id SYNC_REQUEST --allow-online-synchronization

pulsar handy-clock-sync-status PROJECT --sync-request-id SYNC_REQUEST

pulsar handy-clock-sync-cancel PROJECT --sync-request-id SYNC_REQUEST \
  --record-id EXACT_SYNC_RECORD --request-id CANCELLATION_REQUEST
```

Synchronization requires current owner authority, project Read/Export access, the exact
controller generation, accepted preparation qualification and a fresh stopped-device timing
inspection. The two preflight user offsets must match. The send has at most ten seconds and
cannot extend the original controller deadline. It uses a fresh fixed-origin HTTP client with
captured credentials, no redirects, proxy or application retry. The pinned ureq implementation
does not automatically repeat the GET after handoff; loopback tests also count requests after
lost, truncated, delayed, redirected and failed replies.

## Recovery and connection reservations

Every original retry returns history without another GET. Status requires owner authority and
project Read access but no revision. Only an exact Prepared attempt can be cancelled; repeating
its cancellation is harmless. Cancellation never revives the old controller or clock mapping.

| State | Meaning | Effect on new preparation |
| --- | --- | --- |
| Prepared | Durable intent, no dispatch claim issued | Blocked until exact cancellation |
| Claimed | Handoff claimed; completion may be missing | Blocked pending reconciliation |
| Unknown | Transport outcome is uncertain | Blocked pending reconciliation |
| Acknowledged | Bounded synchronous response retained | Fresh preparation may proceed |
| Cancelled | Unclaimed intent retired | Fresh preparation may proceed |

Reservations are independent of native controller entries and bind the keyed credential route,
namespace and logical device. Expiry, release and restart cannot remove unresolved custody.
Matching credential routes or logical device IDs remain blocked when the other identifier
changes; an unresolved namespace change also refuses. Two entirely different logical and
credential identities are not proof of distinct physical units. An unknown outcome may mean the device
synchronized despite a lost reply. A new request ID or a fresh timing GET cannot prove the old
operation has finished. Reconciliation for attempted/unknown synchronization remains unfinished;
the current cancellation command deliberately cannot clear those states.

Completion is retained even if project authority changes during I/O. Current Read authority is
checked before disclosing the response. Failed completion storage leaves the claim reserved;
retrying the original operation cannot reissue the effect. Startup refuses a missing journal
linked by a retained witness rather than creating empty history.

## What acknowledgment establishes

The response retains device uptime, reported clock offset, latest-sync RTT and the native
request bracket. The user offset is the observed value **before** synchronization. No offset
is written. The report always has `synchronization_verified: false` and `playback_enabled: false`.
The vendor's synchronous response is an operation observation; it is not a measured accuracy,
freshness or physical stopping guarantee. There is no remote idempotency key or operation ID.
[Official schema](https://www.handyfeeling.com/api/handy-rest/v3/docs/spec.yaml)

The [combined native timing path](PULSAR_HANDY_PLAYBACK_CLOCK.md) now requires an explicit
version-4 error/age/user-offset contract and combines server/device uncertainty. Its actual
physical evidence and acceptance remain required. Public physical play/stop integration,
browser presentation timing and actual hardware qualification remain open. Do not interpret
the archived original-controller snapshot as current arming.

## Journal compatibility and verification

Device-journal version 4 adds independent synchronization records and atomically bound response
observations. Existing setup and play/stop history are preserved. The preceding application
refuses version 4 before modifying its database or WAL. The scoped native campaign also verifies
unchanged project/journal database and WAL hashes across that refusal.

The [checkpoint](evidence/roadmap-2026-10-22/handy-clock-sync/checkpoint.json) records 335 Protocol,
153 Handy Engine, 28 Handy Client (one existing desktop case ignored), 19 journal and 51 playback
tests; both strict Clippy configurations; app/worker build; format/architecture checks; and an
isolated native CLI campaign with seeded historical records and actual cancellation-reply loss.
The corrected future-schema fixture failure is retained. No real device, synchronization,
setup, play/stop or physical acceptance was used in this campaign.
