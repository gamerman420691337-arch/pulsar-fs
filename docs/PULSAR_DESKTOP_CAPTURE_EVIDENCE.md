# Desktop capture evidence

Open a project and select **Capture evidence**. **Refresh / first page** discovers retained CUDA captures; **Next page** advances the bounded capture-ID index. Each row shows its source and captured revision. Select **Inspect originals** to validate and view the retained record. Select Origin, Manifest, Summary, Receipt or Trace, enter a new destination under **Save as**, and select **Save original artifact**.

The panel uses an independent bounded background queue and the shared authenticated session client. Listing and inspection require the current project revision and Read grant; original downloads use the existing checked bulk transfer, byte count/SHA-256 verification, retirement and no-overwrite publication. The render thread does not wait for Engine or disk operations. Controls are disabled while a panel request is outstanding.

Project/revision changes clear the old page, inspection and destination. Late old-scope query results are discarded. An export already requested can complete after navigation; its completion notice names the original project and capture without installing old records in the new view. The index is ordered by capture ID rather than time, and pages do not imply a cross-request retention snapshot.

Verification passed in [the desktop checkpoint](evidence/roadmap-2026-10-08/desktop-cuda-evidence/checkpoint.json). Headless egui output proves widget construction only. Visual usability, native window interaction, broader failure/concurrency scenarios, retention controls, model/media quality, Handy, platform and human/release qualification remain open.

The final serial workspace gate passed 1,044 tests (56 ignored). The explicitly invoked native desktop-worker case and the shipped CPU-only CLI campaign passed, including all five original artifacts and occupied-destination refusal. Independent SQL/file/controller/CLI readback is exact. All 270 structural checks passed, and all 367 source records match their saved snapshots and archives. Engine/protocol sources are unchanged from the preceding CUDA-feature gate; that gate was not rerun for these client-only production changes.
