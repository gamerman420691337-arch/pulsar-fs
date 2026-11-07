# ADR B5-005: authenticated capture lease lifecycle

Status: engine/protocol/CLI lease lifecycle implemented; frame acquisition, continuous inference and physical dispatch open.

The bounded RGB owner previously had no authenticated engine-session lifetime. Live pixel ownership must be pinned to one actor, project revision and boot so that restart, revocation or revision changes cannot reuse old capture authority.

Add project-scoped limits/start/status/stop operations. Start requires Read, ImportSource and Generate, a matching boot nonce and current revision. Mint source/capture epochs in the server; validate declared source/project/presentation anchors and reserve pixel capacity. Remember start before publishing a runtime. Replay returns current/retired state rather than recreating it. Restart reconstructs no runtime and rejects the old nonce. Stop is replayable retirement only, with no physical stop claim.

Bound active leases to eight and retained reports to 64. Current leases have an absolute administrative lifetime at most 30 seconds; status cannot renew them. Check invalidation on requests and in a weak engine-owned 250ms maintenance loop. Clock observations and pixel admission describe software state; they do not calibrate source/player clocks or prove real-time behavior.

No frame-input, camera/stream access, model execution or device command is introduced by this lifecycle. Those successors must use the lease authority and preserve confinement, aggregate resource admission, cancellation and result fencing. There is no new project database schema or portable format, and ephemeral source identities are not adopted as imported media.
