# ADR-B3-115: checked saved-view training intake

Status: software implemented; native verification recorded separately; qualification open.

The existing desktop author emits a source-bound `AnnotatedGenerationView`, while target-aware staging accepts an aggregate entry-indexed document. Require explicit manifest-ID assignments rather than deriving target labels or silently rebinding geometry. Reuse the existing training-only admission and native view planner, preserve supplied offsets, and require exact source digests/dimensions. Probe an inherited read-only source descriptor and compare source identity and metadata after planning. Existing bounded-process draining now supports optional inherited descriptors; legacy calls retain their default behavior.

Retain exact declaration/manifest bytes, aggregate identity, tool/source hashes and native plans in a new private intake. Publish the whole directory through Linux atomic no-replace rename with descriptor-anchored private staging and durability checks. Refuse output replacement and unsupported publication platforms. Do not delete published output after a durability-reporting error or unowned staging content on cleanup failure.

The bridge is a software tool connecting already authored views to staging, not an annotation generator, timing acceptance mechanism, optimizer or model qualification. Its human target/alignment status stays unverified. See [workflow and evidence](../../docs/PULSAR_SAVED_VIEW_TRAINING_INTAKE.md). Full M1-M8 requirements remain unchanged.
