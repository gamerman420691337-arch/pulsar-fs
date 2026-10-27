# P1D-008: preserve unresolved export recovery identity

Status: **Implemented; affected local tests pass. Current external review unavailable.**
This is a successor to the seven-repair checkpoint, not whole-P1d or Good-phase completion.

## Cause and fix

An unrelated recovered export status replaced the selected unresolved Start request, project and revision. A subsequent fresh draft could then preserve only the unrelated export.

The panel now checks all three binding components before adopting a status while an attempted Start is unresolved. Unrelated results occupy one read-only inspection slot. They cannot replace the selected workflow, permit a fresh draft, or alter its download request/destination. The original exact-replay button stays available. A matching response resumes normal status handling. Ordinary recovery without an unresolved Start is unchanged.

This intentionally defers switching to the inspected export until the original Start is resolved; inspection itself remains available. No recursive recovery graph, new authority, hidden cancellation or request-ID substitution was introduced.

## Current evidence

Source input SHA-256: `f705c4a960c0a64d9301842996c2d72107ecabae58ddc9f49c032e60c5a39ed1` (210 declared entries).
Changed file: `crates/pulsar-clients/src/portable.rs`, SHA-256 `161e6c47d6aeb400bccc99e204876b33604d7fa106f884484161ccfb99445003`.

- Portable-controller suite: 19 passed, exit 0.
- Full client suite: 85 passed, exit 0; includes those 19.
- Portable process target: 4 passed, exit 0; two real-engine scenarios and two oracle tests.
- Two added regressions exercise actual panel polling and headless egui. They check each mismatched binding component, visible read-only inspection, blocked fresh-draft action, exact outbound replay despite a changed current project, matching terminal resolution, retained prior context, and ordinary recovery.
- No separate pre-fix RED execution is claimed. The preimage and patch are retained.
- The prior 559-execution full gate belongs to the prior source fingerprint; it was not rerun or relabelled as current.
- External `review_diff` returned `Transport closed`, with no verdict. The prior patch's PASS WITH NITS does not apply to this patch.

Commands:
```sh
cargo test -p pulsar-clients portable:: --offline --locked -- --nocapture
cargo test -p pulsar-clients --offline --locked
cargo test --test portable_desktop_process --offline --locked -- --nocapture
```

[Receipt](evidence/phase-b-portable-project/p1d008-repair/receipt.json), [source manifest](evidence/phase-b-portable-project/p1d008-repair/source.json), [patch](evidence/phase-b-portable-project/p1d008-repair/repair.patch), and raw logs are retained together.

No commit, push, physical actuation, native-window qualification or whole-phase completion is implied.
