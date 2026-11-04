# Desktop retained library batches

2026-10-12. Implemented scoped software; full roadmap PARTIAL.

The desktop Library batches tab supports the retained **audio recipe** workflow. Supply a local media folder, recursive selection preference and either the shared [six-axis authoring controls](PULSAR_LIBRARY_RECIPE_AUTHORING.md) or an explicit AudioBundleRecipe JSON file. Preparation freezes the validated recipe and selected source identities before possible Engine dispatch. Existing companion output directories are skipped. Opening the panel performs no generation or filesystem intake.

The panel lists owned batches, inspects generation and publication status, runs a bounded number of shared scheduler decisions, and opens retained projects through the existing project controller. Publication uses an explicit private workflow-state directory and the same journal and exact receipt identities as the CLI. Recipe changes after capture cannot silently change a retry. There is no new Engine command, Protocol schema or model default.

Unknown mutation acknowledgements retain the original request identity and captured arguments for exact retry. New mutations are disabled while recovery is unresolved; read-only inspection remains available. Typed current status can resolve acknowledged intake or cancellation under the original identity. Incomplete cancellation keeps the original cancellation request. Release uses the existing metadata-only Engine policy and preserves project/job/candidate/export records and output files.

Pause asks the client to stop before its next scheduler decision. The current operation may finish and an Engine worker may continue. Durable batch cancellation is a separate explicit action. Closing the panel requests local pause; it does not establish worker retirement, output rollback or physical stop. A pre-stopped run performs zero decisions and reports local_stop_requested without claiming completion or budget exhaustion. The ordinary CLI runner supplies an unstopped control and retains its behavior.

The controller runs I/O on one owned background thread at a time. Polling joins only a finished handle; duplicate controls are disabled while busy. Local validation failures before possible dispatch keep intake editable. A post-capture failure retains the frozen action rather than recapturing changed files. Response scope, schema, cancellation identity and false qualification flags are checked before display.

## Verification and limits

[Checkpoint](evidence/roadmap-2026-10-12/desktop-library/checkpoint.json): 366 Client library/integration tests pass, ten ignored; seven new panel tests cover headless egui input/disabled controls, pause, capture, recovery and response scope. Both strict workspace/all-target lint configurations, Windows GNU source check, native App/Worker/controller builds, formatting and architecture/source guards pass. Windows source checking retains existing warnings and is not native Windows qualification. No fresh whole Engine/Protocol runtime suite is claimed for this Client-only change.

The network-disabled native controller uses real SessionClient IPC and the actual Engine/Worker: two retained inputs, a preserved existing companion, a pre-stopped run, one decision, status, complete publication, catalog, durable cancellation, stopped run and metadata release. All twelve scripts match the retained authored PCM/timing oracles. Original cancellation and release receipts replay exactly after Engine restart without new records or journal mutation. The final CLI scheduler regression passes restart, per-entry publication, repeated completion, later input failure and journal preservation. Seven owned Engine processes, including the initial harness failure, are joined. All fixture background operations are joined.

Initial compile, recovery-test, lint and absent-table harness failures are retained in the packet. The harness now counts absent lazily-created batch tables as zero before first intake; no production storage rule was weakened.

Headless egui and native controller IPC do not establish rendered desktop acceptance. Visual/model folder workflow controls, broader input/provider/edition parity, crash/interruption cases, independent annotated media, useful models/defaults, platform qualification, real Handy synchronization and human/release acceptance remain open. No training annotations are created by this feature.
