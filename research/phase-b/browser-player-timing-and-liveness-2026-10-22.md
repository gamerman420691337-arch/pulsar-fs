# Browser player timing and liveness for Pulsar

Retrieved 2026-10-22 from primary standards and browser-maintainer documentation. This is a proposed software contract, not measured browser/device performance or a 200 ms result. No browser session, media, model, credential, or device was exercised.

## Decision

Use the Pulsar-controlled local player first. A native challenge can bracket fresh browser execution, but **HTTP roundtrip alone cannot bound frame presentation error**. Maintain separate identities and error budgets for browser execution, media/frame position, native `CLOCK_BOOTTIME`, server/device timing, and physical response. Browser messages remain observations and user intent; they cannot reconstruct native device authority.

## What browser timestamps establish

`performance.now()` is relative to its global's time origin and is nondecreasing within that origin. `timeOrigin` is an epoch-related monotonic reference, not a mapping to Linux uptime. Window navigation and worker creation establish different origins. High Resolution Time permits coarsening and jitter: its 100 µs baseline, or 5 µs under cross-origin isolation, can be replaced with a coarser implementation-defined resolution. These numbers are not universal maximum timestamp errors. Never establish a native mapping by subtracting `timeOrigin` from wall-clock time. [High Resolution Time, 2026-09-01 Working Draft, §§4,7](https://www.w3.org/TR/2026/WD-hr-time-3-20260901/#sec-time-origin)

Sleep behavior must be qualified separately. Mozilla documents an open Linux/macOS issue where `performance.now()` excludes suspend, and MDN warns of platform divergence. Linux `CLOCK_BOOTTIME` includes suspend. Consequently, a browser/native offset may change during sleep without either clock moving backward. `Date.now()` is an additional discontinuity diagnostic, not a replacement monotonic authority. [Mozilla issue 1709767](https://bugzilla.mozilla.org/show_bug.cgi?id=1709767), [MDN sleep discussion](https://developer.mozilla.org/en-US/docs/Web/API/Performance/now#ticking_during_sleep), [Linux timekeeping](https://docs.kernel.org/core-api/timekeeping.html#basic-ktime-t-based-interfaces)

| Value | Meaning and limit |
| --- | --- |
| `video.currentTime` | Approximate official playback position, stable during script execution; setting it initiates a seek. It is not evidence that the destination frame has appeared. |
| rVFC `mediaTime` | Presentation timestamp of the callback's frame, in seconds on the media timeline. |
| rVFC `presentationTime` | Browser timestamp when the frame was submitted for composition. |
| rVFC `expectedDisplayTime` | Browser's prediction of visibility, not measured screen emission. |
| rVFC callback `now` / `presentedFrames` | Callback execution time / composition-frame counter; useful for age and missed-callback checks. |

The HTML definition separates official and current playback position. The rVFC proposal promises best-effort alignment: callbacks can arrive a refresh late, and busy main threads can miss frames. There is no universal maximum compositor/display delay. These are separate observations, not interchangeable timestamps. [WHATWG media positions](https://html.spec.whatwg.org/multipage/media.html#dom-media-currenttime), [rVFC metadata and scheduling, 2024-08-02 Community Group draft](https://wicg.github.io/video-rvfc/#video-frame-callback-metadata), [browser API description](https://developer.mozilla.org/en-US/docs/Web/API/HTMLVideoElement/requestVideoFrameCallback#description)

Captured tabs have another timeline. `getDisplayMedia()` supplies a display-surface video track. A `MediaStream` attached to a video element has its own advancing timeline, no seeking, and fixed playback rate. It does not expose another tab's HTML player position, source identity, pause or seek events. A source video can pause while its captured surface remains a live stream. Thus Pulsar's captured-stream timestamp cannot silently become source-media time; that needs an explicit source-player integration. [Screen Capture](https://w3c.github.io/mediacapture-screen-share/#dom-mediadevices-getdisplaymedia), [Media Capture and Streams, §6](https://www.w3.org/TR/2025/CRD-mediacapture-streams-20251009/#mediastreams-in-media-elements)

## Conservative native mapping

The following is a derivation, conditional on an admitted browser timing envelope, not a new browser guarantee:

1. Native code creates a one-use unpredictable challenge bound to Engine boot, page session, playback generation and selected source/project. Record `n0` on `CLOCK_BOOTTIME` immediately before releasing the challenge.
2. The browser samples `b = performance.now()` only after receiving that challenge, then echoes the challenge, page/time-origin identity, sequence and player/frame observations. Native code records complete ingress at `n1`, before queued processing can relabel its arrival.
3. With units converted conservatively and a justified browser timestamp error bound `q`, the offset `theta = native - browser` at that sample is enclosed by:

   `theta ∈ [n0 - b - q, n1 - b + q]`.

   This includes both directions' delays, browser task scheduling and processing. The midpoint is merely an estimate with the entire half-width retained; it is not a symmetric-network assumption. Do not discard delay using an unqualified browser processing-duration claim.
4. Mapping another browser timestamp `f` needs its own timestamp error plus a justified relative-rate bound over `|f-b|`. Mapping expected display time additionally needs a measured prediction/display-error bound. Without those bounds, only the challenge-response handling interval is established.

Reject missing/reused/expired challenges, nonfinite or overflowing values, changed origin/session/generation, backward frame/time values, unsupported playback rate, and unexplained nonoverlap with a fresh mapping. Quantize interval endpoints outward. A guessed precision or repeated timer-step observations cannot prove `q`. Avoid averaging intervals into a narrower guarantee without a justified model.

An advancing callback counter and media timestamp must accompany renewals; receipt of an arbitrary HTTP heartbeat must not keep a frozen frame valid. Bind inference frames to the same generation but keep player liveness independent of inference completion. A challenge created after an old frame does not make that frame fresh. Browser-controlled timestamps, even with a nonce, do not attest actual pixels, physical display, or human presence.

## Lifecycle and loopback boundary

HTML timers may wait for fully active document time or unsuspended worker time and add implementation-defined delay. Frozen pages suspend timers/fetch callbacks; discard can occur without events. `pagehide` is not reliable, and back-forward cache can restore old in-memory state. Native expiry must cover the missing-event case. Policy choice: retire on hidden/pagehide/freeze, reject late callbacks, and require fresh binding and explicit resumption after pageshow/resume. Visibility is a liveness input, not evidence of user attention. [HTML timers](https://html.spec.whatwg.org/multipage/timers-and-user-prompts.html#timers), [Chrome lifecycle, updated 2026-10-16](https://developer.chrome.com/docs/web-platform/page-lifecycle-api), [MDN pagehide](https://developer.mozilla.org/en-US/docs/Web/API/Window/pagehide_event#usage_notes)

`keepalive` permits a request to outlive its page; it supplies no stop-delivery deadline. Fetch rejects a body when it plus unfinished keepalive bodies in the same fetch group exceeds 64 KiB. A lifecycle stop send is therefore best effort; only a checked native acknowledgment confirms receipt, and receipt still does not prove physical stopping. [Fetch keepalive processing](https://fetch.spec.whatwg.org/#http-network-or-cache-fetch)

Fetch credentials default to `same-origin`, controlling credential inclusion, not device authorization. Scheme/host/port define origin: localhost, numeric loopback, and different ports are not interchangeable. Loopback's potentially-trustworthy classification enables certain browser APIs but does not authenticate its caller to native code. Retain exact server-side Host/Origin checks, bounded messages and a narrowly scoped page/session capability; keep native owner/device credentials out of JavaScript. [Fetch credentials](https://fetch.spec.whatwg.org/#concept-request-credentials-mode), [origin definition](https://developer.mozilla.org/en-US/docs/Web/Security/Defenses/Same-origin_policy#definition_of_an_origin), [Secure Contexts](https://www.w3.org/TR/secure-contexts/#is-origin-trustworthy)

`connect-src 'self'` constrains this page's scripted network destinations; it is not server authentication and does not prevent another client from addressing loopback. Preserve the restricted local CSP and avoid broad CORS exceptions. If WebSocket transport is added, qualify its explicit origin allowance: `'self'` handling for WebSocket schemes differs across browsers. [CSP connect-src](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy/connect-src)

## Fit to current source and remaining evidence

At the starting checkpoint, the [browser player snapshot](../../docs/evidence/roadmap-2026-10-22/handy-continuous-timing/source/crates/pulsar-clients/src/browser_live.html) sent `media_us` and capture epoch while its `performance.now()` readings remained local. It handled pause/seek/end and sent a keepalive stop on pagehide. The corresponding [HTTP adapter snapshot](../../docs/evidence/roadmap-2026-10-22/handy-continuous-timing/source/crates/pulsar-clients/src/browser_live.rs) checked Host/Origin and stopped capture after three seconds without accepted frames; that established analysis liveness, not a device stop guarantee. The native [Handy clock](../../crates/pulsar-engine/src/handy_clock.rs) already uses Linux `CLOCK_BOOTTIME`.

The [implemented successor](../../docs/PULSAR_BROWSER_PLAYER_CONTROL.md) now adds scoped browser observation/liveness and an optional exact native stop guard. It still does not implement the conditional presentation-clock mapping described above. Retire on stale observations or any page/playback discontinuity; restoration requires a fresh session and explicit resumption. A native expiry check must run before renewed dispatch. During host suspend the process cannot send a stop, so a suspend-aware deadline prevents resumed authority but supplies no physical stopping bound while asleep.

Qualification still needs exact browser/version/OS, privacy precision settings, sustained load, callback loss, hidden/frozen/discarded pages, back-forward cache, sleep/resume, compositor/display lag, source correspondence, and independent device timing/stopping. Specifications and successful loopback requests cannot close those gates. Source dates above identify draft/standards scope; mutable browser documentation was retrieved on the stated research date. Only this note was created.
