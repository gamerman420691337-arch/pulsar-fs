# Held directory identity at worker registration

Status: native defect reproduced and fixed; 483 native engine cases, 469 CUDA-feature cases and seven focused application cases pass.

Registration previously obtained a cgroup identity from the current pathname and recorded it without comparing the directory already held by the attempt guard. A native fixture reclaims the original empty group while retaining its directory FD, creates a fresh group at the same pathname, and asks the fence to register the original object. The preceding implementation accepted the replacement. The retained failed test is in `docs/evidence/roadmap-2026-10-01/cgroup-registration-binding/initial-reproduction/`.

Registration now compares device and inode from the validated pathname directory with the attempt's held directory FD before changing either the in-memory roster or durable ledger. The unchanged regression requires refusal, unchanged ledger bytes, an empty in-memory roster, and preservation of the replacement directory. This is a controlled native replacement fixture; it does not prove absence of hostile concurrent pathname races.

Host-local fence format remains 2, database schema remains 17 and portable format remains 9. Current-source native engine, feature and focused application gates pass without failures; the application cases have no skips. The preceding full workspace and 104-case JavaScript results belong to the archived reclamation subject and must not be relabeled as tests of this edit.

The create-before-register crash window is separate. This check does not journal creation, reclaim unregistered orphans, bound host IO, establish exhaustive crash/power-loss recovery, enable production GPU admission or qualify models, platforms or the Handy.
