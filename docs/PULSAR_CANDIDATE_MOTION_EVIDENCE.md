# Candidate motion evidence presentation

Status: scoped client verification PASS; roadmap PARTIAL; unqualified.

Candidate review shows a breakdown of action evidence on each supplied axis: Observed, Predicted, Inferred and Synthesized. Unresolved gaps have their own interval count, including tracks with gaps and no actions. Missing axes are omitted. Counts describe labels on discrete actions and gap intervals; they do not measure duration, accuracy or model qualification.

A visible notice identifies synthesized actions as insufficient evidence of observed temporal motion. Another notice identifies unresolved gaps that require explicit resolution before standard export. The existing review flags, stale-candidate controls and commit flow continue to operate independently of these summaries.

The desktop computes the summary from the exact hydrated candidate MotionProgram. It caches only six-axis summary rows and a weak program reference. Reusing the same immutable program avoids scanning actions each frame; selecting a different program replaces the rows. The cache cannot keep discarded candidate motion alive. This adds no engine authority, protocol fields, persistent records or output metadata.

## Verification

The client library passes 258 tests with 5 ignored (6.52 seconds). Both default and all-feature strict workspace/all-target lint checks pass. The final default application/worker build and workspace format check pass. Architecture checks pass 406 assertions, and source guards pass 36 tests. The new regressions cover mixed labels, gap-only axes, absent axes, same-program cache reuse, changed-program invalidation and disposal of discarded motion. Interactive rendered GUI qualification remains UNKNOWN. Exact source, logs, build results and identities are retained in [the checkpoint](evidence/roadmap-2026-10-11/candidate-motion-evidence/checkpoint.json).

## Remaining work

This presentation cannot identify a retained image-conditioned approval or distinguish an authored fixture from a learned proposal. Those identities require a verified origin report. The dedicated [candidate image-origin inspection](PULSAR_CANDIDATE_IMAGE_ORIGIN.md) now verifies and exposes retained approval metadata. Retained normalized-pixel preview, richer proposal review and neutral export provenance remain open. Previous useful image-to-pattern controls remain FAIL. Source-matched training annotations are absent; the owner explicitly instructed continued software work. No annotation, training or qualification result is invented.
