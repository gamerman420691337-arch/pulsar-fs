# ADR B3-100: local assistant host setup

Status: implemented; scoped configuration/lint/library/CLI verification PASS. Scope: Linux trusted executable host setup.

Problem: approved offline assets require hand-authored policy JSON and identities. Policy relocation alone does not provide a first-party configuration path.

Decision: add `pulsar assistant-setup` grammar to the clients, but intercept it in the executable composition root before Engine startup. The clients neither import the Engine nor write host policy. The trusted host API measures selected worker/runtime/model files through single opened regular handles, bounded streaming buffers and stable metadata checks. Validate existing profile/deadline/aggregate admission before publication. Default to schema-2 policy-folder-relative assets; an explicit absolute-assets option retains schema 1.

Require explicit host inference approval, absolute input/output paths and an existing owned parent not writable by other users. Open the parent without following its final symlink and publish through its held descriptor with exclusive no-follow creation followed by descriptor-scoped0600 permissions before writing. Existing files never change; uncertain writes/syncs retain only the newly created private file for inspection. No keys, tokens, Engine state, network, inference, project effect or automatic policy activation is involved. Future registration still captures/verifies the measured identities and inference/effects keep separate approvals.

This configures declared local assets, not model compatibility/quality or an offline Core/Complete distribution. Actual shipped defaults, extraction, OS/vendor/device/media/native and human/release gates remain independent.
