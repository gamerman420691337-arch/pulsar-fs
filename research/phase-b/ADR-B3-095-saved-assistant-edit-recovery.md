# ADR B3-095: persist exact assistant edit declarations before optional dispatch

Decision: save a private, schema-checked edit declaration containing endpoint/session/credential binding, original request ID, project/revision, full checked recipe and proposal/context approval digests. The displayed recipe must hash to its recipe ID and to the approved proposal digest. Use the existing owned, no-follow, single-link, bounded private file store. Never store session tokens.

CLI and desktop can restore and separately approve the exact declaration. Dispatch reloads the file and checks the reviewed semantic digest and credential namespace. Restoration does not discover a proposal, reselect a recipe, change the original ID or apply an edit. Historical accepted receipt recovery uses the original revision, including after Engine reopen; fresh execution still requires the retained proposal, original epoch and current revision.

Read-only refreshes preserve pending mutation identity. Changed saved requests, another mutation or restoring a replacement cannot displace an unconfirmed request. Show recovered candidate identity/base revision; stale recovered candidates require ordinary rebase/review before commit. Keep existing explicit immediate edit controls available.

Consequences: manual restore provides software recovery, not automated startup discovery or native desktop/crash qualification. Unaccepted expired requests cannot become new edits by retry. Memory handoff, installed/offline default delivery and model/media/platform/device/human/release gates remain open.

See [saved edit contract](../../docs/PULSAR_ASSISTANT_EDIT_RECOVERY.md).
