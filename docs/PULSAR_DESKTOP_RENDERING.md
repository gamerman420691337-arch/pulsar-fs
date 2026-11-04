# Desktop rendering evidence

Status: initial rendering checkpoint PARTIAL. A subsequent [native workflow and layout repair](PULSAR_DESKTOP_PAIR_LAYOUT.md) verifies limited authored window interaction; human usability remains open.

The current executable opens a 1280 × 820 `Pulsar Desktop` window under an isolated KWin virtual framebuffer and Xwayland display, with software OpenGL and its own DBus session, runtime/config/cache/data directories and `PULSAR_STATE_DIR`. The observed Studio window renders project controls, media placeholder, timeline, development/unqualified label and Engine-connected status. The [screenshot](evidence/roadmap-2026-10-08/desktop-rendering/initial.png) contains only this authored empty workspace.

The agent-workspace-linux MCP backend was unavailable. This check instead used the installed compositor directly in virtual mode; no host display or user Engine was targeted. An initial Wayland launch exposed no X11 application window, so the recorded run explicitly selects the isolated Xwayland display. Attempted XTest and direct X11 event injection produced no observed interaction. Read-only SQLite observation confirms zero projects, sources and jobs. Project creation, import, tab navigation, source-plan controls and annotated pair presentation are therefore not credited as passed window workflows.

The [checkpoint](evidence/roadmap-2026-10-08/desktop-rendering/checkpoint.json) retains pixels, window geometry, private process ownership, logs, state counts and executable identity. All 401 source records match the preceding passing Stash batch checkpoint; no Rust, Python or JavaScript repository source changed, so those suites were not rerun. Only the verified private GUI/Engine processes were terminated, and the compositor session is terminal. Credential files remain private and are excluded from the packet.

Next desktop evidence requires working isolated input and observed project/import/annotated-preview flows. Human usability, platform/vendor/offline distribution, installed Stash, model quality, physical Handy and release gates remain open.

The subsequent EIS-based campaign resolves the input-delivery gap and reproduces/fixes clipped predecessor presentation. Its current source-bound checkpoint supersedes the initial interaction UNKNOWN only for the explicitly exercised workflows; the initial screenshot and limitations remain historical evidence.
