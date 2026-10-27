# ADR B3-135: Image-origin portable lifecycle

Status: scoped authored lifecycle PASS; roadmap PARTIAL; unqualified.

Exercise the existing format-13 image-origin implementation through actual export, asynchronous upload/seal/import, restart, re-export, a second clone and another restart. Make original fixture suppliers unavailable before import. Preserve exact original namespace records and required origin/RGB objects, fresh clone identities and inert import semantics.

Use separate owned subjects to test origin-record and RGB corruption. Require startup refusal and byte-identical SQLite main/WAL after each refusal. These checks qualify the scoped authored portable lifecycle, not useful model quality, historical readers, interactive UI, persistent-disk durability or physical hardware.

Implementation and evidence: [portable lifecycle](../../docs/PULSAR_IMAGE_PORTABLE_LIFECYCLE.md).
