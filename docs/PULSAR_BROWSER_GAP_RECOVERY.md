# Browser original edit-upload recovery

Gap preparation now retains separate original Begin and Finish request identities
and the exact values-byte length/digest before dispatch. The shared Client motion
adapter retains the admitted lease, obtains authenticated accepted-prefix status,
and resumes only remaining bytes. Endpoint, Engine boot, scope, direction,
byte range, digest, expiry and progress are checked before opening bulk. It marks
finalization attempted before dispatch and replays the original Finish directly
on retry, including when hydration or the acknowledgement was lost. This uses the
Engine's existing durable edit-upload receipt; it creates no replacement candidate.
Changed bytes and invalid progress refuse without bulk/finalization. Errors preserve
original transfer state instead of silently abandoning it.

Use **Stage or resume prepared gap proposal** after an unconfirmed upload outcome.
**Inspect gap proposal intent** remains read-only. Explicit candidate diagnostics
and commit are still required. All repairs preserve synthesized evidence; this
recovery does not grant commit or device authority.

Subsequent [durable gap journals](PULSAR_BROWSER_GAP_JOURNAL.md) preserve prepared
uploads across browser server loss. Current review is not restored. [Original approved gap commits](PULSAR_BROWSER_GAP_COMMIT_RECOVERY.md) can now be explicitly reconciled; the journal grants no new authority.
Expired/invalidated unfinished leases refuse; no fresh Begin identity is invented.
Existing ordinary one-shot editor uploads remain unchanged.

## Validation scope

A network-disabled native fixture proxy drops two actual successful Engine replies:
the original Begin and the original Finish. Sanitized proxy receipts confirm both
Begin calls have the same request/lease IDs and both Finish calls have the same
request/candidate IDs. After the lost Finish, SQL already has exactly the original
recording candidate and one gap candidate; recovery creates no third candidate.
The campaign preserves 492 original inferred knots across three handoffs, adds five
synthesized gap knots, explicitly reviews/commits, and compares all 497 exported
actions with the independent scalar oracle and browser download. Two candidates,
two exports and no ordinary jobs remain. Final custody is empty; owned server,
Engine and proxy are joined. No new Engine-restart-after-lost-Finish or real partial
chunk interruption campaign is claimed; accepted bytes followed by a bad chunk
acknowledgement are covered by targeted Client mocks. The initial proxy treated an empty startup probe as malformed and
failed before fault injection; its log/harness are retained, and the corrected
same-source/binary campaign passes.

Targeted fault tests cover lost Begin acknowledgement, accepted bytes with a bad
chunk acknowledgement, lost Finish acknowledgement, changed values/digest and
invalid accepted progress. Original identities remain stable, accepted bytes are
not duplicated, and no implicit abandonment or commit occurs. Full Clients pass
418 tests with ten ignored; twelve mock-player cases and ten validation commands
pass, including both strict Clippy configurations, formatting, architecture/guards,
Windows source compilation and rebuilt native App/Worker. Core/Protocol/Engine
sources are unchanged; no new full suites are claimed for them.

Rendered in-app browser QA records 156 estimates across two captures, stages and
commits them, stages/repeats the confirmed repair proposal, reviews/commits three
authored repairs and downloads 159 strictly ordered neutral actions at revision
two. All positions equal the independent diagnostic scalar oracle (38 percent),
with endpoints 0 and 60000 ms. Exact downloaded bytes, screenshot and full
accessibility state are retained. No fault was injected in this rendered run.

This is diagnostic software evidence. Useful learned motion, arbitrary-tab capture,
real-media/model quality, broader concurrency/platform/CUDA, physical Handy
synchronization and human/release acceptance remain open. Full roadmap PARTIAL.

See [checkpoint](evidence/roadmap-2026-10-13/browser-gap-recovery/checkpoint.json) and
[ADR B3-196](../research/phase-b/ADR-B3-196-browser-gap-recovery.md).
