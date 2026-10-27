# ADR B3-016: cooperative GPU reservations

Status: implemented accounting prerequisite; production CUDA remains unsupported.

## Decision

Extend the existing resource pool with explicit immutable per-device policies, fresh broker-supplied memory observations and offline/interactive GPU claims. Preserve the default no-GPU pool. Commit RAM, worker slot and GPU charges under the same mutex; release all attached resources through the reservation lifetime. Refuse a second nonzero storage attachment instead of leaking the earlier accounting charge.

Check exact UUID/ordinal and observation age before admission and again under the lock. Reserve an interactive slice from offline work and a free-memory control floor. Do not equate cooperative claims, observed free memory or the CUDA arena with a hardware-enforced total VRAM quota.

## Authority and compatibility

This Rust library interface is neither a wire command nor device authorization. The future broker must establish trusted observations, permitted device nodes, execution lifetime and recovery fencing. Accounting is local to one pool/process. Restart, other processes and driver allocation behavior do not inherit or honor these claims automatically. No database/profile schema or default CPU limits change.

## Evidence and rollback

[Interface and checks](../../docs/PULSAR_GPU_RESOURCE_ACCOUNTING.md) and [receipt](../../docs/evidence/roadmap-2026-10-01/gpu-resource-accounting/validation.json) retain thread competition, release and failed admission tests plus a read-only native host-observation scenario. Current CPU compatibility is checked separately. No GPU execution, private data or device playback is performed.

Before enabled GPU integration, withdrawing the new interface changes no durable GPU authority. Preserve the storage leak correction. Future enabling work must bind resource/device/dependency facts to actual attempts and reader/recovery semantics.
