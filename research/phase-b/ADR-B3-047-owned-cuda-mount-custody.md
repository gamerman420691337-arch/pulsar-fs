# ADR B3-047: Retain CUDA mount assets in shared worker custody

Status: Implemented for internal read-only inspection; production CUDA inference admission remains disabled.

The earlier diagnostic mount assembly borrows captured images and device pins while independently owning a raw cgroup. That assembly cannot itself retain image/device/module descriptors through cancellation or a deferred transport/cleanup reaper. Production launch needs owners that follow the same retirement observations as its resource claim.

Shared `WorkerCustody` now retains typed assets behind an internal `Any + Send` container. Installation returns a temporary typed borrow for command preparation; ownership stays in the worker tree. Ordinary retirement drops assets only after direct child exit, transport completion, cgroup emptiness, staging cleanup acknowledgement and exact group retirement. Capture-to-staging handoff drops its worker assets after users and exact group retirement, before returning the image staging owner. Native capture installs supplier input descriptors before spawn, so its actual caller uses the shared asset lifetime.

An internal owned CUDA inspection assembly consumes a supervised image and pinned device owner into that custody before fallible mount/command setup. It reuses the same request/device/alias/module/profile checks as the borrowed diagnostic path, constructs confinement against the existing held group and detaches the pinned image/module descriptors into the installed assets. It creates no new cgroup. Descriptor pins close before image cleanup can release the storage claim. Spawn goes through shared custody rather than exposing a raw command to the caller.

The assembly is a read/hash-only role: it does not authorize inference, measure GPU free memory or create a GPU resource reservation. A native fixture supplies a durable admitted group and CPU claim, a loaded authored policy, supervised authored non-library bytes and actual root-owned NVIDIA device/module pins. It verifies five scenarios: replacement directory entry still mounts the held inode; corrupt backing bytes fail fresh confined hashing; failed spawn; invalid request; and acknowledged staging retirement with blocked deletion. Held transports defer every scenario's cleanup. Independent storage/CPU/lease/image/descriptor observations and empty recovered ledgers establish the tested ownership boundary.

Production integration still requires committed engine admission, a trusted fresh GPU-memory broker, CUDA scheduling claims, inference role launch, bounded egress/result verification, acceptance and restart correspondence. This does not qualify model quality, media, Handy, platform or release behavior.

Evidence: [owned mount checkpoint](../../docs/evidence/roadmap-2026-10-07/owned-cuda-mounts/checkpoint.json).
