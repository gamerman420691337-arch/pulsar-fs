# ADR B3-107: show preparation before graphical packaged launch

Status: implemented software; native GUI qualification open.

Opening an AppImage from a file manager previously ran installation before the normal desktop, so installer errors were visible only on stderr. Present preparation and failure in a small local window, with work off the event loop and explicit retry or close. Keep filesystem authority in the executable callback and preserve the existing installer checks.

Use the existing command parser to distinguish valid GUI invocations from headless commands. Create the callback only after native window initialization succeeds. Execute the cached application after success. A close discards pending results and the composition root exits, preventing a late result from launching. Kernel I/O cancellation and interrupted-stage recovery remain separate responsibilities.

See [software and qualification scope](../../docs/PULSAR_RUNTIME_BOOTSTRAP_GUI.md). No model, device, human acceptance or platform qualification is inferred from controller tests.
