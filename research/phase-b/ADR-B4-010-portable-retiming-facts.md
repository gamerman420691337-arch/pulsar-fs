# ADR B4-010: Retain original retiming facts through portable clone ancestry

Status: implemented archive closure and import recomputation; archival status integrated in B4-011; cross-namespace executable proof open.

Format 4 preserved retiming descriptions and motion but omitted the structured facts needed to verify their intent. Format 5 includes strict bounded, revision-ordered native receipt facts, captured through independent engine verification and bound to original history, placements and motion identities. Import recomputes the declared transformation against verified archived programs under explicit memory admission before publishing a project.

A clone retains each receipt in its original digest-bound manifest rather than fabricating a new local actor/session/request acknowledgment. Existing complete origin mappings preserve project, placement and history correspondence. Local receipts cannot shadow imported revisions. Re-export retains original bytes and executable dependencies stay inert. Format 5 propagates through ancestry to create an explicit older-reader refusal boundary.

The local receipt-status contract remains local. [B4-011](ADR-B4-011-archival-retiming-correspondence.md) now provides a separate archival reader. Verified cross-namespace clock-proof integration remains required; copying declarations into the local receipt table would incorrectly claim destination authentication. No authority or qualification transfers through these facts.

Authenticated capture and recomputation refusal tests supplement compiled export, pinned format-4 refusal and two successive clone/re-export cases. See [the portable contract](../../docs/PULSAR_PORTABLE_RETIMING_RECEIPTS.md) for exact evidence and remaining work. Project database schema remains 6; Stash timing proof schemas remain 1/2.
