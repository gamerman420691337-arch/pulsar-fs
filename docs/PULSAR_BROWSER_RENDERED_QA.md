# Rendered local browser preview QA

2026-10-13. PASS for the listed Codex in-app browser/CPU scenarios only. Full
roadmap PARTIAL; physical playback and real-media quality remain unqualified.

The isolated Linux Agent Workspace MCP tools remained unavailable. The supported
computer-use inventory exposed an empty Codex in-app browser, so QA created one
new background tab and used its documented file chooser, media controls,
accessibility snapshots, read-only DOM observations and screenshots. No existing
user tab, host desktop or browser profile was operated. The owned tab is closed.

The selected file is an authored 90-second, 320×180, 10 fps constant-gray VP8/WebM
fixture produced with the installed system FFmpeg. The bundled runtime lacked
libvpx encoding; this affects test fixture creation, not production decoding.
Frames from actual browser decoding and canvas RGB intake match the independent
packed-gray SHA-256 `b8df4280090e8dbe984b13baccc0600e066ad891fcd24dea4024dc92328e440b`.
The real Engine/CPU worker's 149 sampled estimates match the authored scalar graph
oracle 0.375490196 within 0.0001. The page visibly shows 37.5% provisional Stroke.
This graph is not a useful trained motion model; no real-media accuracy follows.

## Exercised scenarios

- Local file selection, video decoding, native media Play and visible position
  feedback through the actual browser, HTTP adapter and confined Engine worker.
- Sustained playback with five observed prepared handoffs. The Engine sampler
  records each backend and stable browser playback identity; the original source
  clock declarations remain authoritative.
- Pause/seek and fresh resumed playback with a second browser playback identity.
  Native readback preserves historical Superseded captures and confirms stopped
  current captures. The accessibility state clears position on pause/Stop.
- Scrubber keyboard seek and explicit Stop preview. Visible feedback restores on
  resumed playback and clears on Stop; final readback finds all eleven observed
  capture scopes inactive.
- Empty final Worker custody, joined owned server/Engine processes and closed
  owned browser tab. The final browser error/warning query returns no entries.

Accessibility/screenshot evidence and Engine receipts are the primary signals.
The read-only DOM adapter's paused-time value was zero and is not used as exact
seek or pause timing proof. The evidence does not establish exact user seek
latency, seamless estimates through every transition, physical timing or a
browser/platform matrix. Displayed HTTP round trips are not capture-to-command
or device-response measurements.

The initial run rendered playback and resumption, but its 300-second QA server
expired before control verification finished. The supervisor then encountered
ConnectionRefusedError, and the page correctly reported unconfirmed Stop. That
run is retained as PARTIAL with its journal and joined exits. The final bounded
run adds automatic Engine sampling and a longer test window; it completes explicit
Stop and verified retirement before teardown. Production source was unchanged.

All 541 recorded source identities and three App/Worker/controller hashes match
the preceding continuation checkpoint. Its 410 passing Client tests (ten ignored),
four mock-player tests and strict checks retain their original source scope;
Cargo suites are not rerun for this QA-only campaign. Runtime/asset identities are
rechecked. Fixture credentials/database stay in private RAM; only public authored
media, scoped reports, screenshots and bounded diagnostics are retained. The
loopback server shares host networking so the in-app browser can reach it; native
worker confinement and the private filesystem namespace remain in force.

## Remaining gates

Native eframe desktop GUI acceptance is separate and remains open. Other browsers,
codecs, platforms, arbitrary-tab integration, streamed funscript recording, useful
trained-model quality, six-axis/GPU behavior, Handy hardware/firmware/credentials,
calibrated synchronization and human/release acceptance remain required.

See [live screenshot](evidence/roadmap-2026-10-13/browser-rendered-qa/live-feedback.jpg),
[stopped screenshot](evidence/roadmap-2026-10-13/browser-rendered-qa/stopped-feedback.jpg)
and [checkpoint](evidence/roadmap-2026-10-13/browser-rendered-qa/checkpoint.json).
