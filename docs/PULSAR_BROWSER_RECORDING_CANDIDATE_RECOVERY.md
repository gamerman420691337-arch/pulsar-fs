# Browser recording-candidate staging recovery

The browser now keeps the original recording-stage request, checked options,
base revision, attempt marker and confirmed candidate identity in private
session/project-bound storage. Preparation is inert. Stage checkpoints the
original attempt before Engine dispatch and its acknowledgement afterward under
the same workflow lock. Lost replies keep the original request for explicit replay.

Restart `browser-live` under the same owner session/project, then select
**Inspect retained staging intent** or retry
staging. Recovery itself does not create an Engine candidate. Explicit staging
reuses the same request/recording selection; a different prepared range refuses.
Current Engine authority and original durable receipts remain authoritative.
Unavailable native recordings cannot be reconstructed after Engine boot loss;
only an already-successful original staging receipt can replay across that loss.

Restored candidate selection contains no reviewed pin or commit approval. Review
current diagnostics before approving any new commit. Subsequent [original recording-commit recovery](PULSAR_BROWSER_RECORDING_COMMIT_RECOVERY.md) retains explicitly approved original requests across server loss. A new commit still requires fresh diagnostics and acknowledgement.
Gap candidates retain their independent existing recovery contract. No operation
here issues device commands or changes motion evidence qualification.

Schema, credential/project scope, recording bounds/duplicate requests and
acknowledgement/attempt consistency are checked before dispatch. A foreign or
malformed journal refuses without replacement. [Confirmed candidate retirement](PULSAR_BROWSER_RECORDING_CANDIDATE_RETIREMENT.md) now marks completed handoffs inactive while retaining compact history; unknown commits remain fenced. Private
recovery storage is not implemented on non-Unix platforms.

Scoped checks and fault evidence: [checkpoint](evidence/roadmap-2026-10-13/browser-recording-candidate-recovery/checkpoint.json).
Useful motion, arbitrary-tab acquisition, physical Handy synchronization and
model/media/platform/human/release qualification remain open. Full roadmap PARTIAL.

## Scoped verification

The native Linux/private-RAM campaign drops a verified successful Engine staging
reply, kills the adapter, and recovers the original request after restart. Explicit
replay returns the exact candidate ID observed by the proxy; every staging dispatch
uses that same request. A second restart preserves the confirmed selection. Commit
and prepare-commit refuse before fresh review. Changed options and credential binding
refuse without replacement; current diagnostics then permit explicit commit and exact
repeat at revision one. One project and one candidate remain, with no jobs or exports.
Original inferred estimates and missing coverage remain preserved. Owned servers,
Engine and proxy join; final worker custody is empty.

Clients pass 425 tests with ten ignored; fifteen existing mock-player tests, strict
Client and workspace/all-feature lint, native App/Worker builds, formatting, architecture
and source guards pass. Core/Protocol/Engine sources are unchanged and no fresh suites
for them are claimed. No fresh rendered-browser or device campaign is claimed.

Two added commit-campaign attempts missed the existing three-second first-frame
deadline. The retained timing trace locates a 4.427-second caller gap before first
frame dispatch; a separate write probe measures 0.831 seconds in a synchronous
evidence write. Buffering fixture receipts until after explicit Stop makes the full
recovery/review/commit campaign pass. The exact cause of the entire first gap is not
proven. No production timeout or guard was relaxed; initial failures remain retained.

Recording Begin reply-loss recovery, recording-index capacity, broader concurrency/
crash/platform, useful learned motion and real-media/Handy/human/release gates remain
open. Full roadmap PARTIAL.
