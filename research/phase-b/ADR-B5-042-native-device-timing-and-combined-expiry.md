# ADR B5-042 — Native synchronization lifetime and combined source expiry

Preserve successful synchronization only through a current-process context reserved before
handoff and completed after journal commit. It survives ordinary controller replacement but
remains bound to the original setup and dependencies. Include pending contexts in retirement
so revoke/regrant cannot resurrect a callback after the original preparation has been removed.
Historical DTOs and journal records cannot reconstruct this authority.

Require a version-4 device error/age/user-offset contract and add host/server and device errors
against one budget. Keep original synchronization and server acquisition ages, and carry the
earlier absolute source deadline into the playback runtime even when reanchoring an estimate.
This prevents a fresh controller or queue handoff from extending older timing evidence.
