# Client storage and quality verification

Strict all-target Clippy passes for the client and application packages with `--no-deps -D warnings`; engine dependency warnings and full workspace strict lint remain open. Default all-target checks pass. The full roadmap remains incomplete.

Large client reply variants now own boxed payloads, matching the corresponding protocol representation. The wire-to-client adapter forwards compatible boxes instead of copying the payload out and boxing it again. Hydrated project/candidate structures retain their fields and Arc program ownership; controllers that own those values receive the unboxed value. Tagged JSON remains unchanged. This changes Rust constructors for bundled/native client consumers.

The measured client reply occupies 200 inline bytes; a regression test caps it at 256 and reproduces the frozen package-download encoding. Private editor reply/action limits are 256/512 bytes, with a test in the existing controller suite. Heap payload ownership remains; these are neither total-memory nor throughput claims.

A private FileGenerationInput groups existing file-generation arguments, retaining source import, independent-audio association and pack-choice behavior. Mechanical lint changes preserve independent branches and validation. Unused test-only request/call/handshake helpers were removed. No warning suppression, authority expansion, relaxed resource limit or extended timeout was introduced.

The full client library run passed 181, failed the same four concurrent journal cases and ignored four. The shared journal primitive and the four failing recovery modules/cases match their pre-edit source hashes. The new response-storage/encoding cases pass two. The paired WASM build passes, and current native process runs pass portable desktop (four), Phase A (three, two historical readers ignored) and large motion (seven). Earlier startup failures remain recorded; this run does not establish reliable cold startup or full parallel durability.

The parallel package-export run passed three, failed two with transport timeouts and ignored one >1 GiB qualification case. All ten active package-import cases failed before engine readiness; three qualification cases were ignored. Owned engine threads showed journal-commit waits during export. One isolated export and one isolated exact-closure import pass under synchronization-only tracing. Those passing traces capture 149/322 completed sync calls, with maxima 0.227/0.406 seconds; they do not identify the failing parallel calls. No complete package, concurrency or startup gate is closed.

Structural checks pass 340 and source-guard tests pass 36. Global formatting still fails. Engine, protocol, core and worker Rust/Cargo source identities match the preceding protocol checkpoint; no engine/storage/worker implementation was changed in this increment. Package transfer results and owned wait-state samples are retained in the checkpoint. No credentials, real-media/model qualification, live Stash publication, device actuation or human acceptance was performed. No annotations exist; the owner instructed software work to continue.

Evidence: [client checkpoint](evidence/roadmap-2026-10-10/client-quality/checkpoint.json).
