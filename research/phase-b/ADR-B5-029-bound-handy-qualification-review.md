# ADR B5-029 — Separate retained evidence, operator decisions and live admission

Introduce host-local bounded qualification dossiers whose canonical subjects bind the
registration, observed identity, actual loaded control image, platform/transport and
explicit native HSSP profile. Snapshot the expected evidence bytes and commit them with
the import receipt in one transaction. Retaining or hashing data is not acceptance.
Synthetic origin is inadmissible for physical acceptance; official authority is not an
input option.

A distinct owner-authorized decision records an operator declaration, not verified human
identity or an invented signature. Acceptance requires explicit physical-review and online
inspection consents, revalidates the immutable evidence and current subject, and checks
fresh device identity and revision again after I/O. Checked review revisions plus atomic
receipts prevent late acceptance or historical replay from undoing withdrawal. Withdrawal
must remain possible when payload bytes are damaged or a former control build is unavailable.
Current decision metadata must match its original receipt.

Use the loaded controlling executable rather than the analysis-worker identity or a mutable
executable pathname. Linux uses the kernel-owned `/proc/self/exe` reference; other platforms
must implement a verified equivalent before enabling this qualification flow. Binding to
an exact control build deliberately invalidates old evidence for a changed implementation.

A selected accepted dossier can supply checked native timing facts and a configuration-record
match during read-only setup inspection. Public data never become a deserializable motion
grant. The selected match now checks discrete sample limits; it does not prove physical dynamics or current stopping,
validate every operative device setting or load a script. Qualified setup and controller
integration must consume fresh accepted facts and preserve their own effect/recovery rules.

The dossier tables are host-local and excluded from project authority/import restoration.
They use a strict self-versioned reader contract and bounded retained payloads. Core's
qualification type remains non-deserializable. No real acceptance or device movement is
performed by this implementation/validation campaign; positive decision tests are simulations.
