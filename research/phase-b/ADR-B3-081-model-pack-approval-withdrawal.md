# ADR B3-081: explicit model-pack approval withdrawal

Status: implemented; bounded software verification is recorded in the linked checkpoint. Full roadmap INCOMPLETE.

## Problem

Deactivation prevents future selection admission but does not cancel generation accepted under an earlier approval. Artifact equality cannot identify that authority: a direct generation may legitimately reference the same model and approval bytes. Catalog expiry or corrupt assets must not prevent withdrawal of retained metadata.

## Decision

Add an owner-authorized withdrawal command carrying the exact approved manifest/revision, the current optimistic selection fence and an explicit legacy-job scope. Record direct or pack admission roles atomically in the existing private requests table when the Engine creates generation. The private namespace cannot be a checked client SessionId. Bound, typed receipts bind job/project and captured approval/model identity; the digest detects corruption, not malicious host database replacement. Preserve admission roles across resume; do not infer missing legacy roles from artifact equality.

Withdraw the current selection and cancel its unfinished generation in one transaction. Historical withdrawal proves issuance through retained selection receipts, cancels only matching classified generation and preserves a newer selection. Missing classification refuses by default; explicit inclusion cancels all unfinished unclassified generation, including potentially direct legacy jobs. Bound the entire pending-job inventory to 1024 and refuse corrupt records before mutation. Do not read or execute native assets for withdrawal.

Persist cancellation state and the original request receipt before signalling exact captured worker handles under the completion fence. Rollback signals nothing; replay returns historical metadata without signalling replacement workers. Worker/resource retirement still requires join and remains independent of cancellation intent.

## Compatibility

Existing command bytes and portable format are unchanged. No table, public event or database version changes are introduced. Client recovery raises its reader barrier to schema 3 only for withdrawal intent and retains it thereafter; older readers refuse unknown withdrawal entries. Recovery stores exact intent and credential binding, never the owner proof. New command/reply variants require supporting peers; no weaker fallback is allowed.

## Verification and remaining scope

See [implementation and source-bound evidence](../../docs/PULSAR_MODEL_PACK_WITHDRAWAL.md). Typed authored job fixtures and real Engine handlers establish role separation, historical preservation, rollback/retry/replay and publication fencing. They launch no native worker. Automatic catalog-triggered revocation, live capture, Preview and native preparation cancellation, installed native/media execution, storage lifecycle failures, rendered/platform/human and physical-device/release qualification remain open.
