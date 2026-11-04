# ADR B3-097: bound assistant journal cells before decoding

Decision: share one typed report capture path between startup, status/list, cancellation, owner revocation, profile withdrawal and Start admission. SQLite returns only text identities of 1–128 bytes and report text of 1–65536 bytes. Refuse the 129th report before capturing it. Validate the report and match its attempt/project/session/original-request identity against stored columns before returning a catalogue.

Publish through a bounded JSON writer so escaped text cannot overrun the same 64 KiB ceiling. Update only the exact stored identity tuple. Limit schema declaration capture and preserve strict schema comparison. No table, wire format, receipt identity, or automatic pruning changes.

Consequences: damaged cells refuse before restart conversion or cancellation signalling; a valid captured catalogue is bounded to 128 reports and 8 MiB serialized report data plus bounded identity/decoded metadata. This is not a SQLite pager or whole-application peak-memory guarantee. Existing valid small reports and interrupted restart recovery remain supported. Oversized historical reports require explicit recovery; never silently truncate or delete them.

See [storage contract and validation scope](../../docs/PULSAR_ASSISTANT_JOURNAL_ADMISSION.md).
