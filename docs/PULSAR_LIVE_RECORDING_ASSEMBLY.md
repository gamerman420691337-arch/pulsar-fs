# Checked live-recording motion assembly

2026-10-13. Deterministic Core/Protocol assembly implemented. Engine candidate
staging and browser review/commit/export integration remain unfinished.

`assemble_live_recordings` accepts complete stopped, retained recording pages in
explicit order. It checks every page, freezes metadata across pagination, rejects
missing/repeated/regressing pages and scopes, and requires the same original
Engine boot, project, revision, source and profile. Bounds are 256 pages, 64 scopes
and 4096 accepted points. Active, discarded and empty recordings refuse.

Source timestamps map through each original exact project clock and are quantized
once at the final project nanosecond. Points must be distinct, ordered and entirely
inside an explicitly declared range. There is no sorting, clipping, padding or
implicit time repair. All actions retain Inferred evidence; CUDA provisional
presence is explicitly retained in the assembly result. Original accepted-record
proofs must remain associated with the candidate in the next Engine integration.

Interpolation requires matching original adjacent frame IDs, source timestamps
and pixel identities within the same capture, plus an explicit maximum interval
from 1 ns through 1 second. This policy is not measured model accuracy. No edge
crosses a capture boundary. Skipped edges, excessive intervals, leading coverage
and trailing coverage become explicit half-open unavailable gaps; each retained
sample occupies its original project nanosecond. Standard exports must continue
to refuse unresolved gaps under the existing shared export contract.

These pure functions grant no authority. An Engine consumer must obtain records
from its own actor-bound registry, validate current project/revision/permissions,
retain full original provenance durably, and stage an uncommitted candidate through
shared history. Downloaded JSON is not trusted native authority. The read-only
`recording-assembly-fixture` example exists for diagnostics, produces a motion
program JSON, and does not import, commit, export a funscript or send device commands.

Evidence is retained in the [checkpoint](evidence/roadmap-2026-10-13/live-recording-assembly/checkpoint.json).
Initial typed-frame fixture compile errors are preserved along with repaired runs.
Model quality, physical timing, arbitrary tabs, platforms, actual candidate
integration and human/release acceptance remain open. Full roadmap PARTIAL.

## Verification

Full Core/Protocol suites pass 477 tests with zero ignored; default and all-feature
strict workspace Clippy, Windows source check, formatting, architecture and
36 guards pass. No fresh Engine/Client suite or physical-runtime claim is made.
The native read-only harness assembles the actual 152-point browser archive and
four native archives containing 493 accepted estimates. Independent Python
rational arithmetic matches every project time, value, evidence label and gap.
At an explicitly selected 200 ms interpolation limit there are 2 browser gaps
and 5 native gaps, including each capture boundary. At 100 ms, paced native
intervals exceed the limit and correctly retain 493 gaps. Both policies are
retained rather than choosing a threshold as an accuracy claim.

The first native harness range incorrectly ended at 50 seconds despite its last
record mapping to approximately 50.26 seconds; it refused rather than clipping.
The corrected explicit 60-second range preserves the trailing unavailable span.
This replay exercises assembly over previously recorded data; it does not run
new inference or establish native Engine authority from downloaded JSON.
