# Confirmed browser recording-candidate retirement

After the original recording commit is acknowledged, choose **Inspect retained staging
intent**, check **Retire the confirmed recording candidate, preserving Engine history**,
and press **Retire confirmed recording candidate**. The operation binds the original
stage/candidate/commit identities and acknowledged revision. It requires a stopped
preview and fresh authenticated candidate/project readback. Pending or unknown commits
refuse retirement; reconcile their original requests first.

Retirement marks the local handoff inactive and retains a compact receipt. It deletes
no Engine candidate, source, project, recording, commit receipt or output. **Inspect
retired recording candidates** reads up to 64 retained receipts without Engine work.
A new stopped playback can then prepare another stage request on the same project.
Repeating an old retirement returns its original receipt without clearing a newer
active candidate. History contains identity/pin metadata, not copied motion samples.

Existing journals without retirement/history fields remain readable. Read-only
inspection does not migrate or invent identity. A real retirement/preparation writes
its transition under the existing private session/project workflow lock. Changed,
foreign, malformed, duplicate or over-bound history refuses with original bytes
preserved. The active and historical stage identities remain distinct; a retired
handoff cannot be staged or committed as the current recording selection.

[Confirmed gap retirement](PULSAR_BROWSER_GAP_RETIREMENT.md) now has its own independent controls. Recording-index capacity and history reclamation remain separate work. Recording retirement does not clear a gap proposal or release Engine recording buffers. Broader crash/concurrency/platform and useful-model/media/
Handy/human/release acceptance remain open. Private storage is unsupported on non-Unix
platforms. No device action occurs. Full roadmap PARTIAL.

Verification: [checkpoint](evidence/roadmap-2026-10-13/browser-recording-candidate-retirement/checkpoint.json).

## Scoped verification

The native Linux/private-RAM campaign preserves original stage/commit reply-loss and
server/Engine restart recovery, then retires the confirmed recording handoff. Missing
acknowledgement, wrong revision, a commit whose reply was lost and a new unapproved
candidate all refuse retirement. History lists the original compact receipt; repeat
retirement returns the same receipt. A second actual capture on the same project
records two estimates and stages a distinct request/candidate at base revision one.
Repeating the old retirement preserves that newer active intent and prior history.
The database retains one project, two candidates, no jobs and no exports; the committed
master stays at revision one. Owned servers, Engines and proxy join and custody is empty.

Clients pass 425 tests with ten ignored; sixteen mock-player controls pass, including
new inspection/acknowledgement/pending-refusal guards. Strict Client and workspace/all-
feature lint, native App/Worker builds, formatting, architecture and source guards pass.
No fresh rendered-browser or physical-device acceptance is claimed. Core/Protocol/Engine
sources are unchanged and no fresh suites for them are claimed. Initial session-borrow
compilation failure is retained with the repaired scope-owned session. Final checks
and native campaign follow an explicit current-project readback scope guard.

The extended pure recovery test covers legacy absent fields, approved restoration,
retirement receipt binding and duplicate history refusal. Broader full-history, crash,
concurrency, platform, recording-index/gap retirement and model/media/Handy/human/release
gates remain open. Full roadmap PARTIAL.
