# ADR B3-172 — selected neutral-axis publication

Status: implemented with scoped verification; roadmap PARTIAL.

The atomic publisher previously required all six master axes, preventing stroke-only motion from using the same retained publication path. Accept an explicit, checked selection; retain six axes by default. Capture the selection and original request IDs in the private stage, with schema 2 for subsets and schema 1 for full selections. Preserve previously valid schema-1 request ordering. Resume uses the original stage rather than current UI selections.

Check all selected tracks and existing members before dispatch, then retain final byte/revision verification and no-replace atomic publication. Never fabricate missing axes or infer physical motion from a selected export. Existing audio-library workflow keeps its six-axis contract until a separate retained model workflow is implemented.

Explicit selected-axis publication now supports one through six committed neutral tracks through CLI and desktop shared controls. Full six-axis schema 1 is preserved; subset schema 2 retains original named request identities and is refused by the predecessor client. Master/member preflight runs before export dispatch, and final strict-byte/revision verification and no-replace rename remain authoritative. Client tests pass 380 with ten ignored; both strict lint configurations, Windows source check, native build, formatting and architecture/source guards pass. Native client-death/restart recovery, older-reader refusal, missing/duplicate/schema/extra-member refusals and one-/two-axis byte comparisons pass. The initial compatibility-test and native preflight failures are retained and corrected. Rendered GUI acceptance, retained visual/model batch workflow and independent model/media/platform/device/human/release gates remain open.

See [contract](../../docs/PULSAR_SELECTED_AXIS_PUBLICATION.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-12/selected-axis-publication/checkpoint.json).
