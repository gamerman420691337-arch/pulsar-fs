# ADR B3-134: Image-conditioned generation origin

Status: scoped implementation and checks PASS; roadmap PARTIAL; unqualified.

Preserve the original image-conditioned assistant report and generation approval as bounded, immutable evidence rather than reduce historical provenance to only generated patterns. Retain normalized pixels in the existing Engine snapshot store. Declare a reserved origin identity and its historical dependencies in the existing worker manifest; no new executable worker input kind or grant is introduced. Keep deterministic synthesis distinct from the upstream assistant proposal.

Validate the origin before candidate publication and require the worker lineage to carry its declared dependencies. Portable capture includes actual required origin/RGB bytes, and inert import validates them without following historical host paths. Missing evidence refuses reconstruction. Keep qualification false and observed temporal motion false.

Worker wire shapes remain unchanged. Portable packages add an optional typed image-origin reference to the generated origin, requiring format 13 when present. Both exact record and RGB objects must have the existing DependencyEvidence role and belong to the package closure. Packages without these references retain their existing formats. Known older readers refuse format 13; this alone does not establish the full historical-reader campaign. Historical-reader and full export/import/re-export campaigns remain explicit obligations. Do not claim them from typed local object reads or structural checks.

Implementation and evidence: [origin record](../../docs/PULSAR_IMAGE_GENERATION_ORIGIN.md).
