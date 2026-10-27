# ADR B3-108: explicitly select retained runtime assets

Status: implemented software; qualification results recorded separately.

Manifest-addressed installation keeps independent runtime versions, but source-based launch still requires the supplier manifest. Add local host inspection and launch for one explicitly supplied retained manifest identity. Reuse the existing exact cache verifier and admission lock, opening existing components only. Validate schema and the directory's selected manifest digest before execution.

Do not infer a latest version, repair a damaged entry, replace an Engine, or introduce a durable active pointer. Keep graphical preparation and headless forwarded commands consistent with ordinary first use. Preserve authenticated protocol negotiation and project history. A retained-runtime return is distinct from database rollback and from changing an already-running compatible Engine.

See [operation and qualification boundaries](../../docs/PULSAR_RETAINED_RUNTIME_SELECTION.md). Signed updates, retention, component delta distribution, migration rollback and release compatibility policy remain independent design/qualification work.
