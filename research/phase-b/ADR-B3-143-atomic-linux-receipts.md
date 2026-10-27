# ADR B3-143: atomic Linux recovery receipts

Status: implemented with scoped native process-crash evidence. Physical durability and other-platform qualification remain open.

An actual owned SIGKILL before the parent receipt write leaves a zero-byte final-name receipt in the preceding native application. Handled-error destructors cannot run after abrupt death. Do not reconcile arbitrary truncated receipts by deletion or invent their identities.

On Linux, create a private O_TMPFILE inode inside the held destination directory. Hold its exclusive lock, write bounded metadata and sync the file before linking it into the final name with no replacement. Use the held descriptor through procfs for unprivileged linkat, check held/named identity and private single-link shape, then sync the directory and any required outer parent. Existing Pending cleanup still applies to handled errors after linking. Before linking, close/death disposes the unnamed inode. After linking, the visible receipt has its complete contents; process death may still leave directory durability unconfirmed.

Unsupported O_TMPFILE filesystems or unavailable procfs fail explicitly. There is no fallback to publishing incomplete metadata. Other Unix targets retain the preceding implementation and require their own atomic-publication work. Existing legacy truncated receipts remain protected. No overwrite, source deletion, automatic replay, journal bypass or Engine authority change.

Primary syscall reference: [Linux open(2)](https://man7.org/linux/man-pages/man2/open.2.html), O_TMPFILE publication through procfs/linkat. This explains the mechanism; actual probes establish only their measured process-crash behavior.

Verify before-write and before-file-sync deaths leave no visible receipt; before-directory-sync death leaves a parseable complete receipt. Every probe preserves the accepted prefix and original intent and requires a later explicit verified resume. Retain the failed baseline, harness failures and exact final identities. Power loss, storage corruption, failed compensating cleanup and broader platform/model/media/device/release gates remain open.

Verification: nine writer tests pass in RAM and on the SSD filesystem; full Clients pass 318 with 8 ignored. Three actual writer deaths and six preceding I/O/cleanup/client-death campaigns pass with unchanged intent/master and 5,001-action correspondence. Both strict lint configurations, workspace check, formatting, native builds, 443 architecture checks and 36 source guards pass. Journal-creation death is not separately injected; parent receipt death exercises the shared writer. See [scope](../../docs/PULSAR_ATOMIC_LINUX_RECEIPTS.md).
