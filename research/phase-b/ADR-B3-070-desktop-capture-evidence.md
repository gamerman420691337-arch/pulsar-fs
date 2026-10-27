# ADR B3-070: Desktop capture evidence presentation

Status: Implemented; scoped software and native background-worker verification passed.
Date: 2026-10-08

## Problem

CLI inspection, discovery and exact original downloads do not provide the desktop workflow required by product integration. UI navigation must not apply old-scope replies to the current project, and byte transport must preserve the existing publication semantics.

## Decision

Add a Capture evidence tab with a small dedicated panel module. A bounded queue and session-owned background worker perform index queries, inspection and original downloads through the existing clients. The UI thread owns disposable view state only. Requests bind project and current revision; returned pages additionally match the requested cursor/limit and inspections match the requested epoch. Project or revision changes invalidate displayed records and destination drafts. Late stale queries do not populate the new view. Already requested exports retain their original scope in their completion notices.

Reuse the existing download implementation, including authenticated opaque leases, checked ranges/digests, abandonment and held-directory no-overwrite publication. No new protocol, schema, Engine staging or device authority is introduced. The five original artifact types are selectable; saved destinations are explicit. Record details are collapsible. Historical evidence remains unqualified.

## Validation and remaining scope

Check request/project/revision/cursor correspondence, stale view invalidation, and egui widget construction. Explicitly exercise the production panel background worker with the actual retained-evidence Engine, inspection and all five original exports, byte comparison and existing-destination refusal. Headless controller execution does not establish rendered human usability or native-window qualification. Retention controls, further concurrency/fault cases and full product/model/media/Handy/platform/human/release gates remain open.
