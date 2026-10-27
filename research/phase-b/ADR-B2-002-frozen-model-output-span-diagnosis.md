# B2-002: Frozen temporal output-span diagnosis

Status: Diagnosed on retained sterile output; quality remains failed.
Date: 2026-10-08

## Finding

The zero-match result is reproducible from immutable saved candidate/reference data. The matcher self-oracle succeeds. Direct CPU model replay reproduces every production inferred position exactly. The full model output span is smaller than 80% of the smallest reference stroke travel, so no segmentation or retiming of these positions alone can meet the existing travel condition. This proves a recorded output-range deficit, not a universal model defect or independently verified physical motion mismatch. All original frame timestamps are represented at stride one; visual/script alignment remains unverified.

## Decision

Do not change the matcher, stretch amplitudes, retime references or tune against sterile evaluation. Continue model capacity and label-consistency work on permitted live training/development data, preserving existing negative mean/median comparisons and consumed-split status. Keep the sterile fixture evaluation-only and all models unqualified until actual independent acceptance evidence exists.

## Evidence and scope

See [the diagnosis](../../docs/PULSAR_GENERATION_QUALITY_DIAGNOSIS.md). No production/model changes, training, new confined worker campaign, physical actuation or release qualification occurred. The direct cross-path replay uses a different cached Python runtime image; exact agreement is scoped to these 170 positions. The initial diagnostic FFmpeg failure is retained separately. Broader model, real-media, product, platform, Handy, human and release gates remain open.
