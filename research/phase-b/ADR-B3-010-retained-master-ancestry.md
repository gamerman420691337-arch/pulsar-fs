# ADR B3-010: Verify current master ancestry through archival namespaces

Decision: extend the existing bounded Stash lineage walker with verified archival links. Resolve retained authored bases through explicit revision-map composition, and check imported SQL lineage/motion references against the immutable projection before following them. Use archival retiming recomputation when the effective step is imported; preserve native receipt verification for current steps.

Historical generation leaves must match checked original anchors. No imported job, actor or request is activated. Different/live generation clock combinations and unknown provenance refuse until explicit correspondence is implemented. Rebased current candidates check both inherited and base clock histories.

Expose this reusable verification through an optional current-master revision in generation inspection. Its bounded response is a fact report without publication authority. Default command serialization and historical-only responses remain unchanged when the option is absent. Database schema 9 / portable format 8 stay unchanged; no publication effect or stored receipt format is introduced.

Current-source/destination authorization and clock-proof migration remain separate required publication work. See [contract and source-bound local evidence](../../docs/PULSAR_ARCHIVED_MASTER_ANCESTRY.md). Model/media, device, platform and human/release gates remain open.
