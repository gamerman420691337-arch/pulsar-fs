# Desktop startup recovery — 2026-10-10

Valid default and explicit `gui` invocations now open the desktop after the existing engine startup attempt fails. The launcher hands the error to the window; a bounded 4096-character warning survives later request failures and clears only on authenticated Capabilities success. Reconnect submits Capabilities through the existing background request queue, with the normal busy control. It never spawns another engine. CLI startup failures still return before command dispatch; informational and invalid argument parsing does not start an engine or window.

The original ten-second startup budget, quarter-second probes, instance-lock observations and conservative daemon ownership behavior remain unchanged. Existing client entry points remain compatible; the additive warning/classification interface is recorded in [ADR B6-017](../research/phase-b/ADR-B6-017-desktop-startup-recovery.md). Configuration/state-directory failure and native window creation failure still return errors. Recovery does not infer device or model authority.

Current evidence in [checkpoint](evidence/roadmap-2026-10-10/desktop-startup-recovery/checkpoint.json), on dirty branch `codex/pulsar-roadmap-m1` at base `4da4c08b59dc7b90e6c5aa3b420908257194eb8d`:

- All-feature root/client test build PASS; four composition tests PASS, graphical parser test PASS, existing informational parser test PASS.
- Complete current all-feature client library suite: **193 PASS, zero failures, four ignored**, 0.51 seconds. This passing run does not establish a repair for earlier intermittent concurrent journal failures.
- Default and all-feature workspace/all-target strict Clippy PASS. Formatting PASS. Architecture 350 and source guards 36 PASS.
- Four changed source/ADR snapshots, ten public logs, 348 source hashes and both executed binary hashes retained. Temporary test state is not retained.

Tests inject startup refusal at the actual composition seam and verify default/explicit graphical diagnostic delivery, successful graphical startup, CLI refusal without dispatch, and informational/invalid routes without startup/window construction. These are headless routing tests, not rendered window evidence. The production `run_native` callback installs the warning on the constructed desktop; native display execution remains unverified.

Rendered QA remains pending: the agent-workspace-linux skill requires the hidden workspace, but no workspace startup/context/permissions tools are exposed in this session. No host desktop automation or configuration change was used. Human usability, platform portability, model/media quality, Handy 2 Pro physical synchronization/stop behavior and release acceptance remain open. The current engine library failures and native timing reliability recorded in [export refusal assurance](PULSAR_EXPORT_REFUSAL_ASSURANCE.md) remain unresolved; this increment changes client composition only. Full roadmap remains active and incomplete.
