# ADR B6-008: Desktop pack receipts do not become current selection

Status: implemented and exercised headlessly; native-window qualification remains open.

Expose inert intake, byte recheck, current selection, explicit unqualified owner approval, deactivation and exact replay through the existing serialized asynchronous desktop RPC queue. Keep the owner capability in command construction, not model execution or presentation. A selection/deactivation response may be a historical replay: retain it as an operation receipt, invalidate the generation choice and separately query current status. Only that status query populates the generation choice.

Bind UI approval to the entered manifest digest. Editing the digest clears approval and its integrity receipt; disable approval/submission while busy. Retain each mutation's exact request ID and source/digest/revision for replay after a lost acknowledgement. An edited form cannot alter a retained replay intent. This initial state was window-local; persistent session-bound recovery is now implemented under ADR-B6-011-session-bound-desktop-pack-recovery.md.

Use the dedicated engine pack-generation command and preserve placement, interval, target, view and independent audio declarations. Enforce existing validation, including manual-seed forward-placement restrictions. Never convert a selected pack into an arbitrary filesystem model path or silently apply it to the separate preview/batch adapters. Those adapters need their own selection-bound admission. Detector preview is now implemented under ADR-B6-009-selection-bound-detector-preview.md; batch admission is now implemented under ADR-B6-010-selection-bound-stash-batches.md.

The headless tests use actual egui draw and pointer/keyboard input plus command/state checks. They establish local control behavior, not native-window or human acceptance. See ../../docs/PULSAR_DESKTOP_MODEL_PACKS.md.
