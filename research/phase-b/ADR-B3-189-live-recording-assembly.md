# ADR B3-189: Assemble accepted live points without erasing missing coverage

Status: Core/Protocol kernel implemented; Engine and product integration open.

Preserve exact source-to-project mapping and original Inferred/CUDA provisional
semantics. Admit only complete stopped recordings with immutable headers and
bounded aggregate points. Continuity requires actual adjacent frame/pixel proof
and an explicit interpolation policy. Treat scope boundaries and all missing
coverage as gaps. Do not sort or silently repair conflicting times.

Keep assembly separate from authority. Engine-owned records will feed durable
candidate provenance, review and explicit commit/export. An unsigned downloaded
archive can be diagnostic input but cannot attest original Engine custody.

See [contract and evidence](../../docs/PULSAR_LIVE_RECORDING_ASSEMBLY.md).
