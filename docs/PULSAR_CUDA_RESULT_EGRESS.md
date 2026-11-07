# Original CUDA evidence egress and parent verification

Worker results can now carry an optional `cuda_artifacts` declaration containing
a bounded dedicated receipt and the original node trace. CPU results omit the
field, preserving their serialized response. CUDA requests were already refused
by the retained preceding worker; this optional response extension does not
enable production CUDA admission or change the database schema.

Finalized request-bound model sessions write a new bounded receipt and retain
the trace identity after rechecking its hash and canonical file path. Per-thread
attempt state transfers these references only for successful worker results;
failure and reset discard them. The fixed bulk stream carries normal output
artifacts, then the CUDA receipt and trace. Aggregate output admission includes
all four artifacts for generation. CPU or failed responses may not attach CUDA
evidence, and CUDA success requires it.

The parent hashes each bounded stream into its own no-clobber, read-only file.
It rebases evidence references to `cuda-inference.json` and `cuda-trace.json` in
the owned attempt directory. It then reads those original bytes, checks their
declared identities, reparses the strict node trace and checks request/profile
hashes, attempt/job identity, selected model/runtime/owner-policy identity, SDK
device coordinates, successful run count and declared trace summary. Unknown
receipt fields, changed bindings, missing evidence, outside-attempt files and
malformed or excessive artifacts refuse.

These are consistency checks. An authored policy, self-reported SDK metadata or
consistent trace does not authenticate native execution or grant engine launch
authority. A refused transfer may leave partial files in its private failed
attempt; no result is accepted from them.

## Evidence and remaining work

The [validation packet](evidence/roadmap-2026-10-01/cuda-result-egress/validation.json)
retains source archives, compiled identities, raw traces and commands. The
default workspace/all-targets suite has 887 passes, zero failures and seven
ignored qualification cases. The CUDA-feature engine suite has 426 passes,
zero failures and one ignored case. All 96 configured JavaScript cases pass
without skips; 220 structural checks, feature/default builds and the diff check
pass. The exact retained preceding-worker CPU refusal response is byte-identical.
The packet binds 170 production and 117 verification paths, 33 unchanged
executable/spec identities, 74 original native-library readbacks and 144 retained
artifact hashes.

The authored CUDA RGB probe now exercises the actual parent egress reader using
authored transport output after real request-bound native computation. The
transport fixture is not a generated motion program or an engine-admitted job.
Both positive adapters retain independently checked parent copies. Negative
checks alter receipt fields, request/profile/job/device/run binding, trace bytes,
parent artifact identity and path, or transport declarations. Refused envelopes
are checked before parent output publication.

Each positive native path exercises nineteen adversarial consistency/envelope
checks. Trace admission remains bounded to eight MiB and 16,384 calls; full
reference-workload trace scheduling and performance are separate open work.

Production CUDA admission remains disabled. Trusted captured-library/device
mounts, durable GPU attempt/schema barriers, confined production launch,
restart/cancellation fencing and durable accepted-evidence membership remain
required. CUDA preview still explicitly refuses pending its complete launch
and result path. Existing launch pipe/deadline and retirement limitations are
not resolved by this artifact reader. Model/media quality, physical Handy,
platform, performance, human acceptance and release gates remain open.
