# Engine lint gate progress

Date: 2026-10-10. Full roadmap PARTIAL; strict engine lint remains FAIL.

A fresh all-feature/all-target strict Clippy run reported 73 distinct findings. This batch removes 50 of them across 25 source files, using verified source replacements and compiler suggestions. Redundant clones/conversions and parentheses are removed, production items precede test modules, C-string test arguments use typed C strings, repeated capture evidence rows and finalizer callbacks have named types, and fixed-size byte iteration uses the same as_chunks API already present in the project. The shifted-frame fixture iterates its exact eight authored shifts. A WASM publication test hook releases its lock before invoking its callback through an explicit local binding, preserving the previous lexical release behavior. No lint allowance or numerical/deadline change was added.

Final verification:

- All-feature engine tests PASS: 704 unit tests, 12 integration tests, 79 ignored qualification cases. Final unit suite took 14.72 seconds. The earlier intermediate test result is retained separately.
- Workspace all-feature/all-target check PASS, with existing warnings.
- Architecture PASS: 350 checks. Source guards PASS: 36 tests. Diff whitespace check PASS.
- Strict engine Clippy FAIL, exit 101: 23 distinct remaining findings; compiler reports 18 library and 18 test-target errors. These include argument-heavy interfaces, unused storage/capture/test helpers and the model-session backend size difference. No global formatting success is claimed.

[Checkpoint](evidence/roadmap-2026-10-10/engine-lint-batch/checkpoint.json) retains before/intermediate/final lint output, both test commands, final compilation, source hashes and patches relative to this turn's starting worktree. Existing native/startup/transport reliability failures, installed Stash, useful model/media quality, physical Handy, platform, rendered desktop/human and release gates remain open. This work does not replace those gates.
