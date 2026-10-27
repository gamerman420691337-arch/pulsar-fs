# ADR B3-170: original retained batch preparation recovery

Date: 2026-10-12. Status: implemented scoped CLI recovery; full roadmap PARTIAL.

## Decision

Provide explicit recovery from the original retained preparation stage plus its original private pending journal receipt. A matching stage alone cannot establish the interrupted preparation identity. Require the captured receipt to equal the current retained journal with only the selected unresolved Preparing item transitioned to the inspected Prepared acknowledgement. Preserve original export IDs; refuse absent, foreign, linked, altered, conflicting or wrong-scope evidence. An exact already-Prepared repeat performs no journal write.

Use existing Stage inspection/current-master checks and private workflow locking. Recover client metadata only; generation and Engine export dispatch remain separate. Add no Engine/Protocol/storage contract or production crash hook. Desktop recovery controls remain a separate integration obligation.

## Evidence and boundaries

See [contract](../../docs/PULSAR_LIBRARY_PREPARATION_RECOVERY.md) and [checkpoint](../../docs/evidence/roadmap-2026-10-12/library-preparation-recovery/checkpoint.json). Actual client death after pending Prepared fsync and before record rename passes recovery, exact replay/restart and four native refusals; twelve outputs match prior authored timing oracles. Three new unit controls and 373 Client tests pass, ten ignored. Both strict lint configurations and remaining affected checks pass. Two killed clients and four native Engines are joined. External evidence-path interruption remains recorded; final rerun passes in the keep tier.

This proves a specific Linux death window, not general durability, GUI/platform qualification, useful models/defaults, independent real-media, physical Handy, human acceptance or release completion. No missing annotation or original intent is invented.
