# ADR B3-199: Recover only the original pinned browser script export

Persist inspected master/revision, original request and private pinned output
identity before export dispatch. Use the existing session-bound recovery store and
workflow lock to inspect or explicitly replay unknown outcomes under current
Engine authority. Validate direct application-owned paths and directory identity
before effects; checkpoint acknowledgements and read files relative to a held
validated directory after effects. Compare against the inspected projection and
retain confirmed raw-byte digests.

Do not silently adopt legacy unbound directories, invent replacement requests or
automatically dispatch from startup/inspection. Managed release and broader
filesystem/platform/crash and physical/model qualification remain required.

See [contract and evidence](../../docs/PULSAR_BROWSER_EXPORT_RECOVERY.md).
