# Browser recording index recovery

The browser adapter persists a bounded index of original recording identities before
sending StartLiveRecording. The index contains epoch, request, playback group and
revision; samples and capture lifecycle remain Engine-owned. Private recovery
storage binds the index to the current credential/session and project, validates
its schema and rejects duplicate epochs/requests or more than 64 scopes.

After restarting `browser-live` for the same project and owner session, choose
**Refresh recordings**. Retained scopes can be inspected and stopped records saved
through the existing controls. Startup and refresh only read metadata. Neither
operation starts recording, restores a live pixel channel, resubmits Begin, stages
a candidate, restores review consent or sends device commands. A still-active
capture must stop or expire before archive download. If the Engine boot or actor
has changed, unavailable records remain explicitly unknown. An index is not an
Engine record backup and cannot reconstruct samples after Engine loss.

The active bound is per retained session/project index rather than per server lifetime. [Stopped playback retirement](PULSAR_BROWSER_RECORDING_INDEX_RETIREMENT.md) now moves whole readable groups to bounded history.
Restarting cannot bypass it. A foreign or corrupt index refuses without rewriting
its bytes. History release and unavailable-record disposition remain separate work; do not delete this metadata to reconcile an unknown Engine effect.

Verification is recorded in the [checkpoint](evidence/roadmap-2026-10-13/browser-recording-index/checkpoint.json).
This is software recovery evidence only; useful learned motion, real-media quality,
arbitrary-tab capture, platform acceptance and physical Handy sync remain open.

## Scoped verification

A native Linux/private-RAM campaign runs the real Engine, temporal diagnostic
model and HTTP adapter. It kills the owned adapter after real estimates, explicitly
stops the original Engine capture, and starts a fresh adapter under the same session.
Recovered archives preserve the original request and accepted samples; repeated
downloads agree. Explicit staging reaches the shared assembler with two original
estimates and two preserved coverage gaps. Repeated staging returns the same
candidate. There is one project and one uncommitted candidate, no jobs or exports.

Foreign recording selection and changed credential binding refuse; corrupt binding
bytes remain unchanged. An unknown original identity is listed as unknown without
a replacement Start. Final server/Engine processes join and worker custody is empty.
The first harness expected a status snapshot before Start; the correct idle refusal
is retained with the corrected final run. This is native adapter evidence, not fresh
rendered-browser or Engine-boot-loss recovery evidence.

Clients pass 424 tests with ten ignored, including index schema/scope/duplicate/bound
checks; fifteen existing mock-player tests pass. Strict Client lint, native App/Worker
builds, formatting, architecture and source guards pass. Core/Protocol/Engine sources
are unchanged; no new suites for them are claimed. Broader continuation/concurrency,
index retirement, recording Begin reply-loss and candidate/commit recovery campaigns
remain open. Private storage remains unsupported on non-Unix platforms.
