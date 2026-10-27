# ADR B3-023: request-bound CUDA model adapters

Status: implemented adapter prerequisite; production CUDA admission disabled

## Problem

The checked native CUDA session previously operated on authored float tensors.
The production model adapters still constructed CPU sessions directly. Native
success alone did not account for RGB preprocessing or decoded output failure.

## Decision

Share a private session layer beneath the existing detector and temporal
adapters. Preserve CPU behavior and select CUDA only from explicit validated
worker intent. Verify declared model/image bytes in the worker, retain the held
SDK/session lifetime and finalize a request-bound trace receipt by consuming the
adapter. Poison CUDA attempts on preprocessing and decoder failures as well as
native failures. Refuse CUDA preview until its trace egress is implemented.

## Evidence and limits

See [the adapter contract](../../docs/PULSAR_CUDA_MODEL_ADAPTERS.md) and its
source-bound packet. The actual request factory is exercised with authored RGB
inputs, a private authored policy and captured native dependencies. Positive
decoded results and six explicit refusal paths preserve raw evidence and verify
terminal cleanup. This establishes neither production admission nor policy
authority, whole-worker confinement, real-model/media quality or qualification.

Durable job/schema barriers, mounted custody, production launch and result
acceptance, trace egress, cancellation/restart fencing and model/media/device/
platform/human release evidence remain separate obligations.
