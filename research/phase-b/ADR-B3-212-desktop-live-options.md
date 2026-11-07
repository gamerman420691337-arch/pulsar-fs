# ADR B3-212 — Checked desktop browser configuration from Generator

Status: Implemented software; qualification PARTIAL
Date: 2026-10-13
Architecture: ARCH159

## Problem

The desktop browser launcher eliminated manual project/revision arguments but still required users to hand-author live-options JSON. The desktop already retains an explicit file-model/profile choice in Generator. Reusing mutable controls at each browser request would silently retarget inference and timing.

## Decision

Capture and display the Generator's explicit file-model/profile identities in Browser live. Author age, buffer, lease and project start with checked millisecond-to-nanosecond conversion. Before worker creation, build and validate the complete typed LiveCaptureOptions; move that immutable value into the same owned server used by JSON mode. No options file, model default or inference admission is invented. The browser supplies source presentation time and source-domain declaration at Start; project start remains an explicit placement declaration.

Capture is explicit and refuses while a worker is owned. Failure preserves the previous captured choice; later Generator edits cannot retarget it. Launch freezes the then-reviewed form. Owner-selected packs are not extracted into raw-file choices; their existing fresh selection/approval contracts remain authoritative. The Engine continues to check exact model/runtime bytes and resource/provider admission before inference.

## Evidence and limits

The pure checks exercise exact unit conversion, malformed/overflow input refusal and captured choice preservation. The native production-panel controller admits a direct configuration through a real Engine, checks its exact policy/lease/project mapping, mutates the discarded form after launch, and verifies unchanged admitted values plus authored scalar inference. Existing file-mode shutdown and ownership tests remain applicable. These are software/diagnostic checks, not rendered desktop, learned motion, real-media, source-player/device timing or platform/human/release acceptance. See [contract](../../docs/PULSAR_DESKTOP_LIVE_OPTIONS.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-13/desktop-live-options/checkpoint.json).
