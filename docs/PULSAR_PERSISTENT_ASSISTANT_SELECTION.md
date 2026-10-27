# Persistent assistant host selection — 2026-10-12

Roadmap status: **PARTIAL**. Explicit local assistant configuration now persists across launches; no unqualified default is automatically chosen.

After `assistant-setup` saves a private measured policy, run:

```sh
pulsar assistant-select --policy /absolute/path/assistant-policy.json --approve-host-inference
```

The local host validates the source policy, resource declaration and current companion Worker, then exclusively writes a private `assistant-startup-policy.json` under the configured Engine state directory. It prints that saved file's SHA-256. On subsequent startup, `EngineConfig::for_user` selects it when no explicit `PULSAR_ASSISTANT_OWNER_POLICY` override exists. Present invalid selection refuses startup; absence leaves the assistant disabled. Inference and project effects remain separately approved. Selection does not change a running Engine or initialize its database, sessions or authority.

To disable the saved choice for subsequent startup:

```sh
pulsar assistant-disable --expected-policy-sha256 SAVED_FILE_SHA256 --approve-disable
```

Disable requires the exact current digest, private owned bounded single-link regular bytes and unchanged file identity. It removes the selected file through a held directory and syncs the directory. Other files and explicit environment overrides remain separate. Selection never overwrites an existing file; disable the known choice before selecting a replacement. Write/sync uncertainty is reported instead of claimed as success. Corrupted files can be removed only with their exact current digest and safe metadata; unsafe/unknown files need owner inspection.

Assets within the configured data root retain portable schema 2/3 relative paths. External assets use schema 1/4 absolute paths and require explicit reselection if moved. Direct `EngineConfig::new` callers retain explicit configuration; startup discovery is limited to first-party host configuration. Linux support is scoped; other platform delivery and graphical settings workflows are open.

Fresh selected library results: **304 Clients PASS, 8 ignored; 837 Engine PASS, 70 ignored**. Five new Engine cases cover explicit approval, no overwrite, digest-bound disable, relocation/override, corrupt/unsafe selection, changed Worker, missing state and linked-file protection; one new Client case covers approved local-only dispatch. Both strict workspace all-target lint configurations, workspace all-target check and formatting PASS. Architecture 431 and source guards 36 PASS. This is not an all-feature runtime suite.

The initial ordinary-filesystem Engine run had 833 passing and four failures: two absent ONNX runtime lookup panics, native custody assertion and import timeout. Configured RAM runs remove those failures but initially expose six cache ancestor trust refusals because sandbox `/mnt` has unmapped ownership. Changing only the owned namespace mount/permissions to trusted directories yields the final 837-pass run. All failed logs are retained; no production timing or trust guard is weakened. The initial 12-policy-test binary predates the final directory-handle refactor; the complete final library executable covers it.

The actual updated candidate reuses 13 assets and replaces App/Worker, verifies all 15 members, runs offline from a path containing spaces and uses adjacent tools with no asset overrides. Its source policy is created by shipped `assistant-setup`, then selected by shipped `assistant-select`. With the selection environment variable absent, its Engine passes exact learned triangle, separate generation approval/replay, independent 17-action oracle, commit/undo/redo, export and stale-revision refusal. Shipped disable removes the saved choice; a second actual Engine reports no assistant profiles. Both Engines join, typed custody snapshots are empty, and the owned source policy is removed.

Thirteen additional actual CLI metadata/control checks pass for setup, approval, selection, overwrite refusal, stale/missing disable approval, corrupt/public/symlink startup refusal, exact disable, Worker mismatch and reselection. These use an explicitly nonexecuted model metadata fixture, establish configuration behavior and create no Engine database/session/capability or inference. An initial temporary runner syntax error is retained separately from its corrected successful run. Refused Engine startup may create its private guard lock; the retained inventories distinguish that lock from database/session/capability creation. These controls do not establish useful model quality.

[The evidence checkpoint](evidence/roadmap-2026-10-12/assistant-startup/checkpoint.json) binds exact final source and used binary identities to retained native results. Temporary runtime copies and state are removed; borrowed model/runtime assets and shared caches are preserved. Earlier unavailable-yaw substitution and unconfirmed-retirement findings remain open. Edition defaults/assembly, useful required providers/models, GUI/platform, real-media/training annotations, Handy 2 Pro firmware/credentials, physical-device and human/release gates are not closed by this increment.
