# ADR B3-068: Original retained CUDA evidence downloads

Status: Implemented; scoped serial software and native CLI verification passed.
Date: 2026-10-08

## Problem

Historical descriptor inspection cannot replace access to the original origin, manifest, summary, native receipt and native trace. Bulk content must not be embedded in control responses or located through recorded filesystem paths.

## Decision

Add `BeginLiveCudaEvidenceDownload` with a capture epoch and a closed five-part enum. A new Begin requires current project Read authority and expected project revision. The existing opaque transfer lease, authenticated bulk handshake, bounded chunk operations, exact retry rules, per-chunk Read checks, abandonment, revocation and absolute expiry carry the bytes. No bulk wire layout, worker format or database schema changes.

The shared retained reader validates scope, canonical control originals, saved admission membership, receipt/trace consistency and SQL bounds before granting a download. The lease owns those original bytes and their evidence memory admission. Additional chunk memory remains charged after completion so exact retries never use released admission. Abandonment/revocation/expiry drop both claims; completed evidence leases remain counted against aggregate admission until retired. This uses RAM rather than creating new Engine staging files. Database retention eviction does not alter an already granted immutable byte snapshot. Restart invalidates the lease through the existing boot fence and durable request identity journal.

The first-party `live-capture download-evidence PROJECT CURRENT_REVISION CAPTURE_EPOCH PART OUTPUT` command streams bounded chunks, checks full length and SHA-256, syncs verified staging, abandons the lease, and only then publishes the destination without replacing an existing file. It reuses the package client's held-directory publication guard. Unsupported publication platforms remain fail-closed. The command supports origin, manifest, summary, receipt and trace independently.

## Validation and remaining scope

Check all five exact originals; admission lifetime after completed retries and abandonment/revocation/expiry; Read/revision admission; multi-chunk transport; truncated/corrupt bytes; endpoint/project mismatch; and no partial or overwritten destination. Exercise the shipped CPU-only CLI after reopening authored CUDA evidence and compare output files to the preceding exact originals.

Desktop presentation, listing/retention controls, concurrent eviction/restart/read races and native platform campaigns remain open. Original evidence is historical consistency, not model/media quality, physical permission or release qualification.
