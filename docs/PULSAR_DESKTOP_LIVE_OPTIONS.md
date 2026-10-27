# Author browser live options in the desktop

The Browser live panel can now build its launch configuration from an explicitly captured Generator model/profile. A hand-written LiveCaptureOptions file is optional; the existing Options JSON file mode remains available.

1. In Generator → Video motion, enable **Use declared model analysis (unqualified)** and choose **Model file**.
2. Enter the model path and load its explicit compatible temporal profile through **Load profile**. The browser input contract is 64×64 temporal script-position analysis; a detector profile is not interchangeable. The Engine still checks runtime and model bytes at execution.
3. Open Browser live, choose **Generator model/profile**, and click **Capture Generator model/profile**.
4. Review the captured model path, model/runtime SHA-256 identities and provider. This choice stays fixed until another successful capture; later Generator changes cannot silently retarget it. A failed capture preserves the previous choice visibly.
5. Choose maximum analysis age, added preview buffer, capture lease and project start in milliseconds, plus server lifetime in seconds. Click **Launch browser player**, open its link and explicitly Play media.

Preview defaults are a 1000 ms maximum analysis age, zero added buffer, 30000 ms capture lease and project start zero. These are editable admission choices, not measured device or quality limits. Age accepts 1 through 30000 ms, buffer 0 through 30000 ms, and lease 1 through 30000 ms. Project milliseconds must fit checked signed nanoseconds. Invalid settings refuse before launch; no clamping or silent fallback occurs. The browser supplies actual presentation time and input-domain declaration at Start. The project start maps each fresh Play's initial source presentation stamp to the chosen project time; it is not proof of synchronization with an original source player.

Launch freezes a complete typed options value before spawning the server worker. Subsequent form or Generator changes cannot affect that value. The shared server retains all original request/recovery, stop, continuation and freshness behavior. No options file is written by this mode. Stop and join remain required before recapture or replacement launch.

This direct reuse accepts the Generator's explicit file-model choice and loaded profile; it does not extract an owner-selected pack into an unclassified raw-model request, import/approve/select a pack, select a default model or establish qualified inference. Existing file-mode workflows and pack admission remain separate explicit contracts. Useful model selection, media quality, rendered desktop/browser/platform acceptance, source-player calibration, Handy and human/release gates remain open. Full roadmap PARTIAL.

See [server ownership](PULSAR_DESKTOP_BROWSER_LIVE.md), [current status](PULSAR_CURRENT_STATUS.md), [ADR B3-212](../research/phase-b/ADR-B3-212-desktop-live-options.md) and [checkpoint](evidence/roadmap-2026-10-13/desktop-live-options/checkpoint.json).

## Scoped verification

Clients pass 433 tests (431 library and two integration) with eleven ignored. Four new direct-configuration checks cover exact units/identity preservation, absent/overflow/invalid timing refusal before worker creation, changed/failed Generator capture preservation and recapture refusal while owned. Existing ownership/headless tests still pass. Strict Client and workspace/all-feature Clippy, native App/Worker builds, 24 player mocks, format, architecture and source guards pass.

The explicitly run native production-panel controller builds a direct configuration with 1500 ms age, 20 ms buffer, 12000 ms lease and 750 ms project start. The real Engine retains those exact values after the test changes the discarded form to an invalid lease/missing model/file mode. Real CPU inference returns 0.375488758 within 0.0001 of the independent authored scalar oracle. Direct and JSON-mode capture shutdown/revision change/destruction/expiry leave five captures inactive, all servers joined, empty custody, one project and zero ordinary jobs/candidates/exports. This controller is not a rendered desktop test.

Engine/Core/Protocol and the shared browser adapter source are unchanged, so no fresh library suites or full CLI continuation/export campaign are claimed. Their prior evidence remains scoped to its source. Useful models, broader in-flight faults/pack parity, real-media, browser/source-player timing, vendor/edition/platform, Handy/six-axis and human/release qualification remain open.


## Selected offline pack mode

[Pack-bound live admission](PULSAR_LIVE_MODEL_PACKS.md) now adds a separate **Owner-selected pack** mode. Refresh the pack selection in Generator, capture its choice/profile in Browser live, review the identities/settings and launch. No raw model path is extracted; every fresh Engine lease rechecks the exact current selection and pack bytes. Selection loss retires affected captures. This mode requires a compatible temporal profile and does not select a default or qualify motion/device output.
