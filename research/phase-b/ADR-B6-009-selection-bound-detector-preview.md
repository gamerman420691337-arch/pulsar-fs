# ADR B6-009: Selection-bound detector preview shares finite worker admission

Status: implemented; local verification receipt retained separately.

Use a dedicated pack preview command with the current manifest/revision fence, existing frame context and an explicit source or placement clock. Reuse finite preview decoding, placement/grant checks, worker confinement and output verification. Capture pack model/metadata through the same opened-descriptor admission as generation, then resolve the profile against the preview's actual resource budget. Supplier runtime bytes remain inert; execution uses the engine-pinned runtime.

Require project Read and ImportSource authority. The owner selection already records generic unqualified native-execution approval; this operation uses that same bounded offline analysis authority. Update the interface's approval description to analysis and generation. Pack intake, portable history and a historical selection receipt cannot approve this operation.

Recheck the exact current selection before admitting the worker and after its execution. Deactivation does not revoke captured inputs or forcibly stop accepted work, but its old-choice result is refused. Return choice, model and canonical profile identity alongside the validated preview. The CLI and desktop bind that envelope to their current selected profile and existing time/frame/view request. Temporal profiles cannot supply single-frame detector observations and remain pixels-only in the desktop.

Do not alter durable job schema or portable format for ephemeral preview. Stash batch admission uses its separate durable selection and recovery policy under ADR-B6-010-selection-bound-stash-batches.md. See ../../docs/PULSAR_MODEL_PACK_PREVIEW.md.
