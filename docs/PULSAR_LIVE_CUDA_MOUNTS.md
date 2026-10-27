# Owned CUDA live mount custody

Pulsar now has live-specific mount identity and ownership beneath the private CUDA stream contract. It retains the supervised native image, pinned devices and native/module descriptors in shared worker custody before any fallible mount setup. Live declarations carry their own manifest hash and absolute monotonic deadline; they never enter the offline job request contract.

Physical pinning and fresh mounted-byte checks are shared with generation/Preview mounting. The new confined inspection role checks identities and bytes without loading native code or performing GPU IO. It cannot launch inference, reserve GPU memory or grant motion/device authority. Ordinary live execution remains CPU-only.

Actual namespace tests exercise seven live cases and the existing five offline custody cases. They check replaced pathnames, corrupted bytes, failed setup/spawn, retained resources during blocked transport/staging cleanup, altered live identity and expired deadlines. Unit tests check metadata and wire separation. The initial profile-version fixture refusal remains retained.

The [terminal checkpoint](evidence/roadmap-2026-10-07/live-cuda-mounts/checkpoint.json) records 984 workspace passes (52 ignored), 516 CUDA-feature passes (49 ignored), both explicit native custody suites, five shipped CPU application cases without skips and 252 structural checks. All 339 live/archive source records and 64 executable identities match. Native checks use authored library bytes and real pinned device/module mounts; they do not load native supplier code or perform GPU IO. See [ADR B3-053](../research/phase-b/ADR-B3-053-owned-live-cuda-mounts.md) for scope and remaining activation work.

Next comes live-specific native memory observation/admission, a verified loader and persistent stream, original final trace verification and retirement. Fastest-preset live six-axis generation, latency, model/media quality, physical Handy tests, platform and human/release gates remain open.
