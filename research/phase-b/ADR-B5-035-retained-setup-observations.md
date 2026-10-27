# ADR B5-035 — Retain setup responses atomically without restoring playback authority

Keep the sanitized, validated HSP setup response with its original setup intent. A separate
version-3 observation table preserves immutable intent hashes while allowing acknowledgment
and response to commit atomically. New intents require retained response data; missing data
on an acknowledged new intent is corruption. Version-2 acknowledgments retain their original
identity and remain explicitly without response data.

Bind each bounded observation checksum to its setup record, require exact metadata retries,
and reject malformed, orphaned or missing required observations before writable startup.
An older reader refuses the new journal version. Historical recovery returns the response
without constructing a live binding. A fresh stream/program correlation, current authority,
server clock and qualified device behavior remain separate prerequisites for playback.
