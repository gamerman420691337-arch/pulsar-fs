# ADR B3-137: explicit persistent assistant host selection

Status: implemented; scoped library, strict lint and actual bundled CLI verification PASS. Broader qualification remains open.

The local setup command can measure and save a policy, but each launch currently needs an environment selection. Core/Complete settings require a durable explicit choice without choosing an unqualified model automatically.

Add local host-only `assistant-select` and `assistant-disable` commands. Selection requires explicit host inference approval, validates an existing private policy and current Worker, and writes a new private startup policy without overwrite under the configured state directory. Preserve portable relative assets when they are contained in that data root; otherwise retain explicit absolute assets. Environment selection remains an explicit override. Missing saved selection means no assistant; present invalid selection refuses startup. Direct EngineConfig callers remain explicit.

Disable requires the exact saved policy digest and explicit approval, verifies private single-link regular file identity, removes only that selected file and syncs its directory. It changes subsequent startup, never a running Engine's authority. Uncertain storage failures retain an inspectable result and report failure. Host settings do not grant project effects, run inference, acquire a client capability, connect a device or establish useful model/default acceptance.

Verify selection across fresh launches, override/absence, corrupted or unsafe selection, changed Worker, no-overwrite, stale disable digest and approved disable. Preserve shared state and test through private owned roots. Broader GUI, portable edition, platform, real model/media/device/human gates remain independent.

Evidence: [persistent host selection](../../docs/PULSAR_PERSISTENT_ASSISTANT_SELECTION.md). A running Engine retains its original profile; these local commands change subsequent startup. Saved state-contained assets remain portable, while externally located assets retain absolute paths and need explicit reselection after relocation.
