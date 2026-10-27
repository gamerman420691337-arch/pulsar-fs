# Durable browser gap-upload intents

The browser now saves a prepared gap recipe, its exact values-byte identity and
original Begin/Finish identities before dispatch. The existing private recovery
store supplies a single-link owner-only journal, session namespace, no-follow
opens, bounded JSON, atomic rename/fsync and a nonblocking workflow lock.
A credential-binding hash and exact project/recipe/upload scope are checked.
No bearer credential, media pixels, review decision or commit approval is stored.

The shared store now supports explicit checkpoints while retaining the workflow
lock. Gap upload records attempted admission, the admitted lease, accepted-prefix
status and the finalization phase before their next effects. Errors retain the
last durably saved phase. Restarted servers can inspect the saved intent and
resume only its original upload; they cannot replace identities or inherit review
and commit state. Confirmed candidates are read back from the authenticated Engine
and compared with the original recipe before staging is acknowledged.

Relaunch `browser-live` for the same project/session and current project revision,
then press **Inspect or recover gap proposal intent**. Press **Stage or resume
prepared gap proposal** to resume the original transfer. Review diagnostics again
and explicitly approve commit. Merely reopening or inspecting does not execute
an upload or commit. If credentials changed, the journal is invalid/corrupt or an
unfinished lease expired/was invalidated by Engine restart, recovery refuses.
It does not silently prepare a replacement.

Subsequent [original approved gap-commit recovery](PULSAR_BROWSER_GAP_COMMIT_RECOVERY.md)
adds explicit reconciliation of saved gap commit requests. Original recording and
export intents and physical device control remain outside this upload scope. Finished gap metadata remains retained
and blocks replacement until managed release is implemented. Crash/durability
coverage across additional filesystems and platforms remains required; Windows
source compilation does not establish runtime support for private journals.

## Verification scope

The network-disabled native campaign drops real successful Begin and Finish replies
and kills/restarts the browser server after each loss. Both recovered views retain
exact original request/lease identities. The final Finish returns the original gap
candidate; SQL confirms only the recording candidate and one gap candidate. The
prepared journal exists before effects, a deliberately changed credential binding
refuses without Engine dispatch, and recovery alone restores no commit authority.
All 492 inferred knots remain; five authored knots produce 497 neutral actions.
The export and browser download match an independent scalar oracle. Final custody
is empty, and all owned servers, Engines and the proxy join.

Rendered in-app browser QA records 401 estimates across three captures, commits
revision one with four gaps, prepares an intent and restarts its server. The new
page explicitly inspects the same intent with upload still unattempted, stages it,
obtains fresh diagnostics, separately commits and downloads 405 ordered actions
from revision two. Every position is 38, with endpoints 0 and 60000 ms. Exact
downloaded bytes, prepared/recovered accessibility states and screenshot are kept.
This rendered restart is graceful; the native campaign covers the two abrupt kills.

Clients pass 421 tests with ten ignored, twelve mock-player cases and ten strict
validation commands pass, including both Clippy configurations, native builds,
Windows source compilation, formatting, architecture and guards. New targeted
checks cover persisted checkpoints after refusal, workflow exclusion, checkpoint
failure before admission/bulk/finalization, changed binding/values/scope and missing
finalization phase. Core/Protocol/Engine sources are unchanged; no new full suites
are claimed for them. All 549 source identities, two native binaries and sixteen
borrowed runtime files are reverified.

No mid-rename/process-death filesystem campaign or broad platform durability is
claimed. Useful-model/media accuracy, arbitrary-tab capture, GPU live six-axis,
physical Handy sync and human/release acceptance remain open. Full roadmap PARTIAL.

See [checkpoint](evidence/roadmap-2026-10-13/browser-gap-journal/checkpoint.json) and
[ADR B3-197](../research/phase-b/ADR-B3-197-browser-gap-journal.md).
