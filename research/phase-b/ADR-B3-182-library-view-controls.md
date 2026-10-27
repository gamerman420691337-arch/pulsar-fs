# ADR B3-182: Direct retained-library source-view controls

Status: implemented; scoped software evidence; qualification remains open.

The direct visual editor could author models/packs, dimensions and thresholds,
but source views still required JSON. Reuse the existing single-source renderer
and Core validation rather than create another view contract. Capture an optional
inline VideoView with the background IntakeVisual action, reject conflicting JSON,
and validate before constructing the retained recipe. The frozen declaration is
part of the exact Prepare/retry content. The renderer changes form state only;
single-source changes still invalidate preview through the existing seek path.

Engine/Worker source geometry and source correspondence remain their own checks.
There is no Engine/Protocol migration or source-view inference. A batch applies
one declaration to all inputs; differently declared media needs separate batches.

See [contract, retained failures and exact validation limits](../../docs/PULSAR_LIBRARY_VIEW_CONTROLS.md)
and [evidence](../../docs/evidence/roadmap-2026-10-12/library-view-controls/checkpoint.json).
