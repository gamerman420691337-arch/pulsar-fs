# ADR B3-089: bind assistant proposals to explicit generation approval

Status: implemented software increment; qualification incomplete.

Generation settings are request data. Apply a completed sample-stride proposal through an explicit authenticated generation command, rather than assigning model output directly to client state or granting effects through inference approval.

Bind approval to retained attempt/context/proposal identities and require current Read/Edit/Generate, revision, session/project and Engine lifetime. Use the same exact facts for inference capture and current-context comparison. Derive only the existing Generate command and proposed stride; preserve the original approval command as its receipt/fingerprint subject. Recheck after capture inside the publishing transaction. Existing source/model/native/admission checks and separate protected candidate commit/history remain authoritative.

First-party CLI approval is explicit; pending retries keep original identities and complete options. Saved declarations and desktop controls are implemented in ADR B3-091. Full saved approval lifecycle, actual learned-to-generation runtime probes, target/edit application, learned review-selection qualification and release/model/hardware/human qualification remain independent work. Bounded recorded-review selection/display is implemented separately in ADR B3-090. See [contract and evidence](../../docs/PULSAR_ASSISTANT_GENERATION_APPROVAL.md).
