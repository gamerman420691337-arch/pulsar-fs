# ADR B2-009: retained annotated model closure and schema-3 clocks

Status: implemented locally; public annotated generation remains guarded.

## Context

Portable annotated results must retain the model and source-clock facts needed to interpret them after original paths disappear. Existing readers must not accept or erase facts they cannot validate. CUDA and local annotated reader requirements already own conditional database layers.

## Decision

Use conditional portable format 11 with exact annotated-model membership and a required model payload role. Reuse bounded request/receipt/model verification through retained-object readers. Accept schema-3 clocks only for annotated requests and validate mapping, trim, stride, quantization and boundaries. Keep historical paths inert and imported attempts non-executable.

Wrap preserved schema 18 through 23 in an exact schema-24 reader table. Arm it transactionally on import/completed export and preserve underlying activation requirements. Validate metadata at initial read-only startup and retained bytes before writable SQLite recovery. Retained-byte refusal does not currently guarantee unchanged earlier custody/filesystem preparation.

## Evidence and remaining obligations

[Portable annotated generation](../../docs/PULSAR_ANNOTATED_PORTABLE_GENERATION.md) records the authored native worker and actual Engine clone/restart/re-export campaign, clock corruption checks, conditional storage rollback/coexistence and preceding-reader refusal. Initial container construction is authored; active Engine capture and public clients remain to be verified. Natural-media alignment, model quality, broad fault recovery, physical device, platform and human/release qualification remain separate gates.
