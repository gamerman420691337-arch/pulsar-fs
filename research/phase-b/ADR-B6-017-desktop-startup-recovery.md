# ADR B6-017: Open graphical recovery after engine startup refusal

Status: implemented; local verification recorded in ../../docs/PULSAR_DESKTOP_STARTUP_RECOVERY.md. Rendered, platform and human acceptance remain open.

Evidence: the executable previously propagated `ensure_engine` failure before constructing the desktop. Default and explicit `gui` launches therefore exited with stderr and could not show a recovery window. The desktop already uses an authenticated background request worker that can retry connection when no session has been established.

Decision: classify only valid default/`gui` invocations as graphical. Attempt the existing bounded engine startup once, then open the normal desktop with an optional retained diagnostic on failure. Add an explicit Reconnect control that sends Capabilities through the existing bounded request queue. Retain the original diagnostic until an authenticated Capabilities reply succeeds. Bound diagnostic display storage to 4096 characters.

Compatibility: `run_gui`, `run_cli`, and `PulsarDesktop::new` remain source compatible. Add `command_opens_gui` and `run_gui_with_startup_warning` to the client interface. No protocol, engine schema, portable format, startup budget or daemon ownership change. CLI startup errors still return before command dispatch. Help, version, completion and invalid arguments retain their parser routes without engine startup or GUI construction.

Rejected alternatives: extending startup deadlines conceals readiness failures; automatically spawning retries risks duplicate contenders; killing a timed-out child cannot establish ownership of the live instance. Stderr-only failure provides no graphical recovery. Environment/file warning handoff creates unnecessary process coupling.

Consequences: the window can open after launcher readiness refusal, but reconnect does not repair or restart an engine. State-directory/configuration errors and native window creation errors still return normally. No operation, model selection, device resume or qualification authority is inferred from recovery.

Validation obligations: injected composition tests must prove failure/success routing and diagnostic delivery, parser tests must reject informational/invalid/non-GUI invocations, formatting and strict lint must pass. Actual rendered recovery, authentication/restart behavior across platforms and human usability remain distinct acceptance gates.
