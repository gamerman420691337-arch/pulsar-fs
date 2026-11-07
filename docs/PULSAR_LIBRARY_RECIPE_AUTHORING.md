# Desktop library audio recipe authoring

2026-10-12. Scoped implemented software; full roadmap PARTIAL.

The Library batches tab now offers direct six-axis audio controls, reusing the single-file Audio generator's drafts and checked Protocol recipe. A recipe JSON file remains an explicit alternative. Each fixed axis exposes envelope/onset/8 Hz oscillation mode, full/bass/mid/high band, RMS threshold, amplitude and offset. Invalid combined range or non-finite values refuse request creation. These are synthesized mappings, not observed physical axis motion or qualified creative defaults.

A checked recipe snapshot is taken on capture. Folder discovery then runs in the existing background controller and freezes the selected inputs before possible Engine dispatch. After capture, changes in the editor cannot change the pending request or its exact retry. A local discovery failure dispatches nothing and keeps the form editable. An unknown Engine acknowledgement retains the complete Prepare options and original request ID, disables replacement mutations and permits existing read-only recovery controls.

Both file and direct-editor intake share the same discovery/companion-preservation helper and existing LibraryBatchPrepare command. The direct editor writes no intermediate recipe file. The existing single-file Audio generator uses the same recipe snapshot and controls. Imported recipes are canonicalized into Axis::ALL order while preserving every axis's mapping, mode and analysis values. Recipe channel ordering is not a semantic axis association.

## Evidence and limits

[Checkpoint](evidence/roadmap-2026-10-12/library-recipe-controls/checkpoint.json): 370 Client library/integration tests pass, ten ignored. Four new tests cover the actual drawn batch capture button and frozen retry, no-dispatch discovery failure, snapshot equivalence/invalid settings, and reordered recipe import. Both strict workspace/all-target lint configurations, Windows GNU source check, native App/Worker/controller build, formatting and architecture/source guards pass. No new Engine/Protocol/storage contract or fresh whole Engine/Protocol runtime suite is claimed. Windows source warnings remain and do not establish native qualification.

The actual network-disabled native controller passes both direct-editor and recipe-file workflows: two retained sources, preserved existing companion, local pre-stop, bounded generation, status, complete publication, cancellation, stopped run, release, restart and exact cached replay. All 24 outputs match the twelve preceding independently authored PCM/timing oracles. Seven native Engine instances, including the initial failed fixture and preceding file regression, are joined. Test/application credentials are confined to owned temporary RAM; borrowed runtime/model payload identities remain checked and shared caches are preserved.

The initial editor fixture failed with `Error: editor recipe mismatch` because it compared differently ordered channel arrays. The final assertion checks each fixed axis's complete settings and the new reversed-order import test verifies the behavior. The failure, prior native identities and corrected subject remain in the evidence packet.

Headless egui input plus native IPC do not establish rendered desktop acceptance. Visual/model retained folder workflows, broader input/provider/edition integration, interruption/platform/model/media/device/human and release gates remain open. No missing annotations, useful model defaults or physical Handy evidence are manufactured.
