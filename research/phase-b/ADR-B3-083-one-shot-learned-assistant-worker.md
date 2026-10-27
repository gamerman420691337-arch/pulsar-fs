# ADR B3-083: one-shot learned assistant worker

Status: compiled data-only role and bounded Linux learned-inference probes implemented; authenticated application delivery and model selection remain pending. Full roadmap INCOMPLETE.

## Decision

Run learned inference only in the existing dedicated worker process composition. Use fixed guest runtime/model paths, declared artifact hashes checked against actual mounted bytes, one bounded strict JSON request and original version/attempt/artifact-bound proposal response. User input remains file data. Offer context-issued IDs/settings in a grammar while retaining independent whole-body/context validation; never use log/substring recovery or promote model text into authority/evidence/device actions.

Keep one deadline through hash verification, native launch, bounded nonblocking egress and completion. Refuse malformed/oversize/unknown requests, bad artifact identity, extra role arguments, unconfined entry and late/partial output. Kill/poll the direct native child on failure; require the future Engine parent to retain whole-tree resource/staging/fence custody independently. Namespace probes prove their actual execution subject, not that missing parent.

## Compatibility and evidence

No Command/Response variant, framing version, schema or dependency changes. Existing protocol data types gain serialized scope plus additive worker request/result types. No public effect handler, client-native import or installed Python runtime requirement. Pinned Qwen3-0.6B Q8_0 scores 5/6 with open IDs, 6/6 with supplied-ID grammar, and 6/6 through the actual compiled Linux role; six initial and seven follow-up refusal checks (including distinct hash/native deadlines) and explicit real-model Rust execution pass. Tiny synthetic probes do not select a release default or close EDIT-01/TEST-23. Complete all-feature engine 725/80, protocol 112, both workspace lint configurations, formatting, architecture 354 and guards 36 pass. Wire rerun scope is retained separately. See [implementation, limits and source-bound evidence](../../docs/PULSAR_LOCAL_ASSISTANT_WORKER.md).
