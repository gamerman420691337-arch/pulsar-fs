# ADR B3-179: observe library companions per axis before partial generation

Status: accepted software design; partial-generation and qualification gates open.

EDIT-06 and TEST-15 require preserving existing work while handling missing
secondary channels. Current retained intake excludes a media entry when its
audio bundle exists; visual intake preserves adjacent stroke companions. Whole
entry presence therefore cannot describe which secondary axes are absent.

Add one bounded local per-axis inventory shared by CLI and the desktop background
service. Keep it separate from authenticated generation/publication: path
presence does not establish script validity, source correspondence or authority
to replace a file. Retain recognized observations and blockers individually;
never turn a symlink or metadata failure into a missing-channel selection.

The report recognizes current Pulsar bundle paths and adjacent stroke only.
Existing empty or invalid files are protected by presence. It remains a mutable
observation with `qualified:false`, schedules no Engine work, and preserves any
pending desktop mutation/recovery intent. Existing journal schemas and Engine
reader floor 32 are unchanged.

Partial-axis publication still needs its own captured per-channel intent,
protected-file identity checks, exact recovery receipts and no-overwrite
transaction. A fresh inventory must not silently enable that operation or
redefine TEST-15 as an inspection test.

See [contract](../../docs/PULSAR_LIBRARY_AXIS_INVENTORY.md) and
[evidence](../../docs/evidence/roadmap-2026-10-12/library-axis-inventory/checkpoint.json).
