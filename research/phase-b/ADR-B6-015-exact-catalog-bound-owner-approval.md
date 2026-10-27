# ADR B6-015: Fence future owner-approved generation by exact catalog facts

Status: Linux engine/CLI implemented; source-bound local checks passed. Broader update/cancellation/desktop and release qualification remain open.

Add a separate explicit catalog-bound unqualified selection command. Authenticate fresh session and separate owner proof; verify exact installed association and pinned CPU execution profile. Store the exact bootstrap namespace/catalog revision with approval, optimistic selection revision and replay in one SQLite transaction. Recheck catalog facts immediately before publication.

Generation preparation/admission, preview admission/completion and Stash selection admission must fail closed when the bound catalog disappears, changes revision, expires, corrupts or no longer authenticates the original manifest. Changed catalogs require explicit reapproval, with no implicit fallback. Preserve accepted generation inputs and historical replay; automatic in-flight cancellation remains separate work. Ordinary unqualified owner selection/deactivation may explicitly replace/clear the binding.

Use report schema 2 only when bound and conditionally raise database reader barrier to 14. Preserve schema 14 through all older migration writers. Older readers must reject before writable startup. Preserve unbound schema-1 serialization and portable format 9; imported historical approval bytes never confer current host authority. Bound startup selection reports to 1 MiB before string allocation.

This neither grants official publisher trust nor qualifies rights, native dependencies, provider/model quality or online revocation. Desktop/recovery, active update/rollback/cancellation, platform/hardware/human/release gates remain open. See ../../docs/PULSAR_CATALOG_BOUND_APPROVAL.md.
