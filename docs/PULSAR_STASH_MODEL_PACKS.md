# Owner-selected model packs in Stash batches

Stash batch generation can now use the exact current owner-selected offline pack from the CLI or desktop. Import remains inert; select a pack through the separate explicit unqualified native-execution approval before submission. The desktop uses its Generator pack choice/profile. The CLI queries the current owner selection and refuses a different requested manifest digest.

```sh
pulsar stash generate-missing --url http://localhost:9999/graphql --mappings mappings.json --model-pack EXACT_MANIFEST_SHA256 --detach --request-id retained-batch-id
pulsar stash batch-status retained-batch-id
```

Keep using credential environment names and explicit filesystem mappings. The engine prepares and checks the selected pack before discovery, records its exact choice and typed profile in batch intent, and rechecks selection in the transaction publishing that intent. A pack cannot be combined with a file model. Each scene submits `GenerateFromModelPack` with the recorded choice/profile; it never substitutes a loose model path. The existing internal permit binds the exact request and expected model digest, while current project Generate and ImportSource grants still govern admission. Jobs capture the model plus manifest/profile/approval dependencies through the same confined CPU generation path as standalone generation.

Deactivation affects future job admission. A batch waiting to generate retains the refused phase and becomes `needs_attention`; an already accepted job retains its captured inputs and can be reconciled. Reselecting even the same digest creates a new revision and cannot renew the old batch intent automatically. Resume retains the old command and request ID. Explicit stopped-ledger migration is required to choose the new approval.

Migration JSON contains `preset`, `settings`, an absolute `model_path` or null, and optional `model_pack` with `manifest_sha256` and `selection_revision`. Null file model and omitted/null pack explicitly select no custom model. To choose a pack, use null `model_path` and the exact current choice; any supplied profile/input must agree with the admitted profile. Migration rechecks current approval and required future project grants, retains prior options/choices/model digests and phase IDs, reconciles an accepted generation receipt first, and supersedes only unaccepted generation. It remains paused until explicit resume. Unknown publication/scan/tag effects are unchanged.

```sh
pulsar stash migrate-batch retained-batch-id --expected-ledger-sha256 EXACT_OBSERVED_LEDGER_SHA256 --generation generation.json --request-id retained-migration-id
pulsar stash resume-batch retained-batch-id
```

Batch report schema 4 identifies the recorded generation choice, not current owner status or approval of every historical job. The first stored pack batch or pack migration establishes database schema 13 atomically. Current startup validates batch layout/choice/profile intent through its read-only database probe before writable SQLite startup or scheduling. Its SQL query excludes a ledger cell above 16 MiB before allocating/deserializing that string; an explicit oversized-cell refusal case covers the bound. Selection changes, placement initialization, interval recovery and portable operations preserve that boundary. Portable format remains 9; portable project history does not contain executable batch authority.

The [local receipt](evidence/roadmap-2026-10-01/stash-model-pack/validation.json) binds exact source and executable bytes. The authored CPU/loopback fixture queues deactivation during held source capture, refuses future generation without a new job/attempt, restarts, observes exact schema-12 reader refusal with unchanged database main/WAL bytes, and confirms a new selection alone cannot renew the old phase. Explicit replayable migration retains the old choice/request, admits a new choice and completes generation and no-clobber local publication. Final deactivation/restart preserves schema 13 and the finished batch. There are no remote mutations; scan/tag integration retains its separate existing scenarios. Synthetic unit receipts additionally check accepted pack generation reconciliation without replacement authority; they do not prove an actual lost worker acknowledgement.

Two new fixture failures are retained: comparing admitted `f32` scale with the supplier's JavaScript number, and synchronously blocking the mock server while deactivation waited for capture's artifact gate. Corrections changed only fixture expectations/orchestration; no deadlines or production behavior were relaxed.

Installed Stash interoperability, native-window usability, model/real-media quality, Handy, trusted publishers/rights, distribution, other platforms and human/release qualification remain open. Scripts and batch reports remain unqualified. Training and sterile evaluation were not rerun.
