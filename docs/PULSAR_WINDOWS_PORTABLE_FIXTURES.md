# Host and guest path verification

**Windows GNU compilation and isolated portable tests PASS. Native Windows qualification UNKNOWN.**

The preceding [Windows source checkpoint](PULSAR_WINDOWS_SOURCE_CHECK.md) retained 54 emulated test failures. This follow-up corrects authored positive host paths and two actual path-domain defects: native capture's fixed POSIX guest coordinates were incorrectly subjected to host absolute-path validation, and bundled inventory presentation rejected Windows path prefixes. Guest coordinates now require exact canonical strings; ordinary host suppliers remain native absolute paths. Bundled inventory retains traversal, digest directory, exact child namespace, size and authority refusals.

Two real retained-file package tests run on supported Unix. The non-Unix counterpart verifies the existing unsupported refusal without source mutation or transfer progress. Script repair expected output uses the same canonical parent as production; output alias and no-clobber checks remain. Stash server paths stay POSIX and local mappings use native host paths. Model/library fixtures are pure metadata and establish no Windows model execution.

The initial corrected campaign retained 613 passed, 13 failed and 3 ignored tests. It exposed omitted embedded JSON host paths, the bundled prefix defect and an original byte-fixture assertion affected by host adaptation. Preserve exact original wire bytes separately from runtime validation of the adapted native host fixture. Initial Linux wire-test and strict lint failures are retained rather than overwritten.

Final evidence records **38 passing emulated executables, 626 passed tests, 0 failed and 3 ignored**. The final subject is rebuilt after strict lint corrections and the Linux headless pointer fix. Wine uses a fresh private RAM prefix, PID/network isolation, private device surface and no display; its own server is waited for before namespace teardown. Initial and intermediate campaigns remain retained.

Linux Core/Protocol suites pass, including 154 Protocol library tests. Rebuilt Engine passes 841 with 70 ignored; Clients passes 322 with 8 ignored. Both strict Linux Clippy configurations, workspace/all-target check, formatting, native App/Worker build, 445 architecture checks and 36 source guards pass. Windows GNU workspace/all-target check and all 38 linked portable test executables pass. Windows target-specific warnings remain and strict Windows lint is unestablished. The [checkpoint](evidence/roadmap-2026-10-12/windows-portable-fixtures/checkpoint.json) retains scoped source/binary identities, logs, failures and cleanup receipts; these are not hermetic build provenance.

The first rebuilt Linux Client run passed 321 and failed one drawn pointer test: a long absolute fixture path made the text midpoint fall outside the TextEdit clip rectangle. Its helper now clicks the visible start of the actual drawn text. The production UI is unchanged; the complete rebuilt Client suite passes. Owned per-run scratch is removed after accessible process-reference checks; borrowed model/runtime assets, shared cache, toolchains and unrelated state remain preserved.

Isolated Wine is an emulation check. It does not establish native Windows/MSVC IPC, private filesystem custody, confinement, GUI, media/model/device execution, packaging, signing, offline editions or clean-machine qualification. No annotation or physical-device evidence is manufactured. Roadmap remains PARTIAL.

Architecture: [ADR B3-146](../research/phase-b/ADR-B3-146-host-and-guest-path-domains.md).
