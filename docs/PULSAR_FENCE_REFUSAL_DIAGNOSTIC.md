# Worker-fence refusal diagnostic, 2026-10-09

Status: diagnostic improved; four full Engine runs PASS; original workspace failure remains unexplained; roadmap PARTIAL.

The malformed-ledger test still requires each exact refusal reason and verifies original bytes remain unchanged. Its assertion now includes both the expected reason and the observed error. This is a cfg(test) diagnostic change only: production lease/refusal handling, retry behavior, and deadlines are unchanged.

The full default Engine passes 616 tests with 54 ignored at 32 threads. Three subsequent 64-thread runs each pass the same 616 tests with 54 ignored. The preceding workspace failure is not reproduced. These runs neither identify its cause nor prove it resolved. No speculative lease/retry fix is applied. The original red workspace log remains in the preceding prepared-actor checkpoint.

The [checkpoint](evidence/roadmap-2026-10-09/fence-refusal-diagnostic/checkpoint.json) seals 413 source records, one test-only changed path and 412 unchanged, source archive, original file, baseline-relative diff and all four completed run logs. The production prefix is byte-identical. No new CUDA/native/release qualification is claimed. All broader live/model/media/Stash/platform/offline/Handy/human gates remain open.

Updated owner disk rules apply going forward. `/mnt/4tb` is 94% used, so jobs that write over 10 GB are not started. Diagnostic outputs created earlier this turn are moved into project evidence and both the original and replacement per-run scratch directories are removed. Shared existing build caches are preserved; they are not disposable per-run outputs.
