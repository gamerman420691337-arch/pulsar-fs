# ADR B3-161: explicit audio bundles in folder consumers

Date: 2026-10-12. Status: implemented software contract; qualification open.

Extend shared folder configuration with an explicit checked audio bundle recipe and multi-axis intent. Reuse the existing single-file synthesis and atomic publisher; do not create another DSP or pretend audio-derived channels are faithful visual/physical measurements. Preserve full source filenames in new bundle target names. Skip any existing target without claiming its contents are complete; preserve original stroke companions. Emit an immediate source-bound success receipt per entry and stop future entries on failure with retained project/job/staging information.

Keep raw/selected-pack visual generation and single-file atomic output behavior intact. Require independent oracles and native consumer regressions, including failed later entries. Aggregate durable queue/overwrite, other formats/platforms, model/media/physical/human acceptance remain separate requirements. See [scope](../../docs/PULSAR_LIBRARY_AUDIO_BUNDLES.md). Full roadmap PARTIAL.
