# ADR B3-156: desktop authors checked six-axis audio recipes

Date: 2026-10-12. Status: selected software contract; qualification open.

Move desktop audio drafts into a presentation-only panel with independent single-axis and six-axis state. Retain legacy single-axis defaults and omitted analysis for Full/default. Explicit six-axis selection exposes a fixed neutral-axis editor for each channel, with mapping/mode/band/threshold controls and checked Protocol recipe construction. No filesystem, decoder, inference runtime or Engine persistence enters the panel.

Generation uses the existing source-kind selection and revision-bound project command. Disable submission while busy, without an admitted media selection, without a project, or with any invalid channel. Captured requests own checked settings and source identity. Every bundle channel explicitly gates quiet windows; no physical-axis interpretation or device adaptation is added. Candidate generation does not commit or export; existing review/commit and selected-axis export remain separate actions.

See [desktop scope](../../docs/PULSAR_DESKTOP_AUDIO_BUNDLE.md). Headless egui interactions and a native shared-controller fixture can verify contracts, but do not establish interactive native GUI acceptance. Full roadmap remains PARTIAL.
