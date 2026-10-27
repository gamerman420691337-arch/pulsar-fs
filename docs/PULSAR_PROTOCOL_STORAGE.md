# Protocol storage verification

Recorded checkpoint scope: the later [client storage increment](PULSAR_CLIENT_STORAGE.md) supersedes facade storage on current source. The statements below retain the preceding packet and its source identities.

Protocol strict all-target Clippy passes after removing redundant conversions and boxing large native message payloads. Default and WASM workspace all-target checks pass. The full roadmap remains incomplete.

On the measured 64-bit Rust toolchain, inline RequestFrame storage falls from 1,392 to 56 bytes, ResponseBody from 4,096 to 200, WorkerOperation from 1,120 to 328, and WorkerOutput from 1,424 to 208. Payloads move to owned heap storage; these measurements establish neither reduced total memory nor faster execution.

Three synthetic wire fixtures were compiled and encoded against a private copy of the preceding protocol source and the current source. Generate, PackageDownload and bulk Ready bytes match exactly. Permanent tests decode/re-encode those fixtures and bound inline enum sizes. They cover those concrete encodings, not every possible variant value. JSON tags and protocol version remain unchanged; Rust constructors and ownership boundaries change.

The client facade retains its value types. Existing engine input checks, transport byte limits, authority checks and worker deadlines remain in force. Recovery-journal implementation and concurrent journal tests match their pre-edit source hashes. No credentials, real-media training, live Stash publication or device actuation occurred. The owner supplied no existing annotations and instructed software work to continue. Handy 2 Pro remains unqualified with firmware and encrypted credentials pending.

Recorded failures: the full client library run passed 180, failed four and ignored four; its concurrent journal tests reported busy/deadline failures. A sequential diagnostic of the four cases passed, which does not establish parallel reliability. The full WASM engine library run passed 675, failed two package-export tests and ignored 63. Those cases exceeded their bounded completion expectations. No timeout was increased and no durability synchronization was removed. Startup and filesystem timing remain unresolved.

The portable desktop process suite passes four cases, including cancellation during a held real upload and clone/re-export. All three active Phase A cross-process cases fail at engine startup, with two historical-reader cases ignored. An owned process sample collected after those failures observes the later Clippy processes and does not diagnose startup. All 16 package-export cases pass in a separate sequential diagnostic; this does not erase the failed parallel run. The actual confined paired WASM worker passes its transform and fault-retirement campaign. Delayed-start resolution and accepted-guest recovery also pass, but take 88.41 seconds. An owned-thread sample during that test records a journal-commit wait; syscall access is denied, so the exact call and causal explanation remain unknown. Strict workspace Clippy and global formatting still fail. The final corrected client facade passes 17 WASM panel cases (one ignored); an earlier mistyped filter ran zero cases and supplies no validation. The final client lint no longer reports the redundant facade conversions introduced by the boxing pass, but its other strict lint failures remain.

Structural checks pass 340 and source-guard tests pass 36. The retained checkpoint records subsequent build, IPC and strict workspace results with their source identities. Older evidence packets remain scoped to their recorded subjects.

Evidence: [protocol checkpoint](evidence/roadmap-2026-10-10/protocol-quality/checkpoint.json).
