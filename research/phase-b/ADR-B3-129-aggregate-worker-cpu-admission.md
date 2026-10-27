# ADR B3-129: Aggregate admitted worker CPU quotas

Status: implemented; validation and external qualification recorded separately.

## Problem

Each confined worker has a CPU cgroup quota, but the shared resource pool counted only RAM, worker slots, storage and GPU reservations. Two CPU-heavy admitted workers could exceed an owner CPU allowance while remaining inside the other dimensions. A loan bound only to memory could also carry a smaller CPU claim than the launched assistant quota.

## Decision

Add an owner-configured aggregate worker CPU ceiling, default four threads, with validated explicit startup override. Sum actual manifest/request quotas under the same mutex and transaction as RAM/job/GPU admission. A failure updates no counters. Retain the quota on the existing custody reservation until retirement; memory-only claims and retained storage charge zero CPU. Independent cgroup creation validates its CPU quota against the retained claim. Assistant loans match RAM and CPU exactly, and impossible owner claims refuse before pausing generation. Preserve per-worker limits and retained resume budgets.

This accounts declared worker CPU quotas, not CPU discovery, reservation of physical cores, host metadata/control threads, observed process utilization or performance qualification. The existing cgroup quota remains the independent enforcement mechanism. No support matrix or release gate is reduced.

## Evidence

See [the implementation record](../../docs/PULSAR_CPU_ADMISSION.md) and its checkpoint for completed checks, initial failures, source/binary subjects and cleanup. The native CPU-only handoff uses an eight-GiB owner pool that can hold both exact RAM claims, and a four-thread ceiling that cannot hold generation's two threads plus the assistant's four. Passive journal and pool observation must show bounded claims, intentional pause, automatic same-job recovery and terminal release; test invocation alone is not proof.
