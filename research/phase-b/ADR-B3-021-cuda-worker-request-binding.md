# ADR B3-021: bind CUDA worker requests to owner policy and captured image

Status: implemented transport contract; production GPU admission disabled

## Problem

An explicit CUDA profile names runtime and resource intent, but the old worker
request cannot retain the engine's owner-policy identity, selected kernel device
coordinate or captured native-library image. Ambient device/library discovery
cannot supply an immutable attempt binding.

## Decision

Add a strict optional execution declaration to the shared worker request. Require
exact profile/operation model, runtime and input agreement, exact owner/library
identities in the dependency list, bounded images and unique explicit aliases.
Keep kernel minor separate from logical ordinal. Construct engine declarations
only from the loaded private policy and its matching verified capture.

Omit absent declarations to preserve CPU bytes. Keep production GPU admission
disabled and defer persistence to an explicit database reader/writer barrier.
The declaration itself grants no device or filesystem authority. Captured bytes,
fresh resource admission, held/mounted device checks, worker lifetime, SDK checks
and result provenance remain separate effectful obligations.

## Evidence and limits

See [the contract](../../docs/PULSAR_CUDA_WORKER_CONTRACT.md) and its validation
packet for exact tested subjects. The compiled preceding reader is retained
before rebuilding the current worker. Authored metadata checks distinguish
old-reader parse refusal, current structural refusal and the current final
unsupported-provider guard; none is GPU computation or durable job admission.

Independent model/media, Handy, platform, human and release gates remain open.
