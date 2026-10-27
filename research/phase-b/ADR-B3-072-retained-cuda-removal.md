# B3-072: Explicit retained CUDA evidence removal

Status: implemented; scoped software and selected native CLI verification passed.

Retained originals need a deliberate disposal operation independent of capture leases. Use the existing authenticated project transaction and request journal, requiring both Read and ManageGrants. Bind a new request to the current revision, capture epoch and inspected manifest SHA-256. Commit removal and the receipt atomically; exact retries replay only under current authority.

Reuse bounded original validation before deletion. Preserve existing admitted immutable downloads rather than silently invalidating their byte ranges. Expose a CLI command with a required request ID and confirmation flag. Do not represent logical SQLite row removal as physical erasure.

See [operator behavior](../../docs/PULSAR_RETAINED_LIVE_CUDA_REMOVAL.md) and the [evidence packet](../../docs/evidence/roadmap-2026-10-08/live-cuda-evidence-removal/). Desktop disposal, capacity policy, secure erasure, broader recovery and release qualification remain separate open obligations.
