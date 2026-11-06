# Portable authored retiming facts

Portable project format 5 adds an optional, revision-ordered `retiming_receipts` collection and its explicit record count. Export captures each local receipt through the existing independent request/actor/revision/placement bindings and complete motion recomputation. A corrupted local receipt prevents package capture. Older formats remain byte-compatible when this collection and format-5 ancestry are absent.

The strict manifest requires each receipt to belong to its original project namespace, match its exact before/after motion identities, actor, edit kind and parent lineage, retain both complete placement definitions, and include the target in that revision's active placement membership. Duplicate or unordered revisions refuse. Receipt records count toward the existing aggregate metadata/entry limits. Missing legacy receipts remain missing; a historical description does not reconstruct intent.

Import verifies the package and object bytes through the existing bounded staging workflow. It additionally reads the exact referenced before/after programs under per-receipt memory admission and recomputes the declared retime. A structurally valid receipt with axes that cannot produce the recorded motion refuses before project publication. Package hashes establish content integrity, not independent human identity or nonrepudiation.

## Clone namespaces

Clone import gives the destination fresh project/candidate/placement identities and archival actors. Original receipts remain unchanged in the digest-bound original manifest, with the existing complete namespace mappings to the destination history. They are not rewritten into destination-authored edit receipts, sessions or request acknowledgments. Destination-local receipt records cannot shadow a revision already covered by imported ancestry.

Re-export retains those original manifest bytes and object closure. Successive clones retain the original receipt facts and their origin chain. Format 5 propagates when any retained origin uses it, so a format-4 reader refuses at the explicit header boundary instead of accepting an incomplete closure.

The existing `project retime-status` command continues to report only locally authenticated receipts. Imported historical revisions therefore remain NotFound through that command. The separate [archival status reader](PULSAR_ARCHIVAL_RETIMING_STATUS.md) now verifies their preserved origin facts and destination correspondence. Executable clock-proof composition across namespaces and Stash capture/source/proof association migration remain unfinished. This archive change does not install grants, replay original requests, run historical worker manifests, change the master or qualify media/device behavior.

## Evidence

The authenticated engine test verifies captured facts, count/version, forged actor and duplicate refusal. Import evidence validation separately rejects a structurally valid wrong-axis receipt by recomputation. The compiled native fixture exports an authored reverse retime, checks exact original facts, exercises the pinned format-4 reader refusal, performs two successive clone/import/re-export cycles and checks the preserved original manifest digest and receipts with unchanged motion. Destination local receipt lookup remains NotFound.

Database schema remains 6; timing proof schemas remain 1/2. Native fixtures use authored motion and an inert, never-decoded source. Real-media alignment, model quality, GUI usability, platform/device release qualification and full roadmap completion remain unproven. Exact identities and raw results are recorded in [this increment's receipt](evidence/roadmap-2026-10-01/portable-retiming-receipts/validation.json).
