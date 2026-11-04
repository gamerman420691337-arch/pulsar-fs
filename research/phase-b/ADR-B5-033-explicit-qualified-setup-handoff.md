# ADR B5-033 — Explicit qualified setup with durable request and journal links

Expose one bounded HSSP setup operation through the shared protocol and CLI. Require explicit
setup consent, host-local owner authority, project Read/Export grants, a current accepted
qualification and fresh checked connection, settings and hosted bytes. Require existing HSSP
mode without active playback; do not silently change mode or start playback.

Reserve the global actor/request fingerprint before journal preparation, link the prepared
record durably from the project database before claiming, and commit the single-use claim
before HTTP handoff. Check current references again and release all metadata locks before
I/O. Repeats return history without reissuing an effect. Startup verifies the cross-store
links read-only so a missing or emptied effect journal cannot silently reset prior work.

A validated response records acknowledgment only. Unknown I/O and failed observation writes
retain reserved history; no loaded program, playback admission or physical stopped evidence
is created. Authority changes after handoff cannot retract the request and must not erase
its outcome. Loaded-program correlation, reconciliation and physical qualification remain
separate requirements.
