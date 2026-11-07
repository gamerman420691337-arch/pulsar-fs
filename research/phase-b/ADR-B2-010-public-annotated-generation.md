# ADR B2-010: explicit public annotated file generation

Status: implemented locally; outputs remain unqualified.

## Context

Conditional local storage, strict completion correspondence and portable model/clock closure now exist. The remaining blanket submission guards prevent using them through first-party clients.

## Decision

Admit direct file-backed CPU annotated generation with explicit model/profile and source-bound view settings. Check imported source identity and current revision before model snapshotting. Preserve worker admission, local annotated membership, completion verification and portable reader barriers. Keep generic Stash/model-pack/CUDA admission separate.

Expose bounded source-view loading in CLI and desktop and a distinct CLI profile declaration. Reject competing declarations rather than choosing one automatically. Preserve unqualified result/review status.

## Evidence and remaining obligations

[Public annotated generation](../../docs/PULSAR_ANNOTATED_PUBLIC_GENERATION.md) records actual public Engine generation/replay/commit/restart/capture/clone and a shipped CLI/local-IPC/native-worker campaign with independent unrounded Python position correspondence. Desktop compilation and shared-loader checks do not establish rendered usability. Natural-media quality, broader fault/retry campaigns, Stash closure, platform/device and human/release qualification remain open.
