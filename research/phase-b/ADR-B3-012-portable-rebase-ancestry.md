# ADR B3-012: retain explicit portable rebase parent facts

Status: local implementation; release qualification remains open.

Local rebases preserve motion and record a parent candidate. Portable capture previously retained motion and origin aliases but omitted that parent. A later clone could not verify the rebased master's source-clock ancestry without guessing from content identity.

Add optional manifest-local `rebased_from` in format 9. Require unchanged motion/evidence/restrictions, monotone base revision, existing parents and bounded acyclic traversal. Translate parent IDs on clone and retain them on re-export. Verify original correspondence through namespace maps. Archived publication follows checked parent and admission-base histories only when their ordered clocks agree.

Promote the reader boundary to schema 11 with ready format-9 export/completed import. Preceding readers must refuse stored semantics they cannot preserve. Old packages remain readable but absent historical rebase facts remain unknown; no byte-equality inference or active imported jobs are introduced.

See [implementation, native evidence and limits](../../docs/PULSAR_PORTABLE_REBASE_ANCESTRY.md). Piecewise mixed-clock correspondence, real-media/model quality and remaining platform/device/human/release requirements are unchanged.
