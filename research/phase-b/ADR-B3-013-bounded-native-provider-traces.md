# ADR B3-013: bounded native provider trace consistency

Status: implemented prerequisite; production CUDA execution remains unsupported.

## Context

Declared provider choice and successful native output do not by themselves show which provider computed each inference. The CUDA feasibility experiment demonstrated actual node assignments and strict CPU-fallback refusal, but the production resource/profile contract still lacks GPU device, dependency and separate virtual-memory facts. Current CPU admission and execution must remain unchanged while those requirements are implemented.

## Decision

Add a pure bounded parser in the engine library for observed ORT profiling bytes. It accepts an explicit CPU/CUDA pin and independently supplied run count, checks event/byte/run limits, integer clocks, process/thread containment, non-overlapping same-thread runs, exact observed node providers and computation in every run. Known host/device transfers cannot replace computation. Unknown event shapes/providers refuse. Return exact original-byte SHA-256 and typed counts.

Trace validation supplies consistency, not authenticity, model/runtime/attempt binding, complete node coverage, qualification or authority. The caller must bind those facts separately. Do not infer CUDA support from parser availability, a Cargo feature, registration or a permissive session that silently computes on CPU.

## Compatibility and authority

No current wire command, stored profile schema, database barrier, resource budget, native session or capability changes. The standalone native example is read-only and reaches this same library function. Production profile admission still refuses CUDA. A future adapter must bind the original trace and independently observed run count into its attempt receipt and reserve bounded trace storage.

The GPU memory contract must separate physical cgroup RAM from virtual address space. The current shared limit demonstrably refuses dependency loading for the identified CUDA runtime under a 1.5 GiB address-space cap; fresh sampled high-water observations exceed that virtual size while resident memory is smaller. Do not silently reinterpret the existing CPU budget or claim that the CUDA arena is a whole-worker VRAM quota.

## Evidence and rollback

[Strict trace evidence](../../docs/evidence/roadmap-2026-10-01/strict-inference-traces/validation.json) retains full Rust gates, actual native reader positives/refusals, a fresh authored CUDA numerical/trace result and the failed address-space-limited attempt. [Interface and limitations](../../docs/PULSAR_STRICT_INFERENCE_TRACES.md) describe the bounded contract. No private media/model, sterile evaluation or physical playback was used.

The new helper currently has no enabled execution caller. It can be withdrawn before integration without changing durable state or current CPU behavior. Once incorporated into GPU receipts, removing or relaxing its checks requires a new compatibility and authority decision; accepting unproven provider execution is not a fallback.
