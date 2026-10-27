# Keep generated point streams separate from completed scripts

Use a distinct HSP stream owner for generated timed points because the existing HSSP
preparation binds an immutable complete script; live append order and uncertain
outcomes cannot inherit that script's identity or authority. Retain one exact
append at a time with cumulative indices, bounded future coverage and explicit
Play; a gap, stale timing or unknown append retires further motion and preserves
stop custody, because manufacturer sources establish neither append deduplication
nor an exclusive device-side stream lease. Keep canonical positions at 0–100
percent and preserve signed stream status, recording the contradictory REST
position maximum as an unresolved compatibility gate rather than rescaling motion.

The [manufacturer contract](handy-hsp-stream-contract-2026-10-23.md) supplies the
wire semantics. The queue, codec and coordinator are implementation components;
durable claims, shared device ownership, authenticated live-source admission and
the native sender must be composed before this becomes an enabled product path.
Neither approximate buffer status nor local transport acknowledgment qualifies
display timing, model accuracy, physical trajectory or stopping.
