# Rebase regression executable attribution

The earlier portable-rebase receipt assigned its failing native run to the saved `pulsar-schema11-rebase-shadow` executable. That saved executable is actually byte-identical to the corrected final executable (`c946c48f4aec9f21f0371529bcacdcb8789f97b23746e9d1ff7c863b5f286c5c`) and passes the regression. The original run overlapped compilation and fixture changes. Its failure log remains a real observation, but the bytes loaded by its child processes were not measured and remain unknown.

The [audit packet](evidence/roadmap-2026-10-01/rebase-attribution-audit/validation.json) retains original receipt/observation bytes, corrected copies, raw build/test logs, the frozen fixture, commands and executable identities. It preserves the original failing log and source archive in their preceding packet. The corrected preceding receipt now points to this audit.

The retained pre-fix production archive has sorted-path/content SHA-256 `25544cf460bbc2ac63ceb6f2be1c6789cd4d156938486918133ec52c79ea1353`. All 153 files were extracted into a private directory and byte-compared. An offline locked build used a separate Cargo target without production edits. Its executable SHA-256 is `a3f24c0643627b20af89b7ce57fef4e7bfe2b0476fcf9dc0a4b503da0cc867f2`. The fixture SHA-256 is `fa33eb0b879cfaf35956755ffcb62068308a66a8c9dee180b42f8b754f3dac0e`.

| Executable subject | Same forged-link fixture result |
| --- | --- |
| Saved mislabeled executable | 1 pass, 0 failures, 0 skips |
| Isolated reconstructed pre-fix source | Expected negative: 0 passes, 1 failure, 0 skips; forged-link inspection returns success at line 121 |
| Pinned corrected current executable | 1 pass, 0 failures, 0 skips |

The reconstructed baseline and fixture were hashed before and after the negative run. The pinned corrected executable and fixture were hashed before and after the positive run. No intervening compilation or source edits changed them. Build context/pathnames can change executable bytes; the exact retained source identity is the baseline boundary. The pinned older readers were hashed around the positive run; the negative assertion occurs before their checks, and their bytes were not measured before that negative run.

Production and verification source did not change. The preceding ordinary full gate and seven supplemental Rust runtime cases remain their original executions; no new full-suite pass is claimed. These authored local fixtures do not establish installed Stash behavior, model/media quality, Handy operation, other platforms, independent verification or human/release acceptance. The full roadmap remains active and incomplete.
