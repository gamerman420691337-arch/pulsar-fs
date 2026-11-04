# ADR B3-154: folder consumers share explicit file-generation declarations

Date: 2026-10-12. Status: implemented software slice, qualification open.

Explicit model generation already exists for individual sources; folder consumers must use that authority rather than refusing all models or guessing their ABI. Share one folder declaration loader and the existing checked file-generation helper across batch and scan --generate. Require class count or an exact declared inference profile, validate settings before discovery/effects, and retain source-frame timing, project/revision, candidate commit and export checks.

Preserve companion skipping and dry-run JSON syntax. Read-only scans reject generation controls; multi-axis and overwrite remain refused pending their actual contracts. Engine validation remains authoritative for bytes, runtime/model pins and native execution. Source-specific annotation profiles do not acquire fabricated annotations.

Evidence and limits: [folder model generation](../../docs/PULSAR_LIBRARY_MODEL_GENERATION.md). Full roadmap remains PARTIAL.
