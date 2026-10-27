# ADR B3-024: retain original CUDA evidence through parent egress

Status: implemented evidence transport; production CUDA admission disabled

## Problem

The worker's bulk stream retained normal output but not the original CUDA node
trace. A generation receipt's trace digest alone could not let the parent
reparse that trace or independently check its request/profile binding.

## Decision

Extend the private worker response with an optional bounded CUDA evidence pair.
Omit it for CPU responses. Finalized request-bound adapters publish a dedicated
receipt and original trace reference. Include both in fixed, aggregate-bounded
egress; rebase to parent-owned no-clobber files and verify bytes, request identity,
SDK coordinates and strict trace consistency before returning a launch result.
Failed attempts and CPU responses cannot carry this declaration.

## Evidence and limits

See [the result contract](../../docs/PULSAR_CUDA_RESULT_EGRESS.md) and source-bound
packet. Authored RGB CUDA computation supplies the actual original trace to the
shared parent transport/verifier. Adversarial declarations and changed bindings
refuse. This is an evidence-consistency seam, not authenticated execution,
durable engine admission, generated motion quality or release qualification.

Production mounted custody, launch, durable acceptance, schema barriers,
cancellation/restart fencing and independent model/media/device/platform/human
qualification remain required.
