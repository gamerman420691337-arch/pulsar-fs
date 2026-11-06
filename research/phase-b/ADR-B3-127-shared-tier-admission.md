# ADR-B3-127: shared executable tier admission

Status: implemented software increment; broader M3/release qualification open.

Visual Maximum became executable, but the desktop selector omitted it and Stash batch/migration names still rejected it. Unsupported temporal/model-free Maximum could also create a generation attempt before the worker refused. Add one GenerationSettings execution-support check and use it in fresh Engine generation admission, worker execution, desktop preparation, selected-pack command construction, Stash client submission and captured batch start/migration admission.

Separate execution support from structural serialization/archival validity. Do not add wire fields, change WorkerRequest structural validation or rewrite historical manifests. The public Engine continues to admit fast/default/maximum/quality names; economy remains a stored-worker alias. Maximum requires an explicit non-temporal detector declaration. Unsupported non-stroke visual projections and model/input mismatches refuse before generation job/attempt creation. Immutable model capture and bounded metadata admission may precede that check.

Expose Maximum in the desktop alongside Fast and Default when a declared detector is available. Keep a selected tier if the model changes; refuse incompatible preparation rather than switching the user's tier. Selected-pack approval, owner/current selection fences, explicit candidate commit and model qualification remain independent. Stash batch/migration preserve the original preset name, validate the resolved model and retain existing CAS/ledger/publication controls. Source-specific pack resolution may follow read-only discovery; model-free Maximum refuses before discovery.

Authoritative checks cover the support matrix, unchanged serialization, exact selected-pack tier, temporal refusal without selection replacement, zero jobs/attempts on unsupported fresh generation, and refused batch start/migration with unchanged retained ledger. A separate pinned native detector case executes all three visual plans and binds receipts to candidates. No rendered GUI, installed external Stash, useful/default models, natural-media quality, GPU/vendor/platform, physical or human/release qualification follows from these checks.

See [scope and source-bound evidence](../../docs/PULSAR_TIER_ADMISSION.md).
