# Browser committed-script download

2026-10-13. Scoped Linux software and rendered browser checks pass. Full roadmap
PARTIAL; motion and hardware remain unqualified.

In the local browser player, stop capture, then press **Inspect committed Stroke
for export**. Inspection reads the current committed revision, digest, action
count and unavailable gaps. **Download inspected Stroke (.funscript)** first
prepares an immutable original export intent, then requests the Engine export.
**Retry original script download** reuses that exact request. The browser downloads
`pulsar-stroke-revision-N.funscript`; it sends no toy commands.

Export requires a nonempty, gap-free Stroke master with strictly increasing
millisecond timestamps after rounding. Stale revisions/digests, negative times,
quantization collisions and changed prepared intents refuse. Committing a recorded
candidate preserves gaps; it does not silently bridge them. Selected linear
bridges and nearest-value edge holds are currently available through the explicit
[CLI gap proposal](PULSAR_GAP_RESOLUTION.md), followed by review and commit.
[Browser gap-proposal controls](PULSAR_BROWSER_GAP_PROPOSALS.md) subsequently add all-inspected-gap preparation, staging, review and separate commit.

The Client creates a private output directory beside the Engine endpoint, pins its
device/inode and writes/syncs an intent journal before dispatch. The shared Engine
`ExportAxisPinned` command produces the actual bytes. The Client checks the
acknowledgement, reads a bounded regular file without following symlinks, compares
its opened inode/device and checks the parsed script against the inspected master.
Its projection is a comparison oracle, not a second export authority. The page
receives no arbitrary output-path authority or Engine credential.

Export directories and intent journals are intentionally retained in application
state, including after an unknown outcome. Subsequent [export recovery](PULSAR_BROWSER_EXPORT_RECOVERY.md) adds explicit original-request replay after server loss. Managed release remains required; the journal itself grants no authority. Preparation
currently requires Unix directory identity. Windows source compilation passes but
this export workflow is not qualified there.

## Verification

Clients pass 416 tests in total, with ten ignored (414 library and two integration
tests); ten mock-player cases and ten validation commands pass, including both
strict Clippy configurations, formatting, architecture/guards, native App/Worker
build and Windows source compilation. No new Core/Protocol/Engine suite is claimed:
their source is unchanged. All 549 source identities, two native binaries and 16
borrowed runtime files were reverified.

The native HTTP/Engine campaign records 492 estimates across four scopes and three
handoffs, stages and commits the original candidate, preserves five gaps, and
refuses unresolved export. Explicit CLI gap proposals add five authored knots;
commit revision two yields 497 neutral actions. Browser export equals the separate
Engine export, exact retry is preserved and stale preparation refuses. The
campaign retains two candidates and two exports, no ordinary jobs, empty custody
and joined owned processes. The scalar graph is authored diagnostic data, not a
useful trained motion model.

Rendered in-app browser QA independently imports/commits an authored two-point
fixture (20 at 0 ms, 80 at 1000 ms), inspects revision one and downloads/retries via
actual controls. Both 168-byte files are identical and exactly match that fixture.
Screenshot, full accessibility state and actual downloaded bytes are retained.
This rendered scenario performs no video inference or physical playback.

Initial missing-runtime-dependency compilation, clone-on-copy lint, incorrect
fixture-import syntax and teardown optional-field assumptions are retained. The
harness correction follows the actual optional custody schema; production guards
are unchanged. Final cleanup and checkpoint status are recorded in the evidence.

See [checkpoint](evidence/roadmap-2026-10-13/browser-script-download/checkpoint.json)
and [ADR B3-194](../research/phase-b/ADR-B3-194-browser-script-download.md).
