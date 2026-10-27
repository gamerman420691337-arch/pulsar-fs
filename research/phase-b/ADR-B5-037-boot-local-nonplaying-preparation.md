# ADR B5-037 — Retain nonplaying native state without inventing controller liveness

Retain the captured program, accepted timing, credential snapshot, disarmed runtime and exact
transport binding in a boot-local owner registry. Persist the original request receipt before
publishing native state; a missing native entry is retired, even when its receipt says it was
prepared. Exact replay must not repeat observation or reconstruct retired state. Install and
retire under the project DB lock, with retirement after durable configuration/grant changes.

A paired session authenticates an actor but ordinary IPC connections last only one request.
Neither is a live controller. This preparation creates no controller, arm or physical effect.
A complete existence query over the selected logical device's play/stop history initializes
unconfirmed stopping conservatively; saved setup or stopped-stream observations do not clear it.
Live controller lifetime, clock acquisition and physical-effect reconciliation remain distinct
requirements before public play can consume these inputs.
