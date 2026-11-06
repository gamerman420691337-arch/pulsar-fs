# ADR B3-046: Bind CUDA metadata to the supervised capture owner

Status: Implemented prerequisite; production GPU admission remains disabled.

The loaded owner-policy builder previously rehashed the complete captured image in the authority process. Native capture already verifies the copy in a supervised confined process and retains the resulting image/storage owner. Repeating potentially gigabytes of content reads in the parent defeats that separation.

Add `LoadedCudaOwnerPolicy::worker_execution_supervised`, borrowing `SupervisedNativeDependencies`. Its private construction establishes completed copy verification; the builder checks completion length, exact declared policy/specification correspondence, profile/runtime/device/priority admission and current alias metadata. Shared metadata assembly preserves the transport dependency identities. The generic capture builder retains its full hash verification.

Metadata construction neither transfers custody nor reserves GPU capacity. A successful build does not prove current bytes unchanged: same-length in-place edits can retain readonly metadata and inode identity. The confined CUDA bootstrap must continue freshly hashing mounted aliases before native loading. Its existing hashing and mounted device checks are unchanged.

A native regression performs an actual supervised capture of six authored non-library files, exercises binding/refusal cases, observes retained storage and explicitly demonstrates the metadata/content boundary. It does not load native libraries, mount a GPU or perform inference. Production launch still needs fresh measured device memory, aggregate admission, owned mount/device/image lifetimes, durable launch, transport/result verification and recovery.

Evidence: [supervised policy checkpoint](../../docs/evidence/roadmap-2026-10-07/supervised-cuda-policy/checkpoint.json). Model, real-media, physical Handy, platform and human/release gates remain independent.
