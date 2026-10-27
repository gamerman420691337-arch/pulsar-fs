# ADR B6-016: Keep desktop catalog history separate from current approval

Status: desktop integration implemented; source-bound local checks passed. Broader usability/platform/release qualification remains open.

Extend the existing pack panel and authenticated recovery runner to catalog refresh and catalog-bound owner approval. Require explicit independent bootstrap approval, exact namespace/revisions and explicit unqualified execution approval. Treat catalog refresh and selection receipts as historical outcomes; query current recorded status separately. Preserve fresh engine membership/expiry/profile checks at admission. Restored history cannot populate a generation choice.

Persist exact new intent before dispatch under the existing session/credential/owner binding. Atomically raise client journal schema to 2 for catalog intents, preserving older entries. Old clients refuse unknown layouts/intents without dropping records. Exact replay and acknowledged-only dismissal retain original IDs/locators/digests/revisions and never store capability values. Keep the journal barrier after dismissal.

Reuse private storage/admission/locking/sync/rename limits; add no engine schema or portable authority. Disable busy catalog inputs and reset relevant approvals/caches on namespace/bootstrap edits. Display recorded catalog binding and distinguish point-in-time association from execution approval or online revocation freshness.

Headless/native-helper evidence is local software verification. Native window/human usability, non-Unix recovery, update/rollback/cancellation and model/media/device/platform/release qualification remain open. See ../../docs/PULSAR_DESKTOP_CATALOGS.md.
