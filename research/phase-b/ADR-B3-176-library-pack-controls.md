# ADR B3-176 — direct desktop selected-pack library controls

Status: implemented software checkpoint; qualification remains open.

The desktop visual library editor now loads the current owner-selected pack in a bounded background operation and captures an explicit digest, preset, dimensions, threshold and sampling stride. Capture queries the current choice/profile again before discovery and retains fully parsed Prepare content for exact retries; refresh cannot rewrite pending preparation. No import, selection or approval is performed. Client tests pass 392 with ten ignored; both strict lint configurations, Windows source check (with existing warnings), native/helper builds, formatting and architecture/source guards pass. Native controller refresh/capture/publication, wrong-digest refusal without a batch, and the CLI restart/repeat/cancellation/corruption/deactivation/reselection regression pass; four scripts match the retained detector fixture. Stopped-batch migration, unknown-effect disposition, advanced views, continuous watching, broader faults and rendered GUI/model/media/platform/device/human/release qualification remain open. Full roadmap PARTIAL.

Share recorded owner selection lookup with ordinary folder consumers. Refresh is read-only and uses the background controller; capture reads the current fence again and converts successful intake into immutable Prepare content. Preserve the original mutation when an acknowledgement is uncertain. Only Client controls/service/helper paths change; Protocol and Engine source identity remains unchanged from the preceding pack checkpoint.

See [contract](../../docs/PULSAR_LIBRARY_PACK_CONTROLS.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-12/library-pack-controls/checkpoint.json).
