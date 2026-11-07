# ADR B5-036 — Correlate fresh streams without assuming whole-script buffer residency

Inspect an acknowledged setup by its exact actor/request, record, revision, connection,
hosting and qualification selection. Capture the setup's mode generation in new immutable
intent records; legacy missing modes remain unknown. Recheck device identity, mode/settings,
retained credentials, hosted bytes and current authority around two bounded read-only samples.

Report the fresh stopped stream association and both buffer observations separately from the
complete expected script shape. HSSP streams through HSP, so buffer population/endpoints may
change and the full script may exceed capacity. The API exposes no content hash and does not
specify the tail index origin. Neither equal buffer sizes nor a count-minus-one rule supplies
content proof. These observations grant no controller, loaded-program or playback authority;
qualified streaming semantics, native authority and server-clock integration remain necessary.
