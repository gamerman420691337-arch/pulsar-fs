# Source snapshots and retained evidence

Git contains the development source, tests, scripts, research and documentation.
Large generated qualification payloads remain on the evidence host, outside the
source snapshot: fixture databases/session files, executable/runtime copies,
decoded media, generated traces and local artifact symlinks are not automatically
published. Existing tracked historical fixtures remain tracked.

The execution ledger retains exact evidence paths, hashes, assertions and scope.
Run its `--audit` command on the evidence host; another checkout may not have
those local payloads. Missing payloads grant no qualification or acceptance.

Current durable evidence is kept under
`/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/`. Older local
`docs/evidence` records and links are preserved on this workstation. Ignore
rules prevent new generated payloads and temporary harness/backups from entering
a normal source commit; they do not delete or rewrite the retained evidence.
