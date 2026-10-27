# ADR-B3-125: native CPU generation and assistant contention

Status: accepted scoped verification; roadmap PARTIAL.

The memory handoff and capacity recovery fixtures used inert workers and authored accounting claims. Add an explicitly ignored Linux test with the actual confined worker, permitted pinned YOLO11 CPU generation and pinned Qwen3/llama.cpp assistant. Configure a 3 GiB pool against generation and assistant claims totaling 3.5 GiB. Run in an owned network-isolated namespace with delegated cgroups and private tmpfs state.

After observing running generation with pending native custody, start an authenticated assistant request. Observe the journals directly; never request assistant status or manually resume generation. Require the original attempt to remain assistant_paused, the assistant to complete with stride 8, and the same job to produce a candidate through a different attempt. Validate both exact initial checkpoints; compare the complete manifest basis excluding only attempt ID and output directory. Require identical generation operation and unchanged master motion/revision. Cancel owned work and confirm worker/custody retirement before fixture removal.

The test passes on the recorded source/binary subject. It verifies the complete native CPU orchestration path at the immutable initial input boundary. Pending custody does not prove that the first generation attempt completed an inference before cancellation. Neither capacity claims nor one reported assistant peak measure aggregate application memory. No partial decoder progress, GPU/live-device continuity, useful model/media quality, default model or release acceptance is established.

See [capability and source-bound evidence](../../docs/PULSAR_NATIVE_ASSISTANT_HANDOFF.md).
