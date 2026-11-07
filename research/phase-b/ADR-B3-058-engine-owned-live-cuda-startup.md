# ADR B3-058: Engine-owned private live CUDA startup

Status: Private Engine startup and authored native execution verified. Public live routing remains disabled.

Construct a live GPU stream from the Engine's configured owner policy, resource pool, actual durable worker fence and authenticated capture request. Do not manufacture an offline generation job. Require explicitly declared Start/Continue options matching the model path, profile bytes, clock and lease, plus the current Engine boot and project revision. Bind the stream to its opaque RGB runtime scope. Derived-anchor continuation is not accepted by this helper until its admitted options can be bound explicitly.

Authorize and commit format-4 creation admission before capture. Perform native capture, worker/model staging and GPU observation outside database/registry locks. Use the original local lease end for capture and staging. Convert it into CLOCK_MONOTONIC relay time by sampling that clock before measuring the remaining local interval; sampling overhead may shorten the deadline but cannot renew it. Recheck authority/revision/fence after preparation and again after measured GPU admission, before persistent stream launch. Keep cooperative claims and image/device/staging ownership in existing custody through final verification or retirement.

The caller still owns active capture-registry membership, current lease checks after Ready, issued-work freshness/completion, provisional reporting and final evidence persistence. Startup is not current acceptance authority. Public routing must not bypass these obligations simply because the private helper can construct a worker.

The native case uses the Engine's default two worker slots, actual captured 75-library image, CUDA/NVML observations, a 96 MiB interactive claim and one loaded authored model. Three issued-frame exchanges finish with exact transcript/original trace evidence, with zero offline jobs. Boot/clock mismatches and cancellation preserve the pre-start fence bytes and storage charge. All custody retires. The shared offline generation/resume fixture is rerun after extraction of its common setup.

An initial fixture incorrectly supplied 16 MiB output admission to a profile declaring a 64 MiB minimum. It refused before capture. Retain that source/log; use the declared minimum rather than weakening profile admission. See [implementation and evidence](../../docs/PULSAR_ENGINE_LIVE_CUDA_START.md). Fastest-preset six-axis, real-media/model quality, measured live latency, physical Handy, supported-platform and human release gates remain open.
