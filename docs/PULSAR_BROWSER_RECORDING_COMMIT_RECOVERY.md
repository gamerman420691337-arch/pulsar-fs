# Original browser recording-commit recovery

After staging a recording candidate, review its diagnostics and explicitly acknowledge
whole-master replacement before preparing the commit. Preparation now saves the
original commit request, candidate, base revision and program SHA-256 in the private
session/project-bound recording-candidate journal before any commit dispatch.

If the commit reply is lost, restart `browser-live` for the same owner session/project.
Use **Inspect retained staging intent** to recover the recording selection, then
**Retry original commit request**. That action retries the already-approved original
request. It does not create a new approval or request. Inspection is inert; unapproved
recording candidates still require fresh diagnostics and acknowledgement. Confirmed
acknowledgements also survive restart, and exact retry creates no extra revision.

Current Engine authentication and candidate readback remain required. The replay checks
candidate identity, project, base revision, program digest and non-job origin before
commit; returned project revision/digest are validated before acknowledgement is saved
under the workflow lock. Current project readback updates the adapter's revision.
Changed pins, invalid acknowledgements, corrupt scope or unavailable authority refuse
without inventing replacement work. None of these operations actuates a device.

Older candidate journals without a commit field remain readable and acquire no approval.
The optional saved request is an inert record of prior explicit acknowledgement, not a
new reviewed pin or delegated authority. Recording and gap intents remain separate;
explicitly inspect the intended recording/gap selection before retrying when both exist.
[Confirmed candidate retirement](PULSAR_BROWSER_RECORDING_CANDIDATE_RETIREMENT.md) now preserves compact history and enables another take. Recording-index capacity and broader crash/concurrency/platform acceptance remain open. Private recovery storage remains unsupported on non-Unix platforms.

Verification: [checkpoint](evidence/roadmap-2026-10-13/browser-recording-commit-recovery/checkpoint.json).
Model/media accuracy, arbitrary-tab capture, physical Handy sync and human/release
qualification remain open. Full roadmap PARTIAL.

## Scoped verification

The native Linux/private-RAM campaign drops a verified successful recording commit
reply and kills the adapter. The committed master is already revision one; private
metadata retains the original approved request without a confirmed acknowledgement.
Restart/inspection restores that same request. A changed digest and an impossible
acknowledgement refuse before another commit dispatch and preserve the original file.
Explicit replay acknowledges revision one; subsequent server restart retains that
acknowledgement and exact repeat creates no extra revision.

The campaign then stops/restarts the Engine and adapter under the same owner session.
The original durable commit receipt replays with unchanged master and revision one.
Old ephemeral recordings read as unknown; no recording samples are reconstructed.
One project and one candidate remain, with no jobs or exports. All owned servers,
Engines and proxy join; final worker custody is empty. This tests orderly Engine
restart, not power loss or arbitrary crash durability. No device or fresh rendered
browser campaign was run.

Clients pass 425 tests with ten ignored; fifteen existing mock-player tests, strict
Client and workspace/all-feature lint, native App/Worker builds, formatting, architecture
and source guards pass. Core/Protocol/Engine sources are unchanged and no fresh suites
for them are claimed. The first extended-test compile borrowed a Deref field by value;
its source/finding and corrected targeted check are retained. The first parallel suite
found the inactive package-cleanup fixture still locked. The fixture now explicitly
unlocks its own completed stage before declaring inactivity; production lock/refusal
behavior is unchanged. The original failure and final default parallel suite are retained.

Recording Begin reply-loss recovery, recording-index capacity, broader
concurrency/crash/platform, useful learned motion, real-media/Handy/human/release
acceptance remain open. Full roadmap PARTIAL.
