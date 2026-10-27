# ADR B2-003: Native project-time interval regeneration

Status: implemented locally; qualification remains open.

Manual target seeds could only rerun whole-source generation. A user needs to reanalyse a selected project interval and apply a new target choice without replacing outside or protected motion.

Add an optional half-open project interval to immutable generation settings. Select original sampled source ordinals after exact placement/time conversion, preserving the global stride and original clocks. Initialize fresh perception context at the selected source start; no preceding target identity is implicitly adopted. Retain explicit unavailable interval boundaries and the full selection plan. Source decoding still validates the container, so no random-access or speed claim follows.

Expose native scope through the existing candidate snapshot using bounded same-project native job metadata. Require explicit contained merge ranges; refuse whole replacement of an existing master. CLI regeneration and compiled desktop candidate application use retained scope rather than mutable form state. Imported job declarations stay archival and do not acquire native execution authority; clone-aware active scope interpretation remains a separate open implementation item.

Advance the database compatibility barrier to version 8 so schema-7 engines cannot silently ignore native scope. Optional absent protocol fields preserve legacy payloads. Portable format remains 6 because original manifest/receipt objects already retain settings and strict older readers refuse unsupported origin settings; no new native execution authority is added to clones. Independent historical-reader refusal, decoder/lattice and native client tests establish only their local scopes.

See ../../docs/PULSAR_INTERVAL_REGENERATION.md. This does not qualify model identity/quality, physical Handy behavior, rendered usability or the full roadmap.

B2-004 supersedes the portable format / clone-scope deferral with independent restrictions, schema 9 and format 7. Original native selection and clock contracts remain in force. See [B2-004](ADR-B2-004-portable-candidate-interval-restrictions.md).
