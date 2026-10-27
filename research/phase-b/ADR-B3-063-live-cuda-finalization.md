# ADR B3-063: Consume the owned CUDA worker before completed publication

Status: Implemented and locally verified private Engine path; evidence in the linked checkpoint. Public GPU lifecycle remains pending.

Use the capture's inference phase as the single closing state rather than a second boolean. Retain its runtime and cancellation supervisor while refusing new ingress, dispatch and continuation. Consume the actual scope-bound worker through Finish outside database/registry locks, then reserve storage and repeat current authorization, revision, lease, fence and closing membership inside the publication transaction. Do not grant current authority to admitted database envelopes.

Commit the existing schema21 original-byte record before returning a completed report. Link that report to canonical manifest and original receipt/trace identities with the checked summary and `qualified: false`. Recheck the original deadline after blocking commit; crossed-deadline rows are historical diagnostics and cannot cause Completed publication. Keep failure retirement independent of database health and preserve completed reports through idempotent Stop.

Ordinary CPU reports omit all new optional evidence fields. Earlier compiled readers must refuse the new Finishing/Completed variants. The private native test establishes this path only through authored capture input/test registry insertion. Public scheduling and qualification require separate evidence.

See [implementation, verification and limits](../../docs/PULSAR_LIVE_CUDA_FINALIZATION.md). No model, real-media, Handy, platform, human or release acceptance changes.
