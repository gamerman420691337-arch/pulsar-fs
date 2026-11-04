# Explicit review decisions

Pulsar retains original warning evidence when an actor declares its interval reviewed. This declaration does not change motion, fill an unavailable gap, remove protection, establish missing ancestry, calibrate confidence, or qualify a model, script or physical device. An authenticated software session identifies the actor; it does not independently establish that a human inspected the interval.

```sh
pulsar review-status PROJECT
pulsar review-disposition PROJECT REVIEW_ID --disposition resolved \
  --note 'Inspected this interval and accepted the motion' --revision REVISION \
  --request-id MY_REVIEW_REQUEST
pulsar review-disposition PROJECT REVIEW_ID --disposition reopened \
  --note 'Reopened for another inspection' --revision NEW_REVISION
```

Status exposes the complete original flag and its identity. The SHA-256 identity binds axis, entire interval, reason, evidence and confidence. Identical duplicate flags share one review identity. Omit interval flags to review the whole warning. Add both `--start-ns START --end-ns END` to resolve or reopen an exact nonempty half-open interval contained in the original warning. The latest declaration wins only inside its selected interval. Diagnostics retain warnings in every unresolved residual interval. Status requires Read; disposition requires Edit and the exact current revision. A nonempty rationale of at most 256 bytes is required. No client-supplied actor is accepted.

For whole-only declarations, status retains the existing `decision` field. Once interval declarations exist, `decision` is null and `decision_history` carries the chronological whole baseline, if present, and interval overrides. A new whole declaration replaces that warning's current baseline and overrides; earlier revision snapshots remain retained. Private interval receipts bind the recorded range, actor, request, revision, disposition, rationale and timestamp independently of the revision snapshot. This detects changed snapshot intent; it is not an external signature or proof of human inspection.

Each decision creates a new revision with unchanged motion and protection. Its snapshot records the authenticated actor, request ID, previous/new revisions, disposition, rationale and timestamp. Earlier snapshots retain earlier decisions and every original warning. Exact replay under current authority returns the original result without adding a revision; altered intent under that ID refuses. A stale new request refuses. The CLI prints the request ID before sending it.

Protection-only revisions carry current decisions. Undo/redo restore the target history snapshot, including its decisions. Motion edits, candidate commit/merge/rebase and source-alignment changes require fresh review and do not inherit acceptance, including when warning bytes happen to match. Current diagnostics and Stash export unresolved-warning counts use the declared dispositions. Unresolved motion gaps and unknown ancestry remain independently visible and enforced.

Desktop offers whole-warning and selected-subinterval resolve/reopen controls on current committed-project warnings, shows residual intervals and declaration history, requires a rationale, and rejects stale status. It retains the exact command, revision and request ID before queueing, with an exact-retry control. Uncommitted candidates cannot be marked accepted through this workflow. These controls compile; interactive human usability remains a separate gate.

Database schema 7 adds transactional interval intent receipts and refuses older readers before writable startup. Existing supported schemas migrate while retaining their records. Portable format 6 is required when interval declarations or format-6 ancestors exist. Whole-only decisions retain the format-4 boundary, and retiming facts retain the format-5 boundary. Formats 1–3 retain their original serialization when no newer fact is present. Clone import remaps archival actors through explicit namespace maps and retains exact original manifests. Archival decisions remain declarations and are not applied as local acceptance. Private native receipts are not fabricated for imported actors. A new local decision after import can address the retained warning, while unknown original ancestry remains unknown. Successive re-export/import retains that distinction.

Native synthetic checks cover authenticated decisions, unchanged motion/protection, replay/restart, reopening, undo/redo, stale revision and absent evidence, denied Edit authority, gap-export refusal, alignment invalidation, rebase invalidation, original warning retention, portable actor mapping and transitive clone/re-export. Altered archival rationale and actor attribution refuse. The exact retained schema-4 reader is separately configured for database and format-4 refusal. These are local software checks, not a human review pilot or real-media qualification.

The [interval-review checkpoint](evidence/roadmap-2026-10-01/interval-review/validation.json) adds native exact-range resolution/reopening, residual warning boundaries, changed-range replay refusal, undo/redo/restart, receipt failure rollback, changed snapshot-range refusal, schema-6 and format-5 reader refusal, and successive clone/re-export checks. GB-13 remains open for human GUI/review-effort evidence and the remaining product recovery/partial-result scenarios. The full specification and roadmap remain unchanged.
