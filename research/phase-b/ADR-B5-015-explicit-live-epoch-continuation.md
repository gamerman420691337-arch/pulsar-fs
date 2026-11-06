# ADR B5-015: explicit source-preserving capture epoch continuation

Status: implemented; authored native functional validation passed. Real acquisition and device/release qualification remain open.

Native manifests bind absolute entry deadlines. Extending them in place would change admitted lifetime, provenance and worker identity after launch. Continuation therefore requires a fresh bounded epoch, new worker/model attempts and new local clock origin, with predecessor cancellation under the same authority locks.

The explicit control operation preserves the source identity and exact project correspondence while rejecting changed source/profile/policy declarations, non-advancing source anchors, fractional project-anchor loss and retired or unauthorized predecessors. Durable request replay never reactivates either epoch. No work pair, estimate or local lease clock transfers across the boundary.

Resource admission and receipt failure must preserve the old epoch. Successful publication retires old authority; outstanding workers remain bounded and retain reservations until reaped. Every frame/input/result effect continues to require its own fresh epoch scope. The continuation report is predecessor provenance, not authority for old data.

This implements an explicit epoch handoff needed for sustained source workflows. Actual continuous acquisition, automatic segmentation, measured gap/clock correspondence, player/device scheduling, Handy and all roadmap qualification gates remain open. See [implementation and retained evidence](../../docs/PULSAR_LIVE_EPOCH_CONTINUATION.md).
