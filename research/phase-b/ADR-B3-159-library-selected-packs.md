# ADR B3-159: exact selected packs for folder consumers

Date: 2026-10-12. Status: selected software contract; qualification open.

Folder batch and scan generation may explicitly request the exact currently owner-selected installed model pack. Read its authenticated report once, retain its selection revision, derive shape/profile/dimensions from the declaration, and use the existing shared single-file pack generation helper for every pending file. Engine admission remains responsible for current authority and immutable dependency/native checks. Never auto-select, approve, replace or fall back to another pack.

Keep raw-model configuration mutually exclusive, skip existing companions and refuse unsupported multi-axis/overwrite modes. Dry-run performs only selected-identity lookup and pending-file discovery, not byte pinning or inference. See [scope and retained failure](../../docs/PULSAR_LIBRARY_PACKS.md). Model quality, defaults and full roadmap remain unqualified/PARTIAL.
