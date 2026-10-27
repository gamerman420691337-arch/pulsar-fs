# ADR B6-012: Verify catalog metadata before publishing trust state

Status: implemented verifier; owner/storage/client integration incomplete.

Use pinned `tough 0.24.0` for TUF verification rather than treating a detached signature as a complete updater. Keep its HTTP feature disabled and provide only bounded captured bytes through a custom transport. The bootstrap root must be independently approved and exact-hash bound; supplier content cannot bootstrap its own publisher authority.

Seed a private temporary datastore with protected prior role/clock facts, refuse malformed/incomplete state and retain safe expiry enforcement. Verify initial roots from the approved bootstrap and retain the verified current root for subsequent chains; enforce root-version monotonicity and bounded updates/fetches/bytes. Consume authenticated target streams completely, then require exact manifest-name identity and existing strict manifest admission. Admit nondelegated manifest targets only.

Return the complete successor without mutating durable caller state. The forthcoming engine adapter owns fresh authorization, descriptor-relative capture, namespace binding, concurrent serialization and atomic persistent publication. Metadata authentication does not approve native execution, rights, model quality or official release status. Current owner-approved custom selection remains distinct.

Six ephemeral-key real-signature cases cover the primitive. No owner command, durable refresh, native-window, update/revocation or release qualification is claimed. Database schema 13 and portable format 9 stay unchanged. See ../../docs/PULSAR_OFFLINE_CATALOG_VERIFIER.md.

The initial adapter re-applied the original root chain on every refresh. A reproduced signed-fixture failure showed repeated timestamp/snapshot key rotation could reset newer watermarks. Retaining the verified current root fixes that adapter defect; the initial source/full Rust result and failing regression are preserved separately. This is a Pulsar adapter repair, not a claim of an upstream TUF vulnerability.
