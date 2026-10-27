# ADR B3-150: Validate native completions against the offered creative palette

Status: implemented; model and release qualification remain open.

Use one sampling-aware palette to derive the native creative grammar and independently validate decoded parameters. Keep existing Core and scope checks first. Reject unoffered numerical values even when a runtime ignores its grammar. Do not equate a numerically valid proposal with faithful understanding of user intent.

Measured expanded and compact host-context presentations regress the fixed learned comparison, so retain the original production context. Preserve negative controls and raw unsuccessful model responses. Keep explicit generation approval and independent release gates.

See [scope and evidence](../../docs/PULSAR_NATIVE_CREATIVE_CAPABILITIES.md).
