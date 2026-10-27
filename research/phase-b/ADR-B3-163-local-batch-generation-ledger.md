# ADR B3-163: Engine-owned local batch input and generation phases

Date: 2026-10-12. Status: implemented software contract; qualification open.

Local folders receive their own bounded Engine-owned ledger rather than remote Stash identities. Freeze selected source bytes and a checked audio recipe, retain exact phase request IDs before effects, and require current authenticated authority for explicit advancement. Commit phase acknowledgements and outer mutation replies atomically. Retain unresolved outer identity across restart so acknowledgement reconciliation cannot advance a different phase. Bind captured-source import to digest/length before publishing the source association.

Wrap preceding database reader layers under schema 28, preserving their publication/worker/annotation requirements. Retain source membership for eviction protection. Read/list scopes are actor-bound and dispatch no generation. No bearer token enters the record and restart performs no automatic advancement. Keep generation readiness separate from publication and qualification. See [scope](../../docs/PULSAR_LOCAL_BATCH_GENERATION.md). Full roadmap PARTIAL.

Actual validation required preserving the outer barrier during first worker setup and binding benchmark declarations to the profile actually resolved by the manifest. These defects and their corrected tests/native receipts are retained with the checkpoint; they do not confer neural, physical or release qualification.
