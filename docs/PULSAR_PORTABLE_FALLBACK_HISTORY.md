# Portable fallback history

The portable fallback work is in progress under `LIVE-FALLBACK-HISTORY` in
[the execution dashboard](PULSAR_MILESTONES.md). Existing project-package
refusals remain enabled. This document describes the implemented shared
metadata contract and the remaining integration requirements; it is not a
claim that fallback projects can already be exported or cloned.

`InferenceFallbackHistory` retains one original job's selected policy and its
complete predecessor-ordered attempt chain. Each attempt contains its original
manifest and profile hashes, state, admission trace, observed environment and
optional worker outcome. Missing outcomes remain unknown. A transition to an
equivalent backend requires the original failed predecessor's recorded
provider-unavailable settlement. Consumed legacy cancelled resume predecessors
remain readable as inert history; they do not become eligible for a new resume.

The shared Protocol validator checks the original project, job, revision and
source namespace, unique membership, initial/current endpoints, predecessor
order, original admission clocks, ordinal progression and profile cycles. It
checks exact selected references and environment identities. With original
worker manifests supplied by the caller, it also checks canonical manifest and
profile digests, policy tags, worker/runtime correspondence and preserved
generation semantics. Validation opens no paths itself. The Engine supplies
only rows from the same checked SQLite transaction as the historical read.

The canonical encoding has schema 1, an explicit `history_only` declaration,
at most 128 attempts, at most 128 KiB per entry and at most 8 MiB in total.
Raw-byte parsing applies the aggregate bound before JSON parsing, rejects
unknown fields, and requires the exact canonical encoding. The existing compact
trace bound of 512 KiB also remains in force for local history reads. These are
metadata admission limits, not an application's total memory reservation.

Neither these checks nor a serialized environment prove that an equivalence
decision is currently accepted or that a worker, grant, model or device is
currently available. The archive contains no native execution or settlement
capability, cancellation token, current acceptance state, secret or renewed
lease. Recorded filesystem paths remain historical values. Captured queued or
running states do not grant imported work permission to execute.

## Remaining integration and exit evidence

1. Capture the checked archive alongside original worker manifests within the
   same project snapshot. Account for aggregate metadata and retained object
   bytes before assembly; keep filesystem reads outside the authority lock.
2. Add a conditional package format for the history and exact object roles.
   Preserve the selected policy, original environments, outcomes, manifests,
   model/runtime declarations and referenced evidence through root and ancestor
   graphs. Reject omitted, substituted, duplicated, detached and contradictory
   current or ancestral declarations, including rehashed mutations.
3. Validate the complete imported object graph before installation. Clone maps
   must preserve original historical identities without installing origin
   acceptance decisions, grants, owner policies or executable attempts. Keep
   active local acceptance and imported historical evidence distinct.
4. Add the conditional persistent reader barrier and compose it with existing
   CPU, live and fallback barriers. An earlier executable must refuse before
   writable recovery or custody changes; record the exact database/WAL and
   filesystem observations rather than inferring them from an error message.
5. Run confined export, clone, restart, re-export and second-clone campaigns
   with completed, failed, cancelled, interrupted and captured-running attempts.
   Verify exact original correspondence, unknown observations, no restored
   authority, bounded admission, corruption refusals and actual resource release.
6. Connect live/browser fallback through its own admitted source and timing
   lifetime. Decoded-file policy does not admit a browser source. Withdrawal,
   expiry, interruption, replacement and restart need actual consumer evidence;
   serialized policy metadata cannot authorize a native successor.

Useful model quality, real-media validation, platform coverage, physical Handy
behavior and human acceptance remain separate gates. No step above closes them
through structural fixtures or package tests.
