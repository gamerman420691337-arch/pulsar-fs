# ADR B3-080: explicit annotated CPU Preview in the desktop

Status: implemented and software checks verified. Roadmap PARTIAL.

## Problem

The desktop stripped temporal model analysis from Preview requests and could not present the source-bound frame-pair evidence already supported by the Engine and CLI.

## Decision

Route explicit annotated CPU profiles through the distinct annotated Preview command, using a loaded source-bound target view and either file model/profile or current owner-selected pack. Bind presentation to the exact view, canonical profile, model/runtime, source and placement seek. Keep ordinary detector and unannotated temporal behavior intact. Load both pixel artifacts on the existing RPC thread and present the predecessor, current target and optional unqualified position. Clear both textures when controls change or artifact loading fails; preserve explicit unsupported-pair absence.

## Verification and limits

See [desktop implementation and evidence](../../docs/PULSAR_ANNOTATED_DESKTOP_PREVIEW.md). Controller tests and authored native calls establish software correspondence. Actual window rendering, human usability, useful model quality, platform distribution, physical Handy and release acceptance remain independent open gates. No storage or wire-format changes are required.
