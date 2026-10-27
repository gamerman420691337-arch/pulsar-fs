# ADR B5-017: bounded first-party piped encoded ingress

Status: implemented; authored native functional validation passed. Acquisition and device/release qualification remain open.

The authenticated decoder already consumes open encoded streams incrementally. The shipped file adapter requires a completed regular file and cannot represent an ongoing producer. Add a first-party stdin adapter through the existing single-attachment channel, without introducing native acquisition or a client-owned decoder.

Own an unbuffered validated pipe/file descriptor, bounded chunks and idle polling. Reject interactive/device/socket descriptors before source attachment. Detect oversize with at most one excess byte and do not forward it. EOF must explicitly seal and validate native completion; errors drop the connection and never reconnect or rearm.

Propagate a conservative absolute lease end to status, attachment and every byte/completion exchange. Use the beginning of the status bracket, not receipt arrival, and only tighten later observations. Fresh engine authority and native watchdog checks still fence actual effects. Idle producer input must not defer stop/expiry indefinitely.

Decoded-stdout delivery age remains explicit. Piped intake is not exposure-time provenance or physical-device authority. Real acquisition, automatic segmentation/orchestration, measured source/player timing, qualified models/GPU/six-axis, Handy and all release gates remain open. See [implementation and retained evidence](../../docs/PULSAR_PIPED_ENCODED_STREAMS.md).
