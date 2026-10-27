# ADR B3-056: Bind persistent CUDA streams to issued RGB work

Status: Private implementation and eight authored native cases verified. Public GPU live integration remains open.

Bind each private parent CUDA stream to a process-local scope minted by `LiveRgbRuntime`. The scope carries the runtime's private random owner, live identity and presentation clock; it has no serialization path. Starting a stream requires the exact manifest identity/clock. Estimation accepts only a borrowed `LiveRgbWork` from that owner, constructs metadata from its declaration and hashes its actual pixels. Raw metadata/pixel exchange stays private. Borrow the two pixel slices directly rather than allocating a concatenated parent buffer.

The scope is an ownership binding, not proof of current authority or freshness. It does not retain pixel admission, keep a retired runtime active or authorize accepting an estimate. The caller retains issued work and independently completes it against the runtime's actual clock. Future Engine integration must retain revision, revocation, cancellation and retirement checks.

Strengthen transcript preflight on both sides: a later pair cannot move its previous frame backwards; reuse of the prior current frame requires the same pixel identity and equivalent rational source time. A skipped previous frame requires increasing source time. Sequence and current frame/time must still increase under the existing bounds. Invalid preflight permanently poisons the stream. The wire fields, transcript domain and summary layout do not change.

The authored native campaign uses actual runtime-issued frames and one loaded CUDA model. Three exchanges finish with an exact transcript, including one result independently classified stale under authored capture/completion clocks. Other cases exercise parent replay, same-metadata foreign work, cancellation, original-deadline idle expiry, worker-side replay, changed pixel hashes and mismatched Finish. Idle tests call no pipe monitor while waiting for the independent watchdog, and assert image/storage/GPU claims remain held after child exit until full retirement. Worker-side cases require failed child status and EOF without a successful reply. All eight cases assert full accounting, staging, capture and fence cleanup.

See [implementation and evidence](../../docs/PULSAR_LIVE_CUDA_ISSUED_WORK.md). This does not enable public GPU live, qualify measured capture latency or six-axis output, resolve the retained parallel fence failure, or establish model/media, Handy, platform or human release acceptance.
