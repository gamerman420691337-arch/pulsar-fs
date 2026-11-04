# Retained generation origin inspection

Status: implemented read-only historical verification. Publication of edited imported motion under archived generation evidence remains open.

```sh
pulsar project archived-generation-status PROJECT CANDIDATE
```

The candidate must be an original imported anchor in the immutable local archive projection, with the same current motion reference and lineage and no local job. The engine checks the retained identity map, exact canonical manifests, object catalogue and namespace graph, then follows candidate mappings to original generation records. Native and imported manual-script candidates without generation origins refuse. Newly rebased aliases are not accepted as original anchors.

For each origin the engine checks the original worker/source/dependency/receipt/program correspondence using retained, hash- and length-verified objects. Placed generation additionally runs the originating clock verifier on retained receipt and raw program readers. It checks configuration, source placement, original sampled clocks and raw-to-canonical motion identity without reopening the historical worker directory. Unplaced generation reports no sampled clock proof. The native clock path uses the same verifier with its existing regular-file readers.

Only engine-owned, descriptor-selected CAS objects are opened. Archived source, model, runtime, output paths and Stash endpoints remain inert. SQLite metadata is bounded before allocation; graph, object buffers, decoded evidence and clock plans require memory admission. The response has at most 16 sorted original anchors and a 768 KiB bound. Every response identifies itself as archival, without an active job, publication authority or release qualification. Inspection creates no generation job and does not change project motion, protection or live Stash binding.

The desktop candidate panel exposes the same inspection command and discards responses for a different visible project/candidate. The later optional [current master ancestry check](PULSAR_ARCHIVED_MASTER_ANCESTRY.md) additionally binds its revision and content identity. Controls compile; rendered usability is unverified.

The [local validation receipt](evidence/roadmap-2026-10-01/archived-generation-origins/validation.json) retains exact source/binary identities and checks. Authored native fixtures cover forward/reverse placed clocks, two clones and re-export, restart, unavailable original worker output, corrupted retained receipt refusal, unplaced interval settings and manual-script refusal. Existing archived retiming checks run against the shared namespace loader. These are synthetic software checks; no real Stash or Handy contact, private reference training, independent model qualification or release acceptance occurred.

This resolver is a prerequisite for archival publication. The later ancestry verifier covers retained authored bases and current edits for the original anchor. Publication still must consume that verifier transactionally, handle missing/mixed correspondence, migrate clocks explicitly, bind a freshly checked local source/destination and retain publication proof without inventing an active historical job. A historical origin report alone does not establish those facts or authorize publication.
