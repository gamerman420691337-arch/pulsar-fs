# ADR B3-187: Record accepted live inference under Engine ownership

Status: bounded ephemeral recording implemented; downstream candidate/export open.

A latest-position preview loses prior estimates and clears on retirement. Ordinary
values upload/import is authored data and cannot supply trusted live provenance.
Retain accepted estimates at the Engine's existing authority/clock/work validation
boundary instead. Reserve bounded aggregate memory before recording, preserve
original attempt/profile/pixel/time bindings, page under current actor authority,
and keep provisional/inferred/unqualified semantics.

Overflow fails visibly with a partial recording and valid completion counters.
Discard releases bytes/capacity while preserving a non-rearmable intent tombstone.
Keep state ephemeral to its original boot; do not invent persistence or candidate
acceptance. Next integration must retain linked scope coverage and use shared
project review/commit/export authority.

See [contract and evidence](../../docs/PULSAR_LIVE_RECORDING_BUFFER.md).
