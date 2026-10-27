# ADR B6-011: Desktop recovery intent precedes pack dispatch

Status: implemented; local source-bound evidence retained separately.

Persist a bounded, session-bound client record before pack import/selection/deactivation RPC. Keep exact request ID and typed source/digest/revision intent; never persist owner or bearer capability bytes. Bind their original values by SHA-256 digests so rotated credentials or owner capabilities cannot silently renew old intent. Only the engine authenticates and authorizes a replay. Restore entries for explicit inspection/replay, never as current selection or automatic approval.

Use owned private Unix storage, descriptor-relative no-follow file access, per-session advisory locking, current-record merge and synced atomic rename. Refuse corruption, size/count violations, unsafe files, changed intent or binding and busy locks before mutation dispatch. Mark acknowledgement only after validating the expected engine receipt type. Explicit dismissal requires acknowledged local metadata and affects only the client record. Do not silently prune unknown outcomes or clean interrupted staging files.

The desktop RPC thread and native process fixture share the store/runner. Restoration retains the original operation even if form values change. Accepted historical receipts do not rearm selection: query current owner status independently. The journal stores source locators, not imported asset snapshots, and does not replace engine request receipts or authorize a new paired session.

Use client journal schema 1. Database schema 13 and portable format 9 are unchanged. Unix adapter behavior is locally tested; native-window/human acceptance, non-Unix storage and other model/media/device/platform/release gates remain open. See ../../docs/PULSAR_DESKTOP_PACK_RECOVERY.md.
