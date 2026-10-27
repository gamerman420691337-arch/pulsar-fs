# ADR B3-192: Resolve selected gaps as authored values proposals

Construct explicitly selected gap motion through the existing values-only broker
and shared span reconciliation, preserving original knots and missing-source
provenance. Directly deleting gap markers would hide that the new interpolation
is authored; separate proposal, review and commit retain that distinction without
creating a second authority or review-disposition path.

See [contract and evidence](../../docs/PULSAR_GAP_RESOLUTION.md).
