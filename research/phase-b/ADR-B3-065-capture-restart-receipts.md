# ADR B3-065: Observe exact capture admission receipts after boot retirement

Status: Implemented and locally verified. Broader recovery/qualification remains pending.

Separate an exact saved request replay from new live admission. Check current authentication, grants, revision and exact request identity before returning historical capture metadata. If its runtime is absent after boot change, return the existing inactive EngineRestarted variant, without restoring a lease, pixels, registry or worker. Only new admission needs the current boot nonce; unknown old-boot requests still refuse before resource acquisition.

Keep existing protocol and database formats. Receipt observation does not promise recovered execution counters or final evidence, source continuity, automatic restart or physical readiness. An explicit fresh Start gets fresh capture/source identity. CLI replay support and abrupt owner-death recovery remain separate work.

See [implementation and evidence scope](../../docs/PULSAR_CAPTURE_RESTART_RECEIPTS.md).
