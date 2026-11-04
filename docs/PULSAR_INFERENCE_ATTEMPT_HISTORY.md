# Inspect inference attempt history

Pulsar retains local completion observations alongside each immutable generation
attempt. **Generator → Inspect attempt history** and the read-only CLI show the
same bounded records:

```sh
pulsar job-attempts --project PROJECT_ID --job JOB_ID
pulsar job-attempts --project PROJECT_ID --job JOB_ID --after-attempt ATTEMPT_ID --limit 8
```

The page requires current project Read authority. It uses lexical attempt-ID
order, not a claimed execution chronology. The default page size is eight and the
maximum is sixteen. A returned cursor identifies the last displayed attempt.
The desktop rejects a response for a different project, job or current attempt.
Reading history creates no generation attempt or durable command receipt.
The desktop button requires an Engine advertising the `job_attempts` capability.

## What a record says

- The immutable attempt-manifest digest and current stored attempt state.
- An explicitly declared inference profile and its typed serialization digest,
  when present. Historical implicit defaults are not reconstructed. A declaration
  does not establish which providers executed model nodes.
- A bounded typed validation diagnostic, retaining the original code, retryability
  and full-message digest. Long messages are explicitly marked truncated.
- An optional observation of confined worker-tree retirement. Missing information
  stays unknown; cancellation, restart and a terminal state do not supply it.
- An optional exact-profile provider-absence observation from the original worker.

Completion observations are independent of candidate publication. A late failed
worker can be recorded after cancellation while the job stays cancelled and no
candidate is published. A dispatch failure before the queued-to-running transition
must end the current job as failed; it does not claim that a worker executed or
retired. Replayed identical observations preserve the original row; conflicting
observations refuse replacement.

## Provider absence and process lifetime

The new provider classification has one narrow origin: after the existing CUDA
device identity checks and pinned ONNX Runtime initialization, the runtime's
provider-list query successfully reports that CUDA is absent. Enumeration errors,
runtime loading errors, provider registration errors, invalid models, failed
inference, dependency mismatches and resource failures retain their existing error
paths. Message text is never used to authorize fallback. The CPU adapter's
availability API always reports true, so this code does not manufacture CPU
absence evidence.

The worker binds the observation to an explicit profile, original job and attempt.
The parent validates it after whole-tree retirement, response-identity checks and
output accounting, followed by the original cancellation and deadline checks.
Host errors, transport failure, unknown cleanup and late responses cannot acquire
the private settled-worker marker. Older worker errors remain unclassified.

## Storage and compatibility

`job_attempt_outcomes` is optional, immutable auxiliary history with exact layout
and bounded record validation. It does not change the existing reader version or
grant execution authority. Legacy attempts without a row remain unknown. Reads
check the original job/attempt membership and exact retained manifest; they do not
open historical worker paths. Startup preflights reject malformed layouts before
writable recovery.

These local observations are not yet included in portable project packages.
Package import continues to restore inert history rather than active jobs or
permissions. The history API does not restore resource ownership, native provider
access, an accepted equivalence or permission to retry.

## Remaining qualified-fallback integration

The production catalog still advertises fallback unavailable and no qualified
profiles. The full requirement remains open:

1. Import equivalence evidence as pending, then resolve separately recorded owner
   acceptance, withdrawal, exact subject/environment scope, requested tier and
   expiry. A model-pack execution approval is not equivalence acceptance.
2. Bind an automatic selection policy to the original job. Explicit backend pins
   stay strict; only an eligible, current, accepted equivalence can be considered.
3. Persist the transition under a reader barrier that older versions cannot erase
   or reinterpret, then recheck authority and reserve fresh resources.
4. Dispatch a distinct attempt/process after predecessor custody settles. Preserve
   source, target, timing and tier semantics; never pass a changed profile through
   ordinary interrupted-checkpoint resume.
5. Disclose the actual executed profile and transition reason, verify cancellation,
   revocation, expiry and restart behavior, and extend portable history explicitly.

Synthetic tests of these software contracts do not accept an equivalence, qualify
a model, establish hardware behavior or complete the roadmap.

## Verification, 2026-10-24

The [verification record](evidence/roadmap-2026-10-24/inference-fallback/verification.json)
retains 1,630 passing Engine tests (101 ignored by the full suite), 655 passing
client tests (14 ignored), 327 passing Protocol library tests, both strict lint
modes, formatting, architecture/source checks and rebuilt app/worker identities.
The queued-host-failure regression first reproduced `queued` instead of `failed`;
its exact failing image is retained. The first full run found two historical
fixtures that mutated an already terminal attempt or only one copy of an immutable
manifest. Those fixtures now use distinct original jobs or consistent records;
production immutability and withdrawal checks remain enforced.

The separately executed [native CPU case](evidence/roadmap-2026-10-24/inference-fallback/engine-private-ram-attempt-history-002/native-evidence/completion.json)
uses the final pinned test/worker images, an authored neutral video and a 302-byte
authored model. Actual CPU inference produces a finite candidate with measured
successful inference calls. An accurately identified malformed model fails in the
real worker; its original error and confirmed worker-tree retirement are retained,
with no provider-absence classification. The master stays unchanged. Both exact
histories survive Engine restart, with two jobs/two attempts, no automatically
restarted work, and joined shutdown of both Engine instances. The supervisor
confirms joined descendants, unchanged subjects and removed scratch.

The CUDA provider-absence branch is compiled and covered by contract/routing tests;
an actual CUDA absence event has not been exercised. The native case neither
selects a fallback nor accepts an equivalence. Native desktop rendering, portable
outcome-history retention and physical/model/release qualification remain open.
