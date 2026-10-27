# ADR B3-014: explicit CUDA resource declarations

Status: implemented prerequisite; production CUDA remains unsupported.

## Context

The authored CUDA probe requires native virtual mappings larger than the current 1.5 GiB physical worker budget. Raising the existing memory field would change physical admission. GPU identity, scheduling reservation and arena allocation also require distinct meanings before adapter enablement.

## Decision

Add an optional, strictly validated CUDA resource declaration. Version 1 omits it and retains exact existing CPU serialization. Version 2 requires CUDA and the declaration. Separate launch address space from physical cgroup RAM; resolve it only after existing adapter admission. CPU and live launch limits retain existing behavior.

Device UUID and ordinal are declarations, not observation or permission. Reservation is cooperative intent, not a total enforced VRAM quota. Arena allocation is narrower still. Keep the production CPU-only guard until dependency capture, device brokering, aggregate resource scheduling and actual strict session evidence are implemented.

## Durable compatibility and authority

No database or portable-format migration is added because unsupported CUDA cannot reach accepted generation. Preflight Stash batch start and migration before discovery, model capture or ledger mutation; the former direct-file path could previously persist unsupported intent. Preserve readable historical version-1 intent. A preceding strict profile reader refuses version-2 fields. Do not save new intent merely because its structure validates.

## Evidence and rollback

[Resource contract evidence](../../docs/evidence/roadmap-2026-10-01/cuda-resource-contract/validation.json) binds current source, Rust and native checks, exact old/current CPU profile byte comparison, old-reader refusal and job/ledger preservation. Initial constructor compile failures are retained separately. [Contract](../../docs/PULSAR_CUDA_RESOURCE_CONTRACT.md) states bounds and open enabling work.

Before CUDA is enabled, withdrawing this prerequisite changes no accepted durable CUDA state. Keep CPU bytes and limits unchanged. Once durable CUDA attempts become possible, their reader barrier, resource observations and recovery semantics require an explicit new decision.
