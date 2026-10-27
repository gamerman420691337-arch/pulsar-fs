# ADR B3-132: Vision owner policy and client preparation

Status: implementation tested; roadmap PARTIAL; unqualified.

Extend trusted local setup with an optional measured vision projector. Preserve the encoding of legacy text policies. Give portable and absolute vision declarations explicit schema 3/4 identities; schema/capability mismatches refuse. Resolve every portable asset against the selected policy root, require normal relative components, and reject symlinks. Account the worst allowed RGB image and the paired projector in the same aggregate and child budget checks as inference. Publish a new private file only after metadata, identities, capacity and explicit host approval pass; never overwrite a policy.

On startup, register either a text or vision profile according to this exact capability. The policy remains a trusted host configuration, never a client request or project-effect approval. No runtime/model default is selected automatically.

Connect first-party CLI preparation to the Engine-issued image reference. Require exclusively creative scope and an exact registered vision profile before requesting normalization. Save the returned reference in the existing private, namespace-bound start intent. Keep inference and generation approvals separate, and reject expired or foreign references rather than silently creating substitutes.

Native verification uses an explicitly generated temporary absolute policy with the pinned public candidate, then the real Engine and current worker. The separately authored approval fixture stays separate from learned quality. Unit/controller tests do not establish GUI, physical-device, release or real-reference acceptance.

Implementation and remaining obligations: [vision policy and CLI record](../../docs/PULSAR_VISION_OWNER_POLICY_CLI.md).
