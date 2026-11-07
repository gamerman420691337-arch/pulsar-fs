# ADR B3-171: desktop original preparation recovery

Date: 2026-10-12. Status: implemented scoped controller/UI software; full roadmap PARTIAL.

## Decision

Expose original preparation recovery through the existing background desktop library controller and require the same original pending receipt/stage/current-journal transition as the CLI. Snapshot batch, directory, index and both paths at the click. Obtain fresh workflow history after recovery; dispatch no generation or exports.

Distinguish known preflight refusals from uncertain journal publication. Leave the form editable before possible publication; tag journal-write errors and retain the original action after possible publication or a later status-response loss. Block replacement mutations while recovery is unresolved, preserve existing read-only inspection, and retry the original captured action. Add no Engine/Protocol/storage contract or production fault hook.

## Evidence and boundaries

See [contract](../../docs/PULSAR_DESKTOP_PREPARATION_RECOVERY.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-12/desktop-preparation-recovery/checkpoint.json). Two new headless input controls and actual native controller recovery after client death, known missing-receipt refusal, owned 0500 write failure/exact retry and desktop replay after Engine restart pass. Twelve outputs match retained authored timing oracles. Client tests pass 375 with ten ignored; both strict lints and remaining affected checks pass. Two native Engines and one deliberately killed client are joined.

Rendered GUI acceptance, broader crash/storage/platform behavior, visual/model folder workflows, useful defaults, independent media, physical Handy, human and release gates remain open. No missing annotations or acceptance receipts are supplied by this recovery feature.
