# Guarded native Handy playback submission

Successor: [Engine daemon shutdown](PULSAR_ENGINE_SHUTDOWN.md) connects the retained owner to
SIGTERM/SIGINT and joined IPC/background retirement; physical shutdown qualification remains open.

The CLI now exposes `handy-play`, `handy-play-status` and `handy-play-cancel` through
the shared Engine. A new play requires an accepted qualification, acknowledged setup,
current native preparation/controller, fresh combined device/server timing, current
project/grants and explicit play consent. This implementation has local software evidence;
no real Handy was contacted or qualified. Browser-to-Handy playback remains unfinished.

## Commands and current limits

With the exact IDs returned by the preparation, controller and timing workflows:

```sh
pulsar handy-play PROJECT --revision REVISION --acquisition-request-id ACQUIRE_ID --controller-id CONTROLLER_ID --generation GENERATION --clock-request-id CLOCK_ID --start-time-ms OFFSET --request-id PLAY_ID --allow-play
pulsar handy-play-status PROJECT --play-request-id PLAY_ID
pulsar handy-play-cancel PROJECT --play-request-id PLAY_ID --record-id RECORD_SHA256 --request-id CANCEL_ID
```

These are command shapes, not a qualified-device demo. `handy-play` can issue an HSSP
play command only after all current admission checks pass. Keep the same original request
ID and arguments when resolving an uncertain reply. A retry reads history and may repair
missing metadata; it cannot obtain a new clock, restore a controller or enqueue another play.
An original reservation with no history remains inert. Changing the original selection refuses.

`handy-play-status` returns original operation history: Prepared, Claimed, Acknowledged,
Unknown or Cancelled. None proves current physical motion or stopping. `handy-play-cancel`
only cancels an exact unclaimed Prepared operation. It does not stop claimed playback.
Use the existing controller release operation to retire current native ownership and retain
a stop request. An acknowledged stop still leaves physical stopping unconfirmed.

The broad `physical_playback` capability and desktop Arm remain disabled. The three scoped
protocol operations are advertised separately; their presence is not general device readiness.
No browser-player control sends toy commands. Credential values remain outside requests and
project files; existing encrypted environment-name registration still applies.

## Ownership and recovery

The Engine reserves the original request before issuing a runtime effect, then records the
typed journal operation/effect and main-request witness before queueing. Failures after local
effect construction retire the controller and preserve stop custody. An exact orphan history
or incomplete witness can be repaired without queueing. Present witnesses must match the
immutable journal subject and may only lag valid state advancement; corruption refuses.
Startup checks bounded linked witnesses before changing journal ownership.

One Engine-owned dispatcher per preparation handles play and stop against the same runtime
and captured transport. Its final play authorizer shares the original DB mutex, rechecks
authentication, grants, revision, configuration/qualification, exact witness, controller
generation, native clock and route custody, and holds the mutation guards through the runtime
claim. It releases them before HTTP. The worker retains resources without retaining Engine.

Unknown play outcomes retain their stop obligation even when play metadata cannot be written.
Observed completion retries update only metadata. A rejected native handoff remains Claimed
and unresolved; it is not fabricated into a transport observation. Engine destruction drains
and joins the owner. This does not yet establish graceful daemon signal shutdown or emergency
stopping under storage failure.

Controller reports distinguish current Playing custody from the original Active snapshot in
history. Renewal advances the controller generation without refreshing the original device
synchronization age. Source clock expiry retires playback even if the lease remains live.
Continuous playback across synchronization refresh is not established by this increment.

## Journal compatibility and evidence

Device-journal version 6 retains the version-5 layout and adds a reader barrier for play-route
exclusion. Prepared, Claimed, Acknowledged and Unknown plays reserve their logical device and
credential route; changing a device alias or route namespace cannot bypass that custody.
Setup and synchronization claims respect those reservations. Only known-unclaimed cancellation
releases a typed play reservation; attempted-effect reconciliation remains open. The main
project schema is unchanged; its version depends on which existing features initialized state.

The [checkpoint](evidence/roadmap-2026-10-22/handy-play-submission/checkpoint.json) identifies
the checked source, binary hashes, commands, test counts and native CLI campaign. Composed
Engine tests use localhost HTTP and synthetic accepted timing. Native CLI recovery uses seeded
synthetic history in a network-isolated namespace. Neither supplies physical-device acceptance.

The checked scope passes 341 Protocol, 180 Handy Engine, 39 Handy Client, 37 journal and
66 playback-runtime tests; one existing desktop client case is ignored. Both strict lint
configurations, App/Worker builds and architecture/format guards pass. The native campaign
recovers a discarded committed cancellation response after restart, repairs an absent original
witness without dispatch, preserves all five operation states, and refuses corrupt or missing
linked history. The actual preceding v5 reader refuses v6 without changing database or existing
WAL bytes, including explicitly held nonempty WALs. An absent WAL may be created empty by SQLite;
whole-directory or sidecar-presence identity is not claimed. The fresh native fixture initialized
main schema 9; the existing supported ceiling of 34 is unchanged. Intermediate test compilation,
fixture setup, main-version assumption and readiness-probe harness failures are retained with
their corrected results in the evidence packet.

Remaining work includes attempted-effect reconciliation, graceful daemon shutdown, emergency
stop behavior under storage failure, browser presentation/heartbeat correspondence, continuous
clock renewal, and actual device/model/media/platform/storage/human/release qualification.
Full roadmap PARTIAL; the objective remains active.
