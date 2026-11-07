# ADR B3-207: release stopped playback selections while retaining scope history

Status: implemented with scoped real-continuation and player-mock evidence. 2026-10-13.

The private recording index retained all discovered scopes in its active 64-entry
budget. Candidate/gap/export retirement did not reclaim that budget, so continued
browser authoring eventually refused new recordings.

Move a selected whole playback group into a separate bounded historical list only
after explicit acknowledgement and authenticated stopped/retained readback for every
scope. Never drop just one continuation and silently change a take's coverage. Refuse
unknown or active records. Keep old identifiers available for read/save, independent
of candidate staging. Restore history after adapter restart without new effects. Bound
active/history lists to 64 scopes each and reject duplicate IDs or split groups.

This retires local selection only; Engine buffers and durable artifacts remain under
Engine lifecycle authority. Preserve unavailable records and current actor checks.
History release and unknown-boot disposition remain open. See
[contract](../../docs/PULSAR_BROWSER_RECORDING_INDEX_RETIREMENT.md).
