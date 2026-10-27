# Native prepared CUDA handoff, 2026-10-09

Status: authored CUDA overlap/refusal mechanism PASS; roadmap PARTIAL. Full live qualification remains open.

The current CUDA release executable passes two public-CLI campaigns using the authored 302-byte scalar RGB model. The positive private policy explicitly allows 256 MiB of GPU reservations; the refusal campaign retains the original 128 MiB limit. Each worker declares 96 MiB. Only the private positive policy's aggregate maximum changes; production/default owner policy remains unchanged.

| Case | Preparation outcome observed | Additional predecessor results while pending | Maximum client-observed result gap | Final CUDA runs/nodes |
|---|---:|---:|---:|---:|
| 256 MiB overlap | Ready at 8,864.86 ms | 265 | 97.65 ms | successor 61 / 183 |
| 128 MiB refusal | refusal at 5,165.12 ms | 154 | 37.60 ms | predecessor 234 / 702 |

The harness supplies paced fresh authored RGB frames to both epochs during preparation. The predecessor remains active and emits real CUDA results. In the positive case, validated Ready switches to the successor and retires the predecessor as Superseded; later predecessor ingress refuses. In the negative case, aggregate GPU admission exhausts and the successor fails while the predecessor remains Running and continues output. Both campaigns drain bounded queues, report zero stale final-epoch results and consume Finish on the remaining active epoch. The failed successor reports the existing outer Unavailable code with ResourceExhausted in its message; this does not claim a new public error classification.

Independent readback checks original manifest/receipt/trace byte lengths and SHA-256 values, summary equality, origin boot/identity/options, actual model_run count and exclusively CUDA nodes. It verifies all authored upload hashes/timestamps and observation oracles; maximum position error is 9.536743161842054e-8. Each owned Engine exits, custody is empty, authoritative jobs are zero and device effects are zero. Earlier predecessor output remains provisional in the positive case; its observations are not substituted for final original traces.

These are roughly 21- and 17-second scalar campaigns with explicit RGB ingress. Polling gaps are observations, not a statistical latency qualification or all-frame processing guarantee. Initial cold startup remains. Camera/encoded acquisition, live six-axis output, device dispatch, useful model quality and physical Handy playback are absent. No real media, training or user service contact occurs.

The checkpoint binds 410 source records: one new JS test and 409 unchanged from the prepared implementation seal. CUDA release build, syntax, two native tests without skips, 289 architecture checks and 36 source guards pass. The prior 625 CUDA Engine and 98 protocol tests are inherited through unchanged Rust source. The workspace gate remains FAIL (1,129 passed, ten failed, 63 ignored). A single failed import-restart test passes serially against the original temporary directory; this rules out the directory path alone as a sufficient reproduction, not concurrency or another cause. Prior failed logs remain intact.

Native CPU overlap, prepared public replay/restart, complete capacity cases, encoded/camera and long-duration capture-to-command, workspace fault diagnosis, quality/media, desktop combinations, installed Stash, offline/platform/vendor, Handy and human/release gates remain open.
