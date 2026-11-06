# ADR B3-188: Opt-in browser recording with bounded stopped-capture downloads

Status: implemented with scoped native and rendered-browser evidence.

Begin Engine-owned recording before the first pixel of each initial or prepared
capture. Retain original intents before dispatch and keep unknown outcomes visible.
Expose stopped captures separately, preserving original profile/clock/provenance
and evidence labels. Bound archive reads and require stable metadata. Read/save
failure must not cancel unrelated active preview. Keep recording off by default.

Downloaded JSON is inspectable evidence, not native authority, candidate acceptance
or final funscript output. Follow-up candidate assembly must retain missing coverage
and use shared project review, explicit commit and export.

See [contract and evidence](../../docs/PULSAR_BROWSER_RECORDING_CONTROLS.md).
