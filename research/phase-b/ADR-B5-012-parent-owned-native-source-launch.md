# ADR B5-012: Parent-owned native source launch

Status: private Linux launcher implemented; authenticated source/lease integration open.

## Decision

Launch the B5-011 encoded-source role through existing native confinement with byte-exact private executable/tool snapshots and aggregate memory/worker/storage admission. Start the independent absolute deadline before staging and supervise the owned process/cgroup even without active readers. Transfer one non-clone producer handle; require explicit sealed EOF and retire ambiguous input instead of retrying it. Use bounded OS backpressure and one bounded pending decoded packet; callers must produce and consume concurrently.

Parent stateful acceptance checks exact scope, ready/frame order, frame ordinal, signed rational source PTS progression, delivery clock progression and actual RGB hashes. Successful completion also requires parent sealed intake hash/count, exact accepted frame count, terminal EOF without appended output and native exit success. Drop kills/reaps before joining blocked readers. Reservations and snapshots remain owned until parent drop.

Keep native delivery clock facts separate from exposure, engine lease time and physical-device authority. This primitive cannot set `native_acquisition`, accept authenticated encoded input or offer frames under a capture lease. The successor must bracket clocks, retain the source/delivery age basis and enforce fresh lease authority and absolute lifetime before publishing frames.

## Evidence

See [parent source launch](../../docs/PULSAR_PARENT_SOURCE_LAUNCH.md) for sequence tests and authored native confinement/cancellation/idle-expiry/streaming evidence. Authored stream correspondence is not camera/media/model/GPU/Handy/platform/human/release qualification. Sterile evaluation and training remain unchanged.
