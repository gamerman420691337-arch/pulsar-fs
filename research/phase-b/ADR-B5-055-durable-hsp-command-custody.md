# Claim exact HSP commands durably before native dispatch

Record each original HSP stream and command under journal v10, sharing device and
credential-route exclusion with hosted-script playback, because an uncertain Add
cannot be reconstructed or safely retried from an approximate remote tail.
Bind the native sender's captured credentials, device and original immutable
effect to a single-use durable claim; retained completion can retry metadata
without sending again. A rejected worker needs separate exact unclaimed-cancellation
proof before releasing its runtime slot for Stop, while journal history, Stop
acknowledgment and cancelled local I/O never become physical stopping evidence.
