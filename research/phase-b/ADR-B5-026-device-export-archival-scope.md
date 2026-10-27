# ADR B5-026 — Preserve proposal scope in portable export history

A real native reproduction showed that a successful analytic device-motion export makes
package capture fail: the exporter records `device-motion`, while the archive requires a
single Core axis. Add an archival scope type distinguishing a selected axis from the whole
proposal. Relabeling the proposal as Stroke, dropping its receipt, or reading its mutable
output to guess axes would lose or invent historical meaning.

Keep ordinary receipt JSON byte-compatible and retain the legacy `axis` field name.
The new scope requires package format v14, including when retained through import origins.
Container admission, stored package descriptors and client probes accept v14 and continue
to reject unknown versions. This explicit version boundary prevents older readers from
silently treating a composite proposal as a neutral-axis export.

A complete receipt still means local file publication, not device execution or admission.
Pending and unknown publication states survive unchanged. Neutral motion association must
match an actual axis scope; a proposal receipt cannot supply that evidence merely because
its hash matches. No Core axis, project timeline, physical authority or external path
resolution is added by this archival correction.
