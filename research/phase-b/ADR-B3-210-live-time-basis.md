# ADR B3-210: retain declared timestamp domain through live recording lineage

Status: implemented with scoped Protocol, shared-library and native evidence. 2026-10-13.

Local media presentation and display-capture presentation previously entered the same
unlabeled SourceTimestamp path. A recorded tab stream could lose the distinction before
candidate review. Documentary caveats alone did not retain it in authoritative lineage.

Add a producer-declared enum to admitted live options and Engine-owned recording pages.
Preserve legacy missing-field reads as generic producer declarations, not verified media
clock. Freeze the label across continuation and reject mixed-domain assembly. Include
it in durable recording proof and stable archive paging; warn on display-capture
candidate review. Explicit browser source acquisition chooses the appropriate declaration.
Reject display labeling on encoded-source input. Do not change scheduler time mapping,
neutral motion values or qualifications; source-player identity and clock integration
remain required separately. See [contract](../../docs/PULSAR_LIVE_TIME_BASIS.md).
