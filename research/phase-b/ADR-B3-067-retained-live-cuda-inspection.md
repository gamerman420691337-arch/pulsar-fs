# ADR B3-067: Project-authorized retained live CUDA inspection

Status: Implemented; scoped software verification passed.
Date: 2026-10-08

## Problem

Capture Status describes an ephemeral registry entry. Original CUDA evidence survives registry retirement and Engine restart in schema22, but the first-party CLI cannot query its historical descriptors. Reading SQLite directly bypasses the normal product interface and cannot stand in for that capability.

## Decision

Add the nonmutating `InspectLiveCudaEvidence` query and `live-capture evidence PROJECT CURRENT_REVISION CAPTURE_EPOCH`. Require current project Read authority and the current expected project revision. Scope lookup to both project and capture epoch. Historical capture revision and boot remain in the response; they do not mint a new lease, launch work, resolve review or confer device permission.

Reuse the startup reader's bounded SQL shape checks, evidence memory admission, canonical original origin/manifest/summary validation, indexed identity correspondence, saved admission membership and native receipt/trace consistency. Factor the per-row reader so startup validation and inspection cannot silently diverge. Return compact typed descriptors and historical actor/request identities, never original artifact paths or content. Client response hydration validates the record and project identity.

This adds protocol vocabulary without changing the database or worker format. Older readers cannot request the new operation; existing operation semantics are unchanged. No inference runtime is loaded to inspect retained evidence, and no recorded source, runtime or worker pathname is opened.

## Validation and remaining scope

Cover current Read grant/revocation, stale expected revision, missing/foreign epoch, a later current project revision, original correspondence, corruption/oversized SQL refusal and resource admission. Exercise the shipped CPU-only CLI after reopening an authored CUDA capture, comparing descriptors to its original completed report and confirming stored original bytes remain unchanged.

Original-byte download/export, listing/retention controls and desktop presentation remain separate product work. Retention eviction may make a historical epoch NotFound. This query does not establish model quality, platform/device qualification or release readiness.

Verification: 1,036 serial workspace tests, 552 serial CUDA Engine tests, the native CPU-only CLI scenario, default build and 266 structural checks pass. See `docs/PULSAR_RETAINED_LIVE_CUDA_INSPECTION.md` for scope and remaining gates.
