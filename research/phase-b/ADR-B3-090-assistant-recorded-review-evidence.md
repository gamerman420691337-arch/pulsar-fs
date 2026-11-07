# ADR B3-090: explain recorded reviews through authenticated evidence reads

Status: implemented software increment; qualification incomplete.

Offer bounded current-master review records in Engine-owned assistant context, with optional explicit focus of up to eight exact record identities. Bound the stored review cell before capture and preserve absent confidence and unknown/conservative status. Treat reason codes as data. Reconstruct identical selected facts for generation approval freshness.

An ExplainReview proposal selects an offered identity. First-party clients read current ReviewStatus through existing authenticated authority and return the exact matching record after checking ownership, revision, terminal proposal and content identity. Do not accept model-authored reasons or confidence as evidence. Bind desktop cached results to the current view and selected attempt, preserving independent mutation retry identity. Empty-focus request serialization remains compatible with prior saved intents.

Candidate review selection, narrative quality, target and protected edit application, actual learned review selection and full product qualification remain open. See [behavior and evidence](../../docs/PULSAR_ASSISTANT_REVIEW_EXPLANATIONS.md).
