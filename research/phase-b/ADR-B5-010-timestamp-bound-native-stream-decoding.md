# ADR B5-010: Timestamp-bound native stream decoding

Status: implemented decoder primitive and finite worker integration; live acquisition admission and qualification open.

## Context

The finite path pairs a separate ffprobe frame index with raw RGB decode order. Camera/unknown-length streams cannot depend on a complete upfront frame index. Raw RGB output drops timestamps, and diagnostic text is unsuitable as the authoritative source of pixel/clock correspondence.

## Decision

Decode once and tee each raw RGB packet to separate raw-video and SHA-256 framehash pipes. Parse bounded version-2 packet records, keep exact rational source PTS, independently verify pixel SHA-256 and require one-to-one output counts and successful native termination. Keep source time, decode ordinal and stdout delivery event distinct. Preserve original time base/offset and refuse unresolved timestamp changes instead of silently repairing them.

Use bounded channels and an independent cancellation/absolute-deadline watchdog. Kill/reap the owned child on malformed metadata or terminal error and join owned readers on drop. Caller-owned immutable tool/source admission, resource pool and native confinement remain mandatory. Native EOF, backpressure and delivery do not become physical capture provenance or stopping qualification.

Integrate the adapter into finite generation now, with exact correspondence checks against each selected ffprobe source frame before inference. Retain the index for existing finite sampling/placement behavior; do not let it replace packet verification. Live camera/stream source admission, clock provenance and lease-owned delivery remain successor work. No legacy source/model/training output is tuned or qualified by this increment.

## Evidence

See [native timestamped decoding](../../docs/PULSAR_TIMESTAMPED_NATIVE_DECODE.md). Native authored unknown-length input must yield frames while its producer remains open. Irregular/nonzero timestamps, malformed input, checksum mismatch, truncation, cancellation and expiry are independently exercised. Full Rust, compiled CLI/native JavaScript and structural checks are retained separately from camera/media/model/GPU/Handy/physical/platform/human/release gates.
