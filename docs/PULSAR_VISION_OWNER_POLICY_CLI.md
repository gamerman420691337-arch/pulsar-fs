# Vision owner policy and CLI preparation

Status: software increment implemented and tested; full roadmap PARTIAL; unqualified.

`pulsar assistant-setup` now accepts an optional `--projector /absolute/path`. It measures the projector alongside the runtime, model and companion worker, checks private publication and conservative aggregate RAM/CPU capacity, and writes a new policy without starting an Engine or running inference. Portable vision policies use schema 3; explicit absolute-asset vision policies use schema 4. Text policies retain schema 1/2 and omit the new field. Portable resolution confines the projector to the selected installation root and refuses symlinks or traversal. Startup advertises the exact vision capability only after the selected private policy validates and matches the captured companion worker.

Select the private policy through `PULSAR_ASSISTANT_OWNER_POLICY` at Engine startup. This setting is a configuration path, not an API credential. No global policy, model default, user key or device setting was installed by this increment.

For an imported single-frame source, use:

```sh
pulsar assistant prepare PROJECT --profile PROFILE --revision REV \
  --image-source SOURCE_VERSION --creative-scope /absolute/scope.json \
  --prompt 'Suggest an image theme' --request-id REQUEST \
  --output /absolute/private/intent.json
pulsar assistant start --intent /absolute/private/intent.json --approve-inference
```

Preparation requests supervised still normalization and saves the exact Engine-issued context, profile and namespace in the existing bounded private intent. It does not run model inference. Image preparation requires exclusive creative scope and a vision profile; text profiles reject image sources. The shared transport and saved-intent validator check project, session and revision. Restart or eviction can expire a prepared reference; automatic re-preparation or profile substitution is not performed. Generation remains a separately reviewed action.

## Verification scope

Default Engine 840 PASS/67 ignored, final clients 255 PASS/5 ignored and all-feature Engine 854 PASS/93 ignored. The native owner-policy still case passes in 76.96 seconds. Both strict workspace/all-target lint configurations, final default paired build, format, whitespace, 403 structure checks and 36 guard tests pass. Library tests use the bounded network-isolated RAM harness; persistent-disk performance and durability remain unqualified. Policy tests cover portable relocation, projector symlink/traversal refusal, missing/relative assets, CPU/RAM rejection, schema/capability consistency and refusal to overwrite. Client tests cover CLI scope/approval controls, exact saved references and foreign namespace refusal. The native authored still test now starts from a generated private vision policy and then exercises normalization, inference, report bindings and stale-byte approval checks. It is a real Engine startup/inference check, not a full CLI subprocess or real desktop qualification.

The initial application compile found a duplicated setup field; the initial client fixture was inserted outside its test module. Strict lint found an owned comparison in the parser test. These failures and corrections are retained with exact before/after subjects.

## Remaining work

[Desktop image preparation and saved-request review](PULSAR_DESKTOP_IMAGE_ASSISTANT.md) are implemented and controller-tested; interactive GUI qualification remains UNKNOWN. Dedicated image-conditioned candidate/receipt lineage remains open. The catalog is ephemeral; crash reconciliation and persistent-disk performance remain unqualified. Combined text/vision startup configuration, installed/offline defaults, useful-model acceptance and real-reference accuracy still require work or evidence. No annotations or training were fabricated. Handy 2 Pro physical qualification, platform, human and release gates remain open.

Decision: [ADR B3-132](../research/phase-b/ADR-B3-132-vision-owner-policy-and-cli.md). Preceding binding contract: [authenticated image requests](PULSAR_AUTHENTICATED_IMAGE_ASSISTANT.md).

Exact subjects, negative results and logs: [checkpoint](evidence/roadmap-2026-10-11/vision-owner-policy-cli/checkpoint.json).

Follow-up: [retained image generation origin](PULSAR_IMAGE_GENERATION_ORIGIN.md) implements scoped receipt/candidate/object-capture retention. Full portable lifecycle and presentation/qualification gates remain open; earlier evidence stays historical.
