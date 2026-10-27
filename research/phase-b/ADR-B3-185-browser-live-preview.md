# ADR B3-185: Browser frames feed existing live authority; reserve actual CPU launch limits

Status: local preview adapter implemented; product browser/device qualification open.

A browser player can provide pixels and presentation timestamps but cannot mint
Engine authority or establish physical synchronization. Add a loopback-only Client
adapter and locally selected video player. Keep Engine credentials native; use the
existing RGB bulk channel, explicit temporal profile, bounded queues and original
leases. Fence pause/seek and late replies; make stale estimates and absent results
visible. Do not substitute toy output or a trained-model claim for a preview.

The actual adapter campaign exposed live staging's one-thread reservation being
transferred into a two-thread CPU worker. Carry a named preparation budget from the
admitted CPU manifest and reserve those exact launch limits before staging. Keep
GPU preparation's distinct one-thread claim and retirement behavior. Preserve the
cgroup budget guard. Native red/green observations use the same authored graph and
transport, not relaxed resource limits.

See [contract, use and limitations](../../docs/PULSAR_BROWSER_LIVE_PREVIEW.md)
and [checkpoint](../../docs/evidence/roadmap-2026-10-12/browser-live-preview/checkpoint.json).
