# ADR B3-025: durable CUDA intent and older-writer barrier

Status: implemented intent ledger; production GPU admission disabled

## Problem

GPU requests add device, image, policy and resource semantics that older writers
cannot preserve. Persisting such attempts without a reader barrier could lose
those semantics on reopening. Ordinary dependency-based restart also cannot
reestablish fresh GPU authority, observations or process retirement.

## Decision

Persist canonical validated request identity and engine epoch in an immutable
attempt ledger, atomically with the job/attempt and schema-15 barrier. Validate
membership and layout through read-only startup before writable SQLite. Preserve
the higher barrier through existing migrations. Retain interruption across
restart and exclude GPU declarations from automatic resume. Keep launch disabled
until the remaining owner/resource/mount/retirement boundary is implemented.

## Evidence and limits

See [the durable-intent contract](../../docs/PULSAR_CUDA_DURABLE_INTENT.md) and its
source-bound packet. Authored records are exercised through real engine/CLI
reopening, an exact preceding reader and held nonempty-WAL refusals. Unit tests
also check atomic rollback and independent count/byte admission failures.

This ledger records intent, not approval, captured-byte custody, successful
inference, physical retirement or qualification. Accepted GPU evidence and
durable snapshot retention remain separate work. Model/media/device/platform/
performance/human and release gates remain open.
