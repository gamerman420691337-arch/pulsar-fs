# ADR B3-128: Profile RAM within owner limits

Status: implemented; native and broader qualification recorded separately.

## Problem

Ordinary generation, CUDA generation preflight and preview fixed their host claim at 1536 MiB. A valid profile declaring a larger RAM minimum refused even when the owner pool had room. Live CUDA could already grow the claim, but live CPU could not. Updating only the manifest would leave the actual aggregate reservation too small.

## Decision

One private Engine budget function sizes fresh host RAM to `max(existing operation baseline, declared profile minimum)` and refuses claims beyond the owner RAM pool. CPU, output and wall-time classes remain explicit caller limits; a profile exceeding them still refuses. The helper applies to generation, CUDA preflight, preview and live CPU/CUDA manifests. Generation and preview reserve the resulting manifest RAM, not the old constant. Existing aggregate pool, interactive-slot, physical-availability/control reserve, storage, device and confinement checks remain independent.

A resumed attempt uses its exact retained budget. It validates the retained claim against the profile and current owner RAM limit; it never grows a malformed old budget or clips it to fit a smaller owner pool. Captured time, CPU and output limits are preserved. Dependency/checkpoint checks remain necessary.

The catalog reports the owner RAM ceiling and discloses the fresh 1536 MiB floor, declared minimum and possible aggregate refusal. This does not promise free capacity or qualified models. CPU class, output ceilings, backend support, profile identities, candidate authority and wire shapes remain unchanged. Existing compatible profiles retain their original fresh budgets.

## Evidence and limits

The aggregate regression first exposed an under-reserved manifest-only change; both ordinary reservations were corrected. The broader catalog test required an updated owner-ceiling expectation. Tests cover durable claim recording, overlapping reservations and release, pre-publication owner refusal, exact retained budgets and unchanged CPU/output limits. Pinned learned native cases exercise a deliberately larger unqualified 2 GiB declaration; this is an admission/confinement claim, not a measured model requirement or memory peak.

See [the implementation record](../../docs/PULSAR_PROFILE_RAM_ADMISSION.md) and its source-bound checkpoint. No milestone or model/platform/device/human release gate closes from this change.
