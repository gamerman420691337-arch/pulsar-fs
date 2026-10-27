# ADR B5-011: Bounded native encoded-source role

Status: implemented private worker primitive; authenticated acquisition integration open.

## Context

The timestamp-bound native decoder can consume an unknown-length pipe, but live leases currently accept only already-decoded RGB. Source acquisition needs a private native worker boundary with explicit format, complete identity, bounded intake/frames/lifetime and independently checkable pixel/clock output.

## Decision

Add a strict engine/worker source manifest and readiness/frame/completion/error replies. Admit only explicit YUV4MPEG2, NUT or Matroska demuxers; encoded input arrives through one owned unbuffered stdin descriptor. Bound bytes, frame count, lifetime, CPU and per-frame output. Verify admitted FFmpeg bytes before executing the timestamped decoder and retain the complete intake SHA-256 only after successful EOF and native termination.

Keep worker-relative delivery distinct from hardware capture, source PTS and engine lease time. Emit ready before forwarding; bind every frame to the native attempt/capture identity and exact RGB hash. Parent stateful transport/sequence/clock/authority checks remain required. Cancel/reap the owned native consumer before joining a potentially blocked intake writer. Poll idle input with an absolute deadline. Do not retry or rearm on partial output/failure.

An environment marker refuses accidental unconfined role invocation but does not prove isolation. The engine must still snapshot worker/tool bytes, reserve aggregate resources, establish native confinement, own independent worker/cgroup supervision and publish only after fresh lease checks. Do not turn this private role into an authenticated camera/source capability until that integration exists.

## Evidence and successors

See [native source worker](../../docs/PULSAR_NATIVE_SOURCE_WORKER.md). Native authored format/intake/pixel/time tests and live pipe output before EOF are separate from the compiled CLI's role refusal and the existing confined worker regressions. Engine source launch, authenticated encoded stream transport, age-basis/clock correspondence, acquisition provenance, continuity and camera/media/model/GPU/Handy/physical/platform/human/release qualification remain required. Training and sterile evaluation were unchanged.
