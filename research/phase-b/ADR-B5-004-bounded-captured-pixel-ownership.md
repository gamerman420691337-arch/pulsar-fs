# ADR B5-004: bounded captured-pixel ownership

Status: conditional engine-library runtime and production pair parser implemented; continuous session integration open.

The metadata scheduler bounds descriptors but cannot prove that replaced pixel allocations are released. Temporal inference also needs previous/current chronological captures, which are not necessarily the previous/current analyzed frames under overload.

Give the host runtime immutable fixed-size RGB buffers, one pending pair and one opaque issued pair. Derive artifact identity from actual bytes. Retain the last chronological capture as an alias; replacement can transiently require a fifth buffer. Reserve that pixel capacity from the aggregate resource pool and attach reservation lifetime to shared buffer ownership. No issued work may detach its backing buffers from that reservation.

Use a private runtime owner identity as well as epoch/revision/source/frame metadata. Preserve issued work on invalid-clock/wrong-owner completion so the correct consumer can retry. Consume successful/stale/retired work. A source or capture gap over 500ms seeds a fresh temporal pair; it never bridges the gap or relabels issued work. A shared packed parser connects the byte ABI to the production finite-pair worker.

This module remains conditional host software. Acquisition authority, clock calibration, authenticated transport, model-result binding, continuous worker confinement, project commit and device dispatch stay with their owning engine services. The pixel reservation is not a whole-process/model memory qualification. Existing GPU six-axis, selected Handy and physical release gates remain unchanged.
