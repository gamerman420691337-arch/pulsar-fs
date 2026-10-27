# B6-001: revision-bound review decisions

Status: implemented locally; broader GB-13 qualification remains open.

Original flags are immutable evidence snapshots. Explicit resolve/reopen declarations are separate, bounded decision records embedded in revision review state. The authenticated engine supplies attribution and commits the decision, motion-preserving revision, history membership, placement membership, event and replay receipt in one transaction. Clients supply a warning identity, rationale and expected revision; none of those values grants authority.

Whole-flag identities hash the canonical validated flag. Repeated identical flags share one identity. Decisions are bound to consecutive recorded revisions, retained evidence and the revision actor. Earlier revisions retain the old decision. No warning removal, evidence fabrication, confidence calibration or physical admission follows from a disposition.

Protection carries decisions; undo/redo restore their history snapshots. Motion mutation, candidate replacement/merge/rebase and alignment mutation invalidate acceptance conservatively. Existing gaps and unknown ancestry remain enforced independently. A candidate cannot inherit accepted review merely because program or warning bytes compare equal.

Database schema 5 prevents older readers from discarding these semantics during mutation. Portable format 4 preserves declarations as archival evidence with explicit actor namespace correspondence, including through transitive clones. Imported declarations do not become local review authority. Original archive bytes remain retained; rewriting rationale or attribution violates correspondence. Formats 1–3 omit empty decision fields and preserve legacy bytes.

See [the client and persistence contract](../../docs/PULSAR_REVIEW_DECISIONS.md). Partial-interval decisions, human identity/inspection evidence, GUI usability, review-effort measurements and broader recovery/release gates remain separate work.
