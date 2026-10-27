# ADR B3-069: Bounded retained capture index

Status: Implemented; scoped serial software and native CLI verification passed.
Date: 2026-10-08

## Problem

Known-epoch inspection and original downloads leave users unable to discover retained captures through Pulsar. Browsing must preserve project authorization and must not materialize all evidence or treat an indexed row as validated execution.

## Decision

Add a nonmutating `ListLiveCudaEvidence` command with an optional exclusive epoch cursor and a page size from 1 to 16. Require current Read authority and expected project revision through the ordinary Engine transaction. Return a typed page of capture identities, scoped to that project, ordered by SQLite binary epoch comparison. Query at most limit plus one scalar rows after the existing bounded table shape checks. Do not load original blobs, follow filesystem paths, reserve CUDA resources, create transfer leases or alter database layout.

A continuation is the last returned epoch only when a further row exists. Pages are individually transaction-consistent; changes between requests are not hidden by an implied cross-page snapshot. Epoch order is explicitly not chronology. The first-party CLI exposes the same cursor and bounds, and client hydration validates response shape and project scope. Inspection/download retain independent full-original validation.

## Validation and remaining scope

Exercise multiple pages, exact last page, empty/foreign project, cursor beyond last, invalid limits, malformed scalar refusal, malformed-original index distinction, current Read/revision admission, revocation and shipped CPU-only CLI discovery after restart. Retention editing, desktop browsing and broader concurrency/platform/quality/release qualification remain separate obligations.
