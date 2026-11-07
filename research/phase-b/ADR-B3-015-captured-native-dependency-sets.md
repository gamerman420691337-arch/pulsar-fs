# ADR B3-015: captured native dependency sets

Status: implemented capture prerequisite; production CUDA remains unsupported.

## Context

The CUDA feasibility probe computed on GPU using mutable host runtime directories. The new resource declaration separates virtual mappings from physical RAM, but production enablement still needs exact selected native dependencies. A mapped-file inventory is useful evidence for one graph; it is not a complete dependency qualification or authority grant.

## Decision

Provide an engine library helper that validates an explicit bounded set of expected hashes, lengths and loader aliases, captures opened regular files into private lifetime-owned snapshots and verifies captured bytes/aliases. Grant no executable permission or native loading in the engine. Explicit cleanup reports errors. No provider admission, durable state or database/profile schema changes.

Exercise the helper through an experimental native child using a captured loader and libraries with host runtime directories absent. Check the authored numerical oracle, strict node trace and observed mappings. Keep device/sysfs exposure explicit and retain current CPU-only production admission.

## Authority and limitations

The future owner broker must authorize and bind the selected set, reserve storage/device resources, own cancellation and capture/mount lifetime, and decide durable reader/recovery semantics. The helper's byte bounds do not impose filesystem deadlines or protect against arbitrary same-user mutation. Rehash before read-only confinement; descriptor-bound mounting and complete future dependency behavior remain open.

The experimental device, 16 GiB address space and observed library set are research configuration, not a general adapter profile or whole-worker VRAM quota. No automatic host dependency search, supplier native loading in the engine or silent CPU substitution is permitted by this change.

## Evidence and rollback

[Capture interface and native scenario](../../docs/PULSAR_NATIVE_DEPENDENCY_CAPTURE.md) and [receipt](../../docs/evidence/roadmap-2026-10-01/native-dependency-capture/validation.json) retain actual capture/cleanup, digest/alias/non-regular refusal, authored CUDA computation and current CPU compatibility. Initial alias rejection and pre-cleanup-reporting evidence are separate from final checks. No private data or device playback is used.

The helper has no enabled engine command/session caller. It can be withdrawn without changing durable state; enabling GPU use requires the remaining authority/resource/recovery work rather than relaxing validation.
