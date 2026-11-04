# Browser authored gap proposals

The browser inspects a committed Stroke master and proposes explicit authored
linear interior bridges and nearest-value edge holds for all inspected gaps.
Preparation binds exact revision, digest and gap list, requires acknowledgement,
checks protected stencils through shared diagnostics and retains a local intent
before upload. It does not change the master. Staging uses the existing values
upload broker and validates the result against the prepared pure Core recipe.

A confirmed proposal can be inspected repeatedly without creating another
candidate. [Original upload recovery](PULSAR_BROWSER_GAP_RECOVERY.md) subsequently adds exact
Begin/lease/Finish resume after unknown outcomes; it creates no replacement upload. [Durable gap upload journals](PULSAR_BROWSER_GAP_JOURNAL.md) subsequently preserve original uploads across server restart; review/commit authority remains fresh.
A staged gap proposal becomes the active review/commit selection. Its explicit
commit uses an original identified Engine request, and preserves unrelated motion
and all original knots. Repaired points remain synthesized evidence; gaps do not
become observed or qualified motion. Protected/unanchored gaps refuse.

Use **Inspect committed Stroke for export**, acknowledge authored repair, then
**Prepare all inspected Stroke gaps** and **Stage or resume prepared gap proposal**.
Review diagnostics; acknowledge whole-master replacement and commit separately.
Finally inspect the new committed revision before downloading. The browser
currently repairs the whole inspected Stroke gap list; selective subsets remain
available through CLI. No operation issues a device command.

## Verification scope

The native HTTP/Engine campaign records 494 points across four scopes and three
handoffs, retains original recording staging through actual Engine restart, then
commits revision one. Browser repair preparation refuses missing acknowledgement,
stale revision and changed selections; repeated preparation has no upload effect.
Confirmed staging repeats preserve the same candidate. Shared review precedes an
explicit identified commit, whose repeat preserves revision two. All 494 original
knots remain and five new knots are synthesized. The independently checked
499-action export equals the browser download; original download retry is exact.
The database has one project, no ordinary jobs, two candidates and two exports.
Owned processes join and custody is empty. This does not exercise lost-upload
acknowledgement recovery; that branch remains fenced and unfinished.

The actual in-app browser decodes an authored gray WebM through the diagnostic
scalar graph, records 79 estimates, commits revision one with two gaps, and refuses
repair preparation without acknowledgement. Explicit preparation/staging/review and
commit produce revision two with 81 actions and zero gaps. Repeated original commit
is acknowledged without another edit. Actual downloaded actions are strictly
increasing, all position 38, from 0 to 60000 ms. The full accessibility state,
screenshot, bytes and independent download receipt are retained. This is diagnostic
model/software evidence, not trained accuracy, meaningful motion or device output.

Clients pass 416 tests with ten ignored; eleven mock-player cases and ten validation
commands pass, including both strict Clippy configurations, Windows compilation,
formatting, guards and native builds. Core/Protocol/Engine sources are unchanged;
no new full suites are claimed for them. Initial immutable-borrow compilation and
repeated-route warnings are retained with the repaired passing check.

Broader recovery/concurrency, browser/platform compatibility, useful learned motion,
real-media accuracy, arbitrary-tab capture, physical Handy sync and human/release
acceptance remain open. Full roadmap PARTIAL.

See [checkpoint](evidence/roadmap-2026-10-13/browser-gap-proposals/checkpoint.json) and
[ADR B3-195](../research/phase-b/ADR-B3-195-browser-gap-proposals.md).
