# B3-073: Desktop retained CUDA evidence removal

Status: implemented; scoped software and selected native desktop-worker checks passed.

Expose the existing Engine disposal command through the background evidence worker. New requests require an inspected capture and explicit confirmation. Preserve one submitted intent independently of project navigation, and validate the removal receipt against that exact intent. An uncertain outcome offers exact retry rather than silently minting a new request or rebinding a revision/hash.

Keep the pending command visible after worker failure or project closure, with copyable CLI recovery and explicit local dismissal semantics. This first desktop control keeps intent in memory; durable recovery after desktop death remains open. Historical row removal is never presented as secure erasure.

See [behavior and limitations](../../docs/PULSAR_DESKTOP_CUDA_EVIDENCE_REMOVAL.md) and the [source-bound packet](../../docs/evidence/roadmap-2026-10-08/desktop-cuda-evidence-removal/). Native refusal checks preserve the original authored capture. Actual isolated-window QA and human acceptance remain unverified.
