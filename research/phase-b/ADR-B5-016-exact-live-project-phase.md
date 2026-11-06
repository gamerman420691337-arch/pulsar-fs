# ADR B5-016: preserve exact live project phase through epoch handoff

Status: implemented; authored native functional validation passed. Real acquisition and device/release qualification remain open.

The prior bounded epoch handoff refused source anchors with fractional project nanoseconds. Ordinary frame rates produce such anchors, and replacing them with their integer floor loses phase over repeated handoffs.

Keep the integer project anchor for its published floor and legacy default behavior. Add an optional reduced exact-seconds anchor whose floor must match. Evaluate exact anchor plus original source delta and quantize only the final mapped project timestamp. Presentation deadlines retain their independently reset local epoch origin.

Expose a pure declaration-derivation helper and a separately authenticated engine operation that derives against the current predecessor under authority locks. Its durable fingerprint contains the explicit anchor/lifetime, so replay precedes derivation and works after predecessor retirement. Preserve fresh actor/boot/revision/grants, unchanged source/profile/policy, bounded resource admission and no implicit rearm. A changed fractional phase must refuse even when the integer floor is unchanged.

The full original clock remains in native pair/manifest identity and evidence. This change does not qualify acquisition clocks, continuity gaps, model quality, physical dispatch or Handy. See [implementation and retained evidence](../../docs/PULSAR_EXACT_LIVE_PROJECT_PHASE.md).
