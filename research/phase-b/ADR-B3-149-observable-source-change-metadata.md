# ADR B3-149: Detect observable source changes despite restored modification time

Status: implemented; qualification remains scoped.

A mutable input can be rewritten at the same length and have its modification timestamp restored. Length/mtime comparison alone therefore misses an observable change recorded in Unix ctime. Use one metadata predicate for fresh source capture, worker reuse and retained-object verification. Require successful modification-time retrieval; compare Unix ctime seconds and nanoseconds. Do not use atime, which ordinary reads can change.

Retain a failing preceding-predicate regression using actual rewritten bytes and restored mtime. Require the fixture to expose different ctime before asserting rejection. An unchanged control must remain accepted. This checks observable metadata semantics, not every race or filesystem durability outcome. Keep captured content hashes, existing no-clobber publication and platform support boundaries intact.

See [scope and evidence](../../docs/PULSAR_SNAPSHOT_CHANGE_DETECTION.md). This decision does not close independent model/media/device/platform/edition/human/release gates.
