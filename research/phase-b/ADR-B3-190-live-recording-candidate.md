# ADR B3-190: Retain original live recordings as a distinct candidate source

Capture original actor-bound Engine records into a distinct durable recording
source and stage an uncommitted proposal through shared lineage. Importing a
funscript or unsigned downloaded JSON would erase original inference authority,
clock/provisional context and unavailable coverage; the separate persisted source
kind preserves those facts while permitting review without granting physical
qualification or new live-capture authority after restart.

See [contract and evidence](../../docs/PULSAR_LIVE_RECORDING_CANDIDATE.md).
