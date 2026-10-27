# ADR B3-160: cleanup receipt lock follows its owner lifetime

Date: 2026-10-12. Status: implemented with scoped regression evidence; qualification open.

The cleanup Record owns the receipt lock for the helper lifetime. Explicitly unlock that open-file description at Record retirement before its field is closed, so an unrelated inherited descriptor does not extend the ended helper's ownership. Preserve locks during actual operations and retain every receipt/directory/artifact/authorization/cancellation check. Use the existing private recovery-journal owner guard as the corresponding lifetime pattern.

A child-descriptor regression must refuse while the parent owner is active, then recover while the child remains alive after owner retirement. Preserve the original FAIL and source subject. Do not relabel intermittent historical failures or claim every lock site, unlock fault or platform is covered. See [scope](../../docs/PULSAR_CLEANUP_LOCK_OWNERSHIP.md). Full roadmap PARTIAL.
