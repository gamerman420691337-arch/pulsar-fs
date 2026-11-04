# CUDA worker request contract

The optional `WorkerRequest.cuda_execution` declaration binds an explicit CUDA
profile to the loaded owner-policy byte identity, expected device UUID, PCI
address, kernel minor, logical ordinal and a captured library directory. Each
library carries its artifact identity and explicit loader aliases. Policy and
library identities must also occur in the request's dependency list. The runtime
alias must identify the profile's exact runtime, and the operation's model,
runtime and input contract must agree with the profile.

Validation rejects unknown fields, unsupported versions, missing roles,
conflicting aliases, duplicate library identities, traversal, malformed device
coordinates, oversized libraries/images and incomplete dependencies. CUDA
profiles require this declaration. CPU and non-model requests reject it. Absent
declarations are omitted, preserving existing CPU wire serialization.

`LoadedCudaOwnerPolicy::worker_execution` constructs the declaration from its
immutable loaded policy and a matching, verified `CapturedNativeDependencies`.
It checks the owner priority ceiling before construction. Original supplier
path replacements do not change captured bytes; altered captures and captures
from a different specification refuse. The returned metadata does not extend
the capture's lifetime: the launch owner must retain it until worker retirement.

This is a transport contract, not a completed durable GPU attempt. No database
schema or portable format changes, and production CUDA admission remains
disabled. A preceding worker refuses CUDA-bearing JSON before executing it.
The current worker accepts structurally valid metadata, then refuses the
unimplemented production provider. Neither behavior loads CUDA libraries.

Before enabling GPU jobs, implement supervised image capture and aggregate
admission, held device binding and guest checks, the confined Rust CUDA session,
result/trace binding to the admitted attempt, cancellation and restart fencing,
and a database compatibility barrier for persisted GPU manifests. Do not store
new CUDA attempts in a database that older writers still accept.

The retained [validation packet](evidence/roadmap-2026-10-01/cuda-worker-contract/validation.json)
records the tested source and binary subjects. Authored transport metadata and
inert library captures establish neither model quality nor physical-device,
real-media, platform, performance or release qualification.
