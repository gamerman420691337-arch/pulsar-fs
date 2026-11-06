# ADR B3-197: Persist original browser gap uploads without restoring authority

Extend the private recovery store with locked pre-effect checkpoints. Bind each
saved recipe and upload identity to project/session credentials, validate values
and admitted lease scope, and keep the workflow lock through Engine effects.
Store original transfer phase before its next dispatch; unknown outcomes resume
only the saved Begin/lease/Finish sequence. Read back confirmed candidate values
and evidence from the authenticated Engine before acknowledging restored staging.

Do not save or restore review/commit approval. Recovered proposals require fresh
diagnostics and explicit commitment. Original recording/commit/export recovery,
managed release and broader durability/platform qualification remain obligations.

See [contract and evidence](../../docs/PULSAR_BROWSER_GAP_JOURNAL.md).
