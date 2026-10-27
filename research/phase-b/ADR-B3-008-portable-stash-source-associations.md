# ADR B3-008: Retain Stash source associations without importing authority

Status: implemented locally; historical publication/clock-proof migration and broader qualification remain open.

Portable source snapshots currently preserve media content but omit unpublished Stash capture associations. Preserve archival endpoint/scene/file/server identity and exact capture receipt objects in each source. Use bounded sorted declarations and typed receipt/source correspondence. Host paths remain inert; dependency objects include no bearer/API credentials or owner proof.

Require format 8 when associations or format-8 ancestors exist. Carry the exact receipt bytes across clone/re-export and require imported original associations to remain a subset of current declarations. Import never creates live Stash associations/jobs or resolves archived addresses. The pinned previous format-7 reader refuses before project publication; retain database schema 9.

Extend explicit Stash capture with optional expected existing source version. Check current project Read/ImportSource authority and revision, refuse absent/inconsistent source before query, then re-query and capture through existing descriptor mappings. Require matching immutable source digest and length, recheck in the publication transaction, and retain source version. No historical generation/publication authority is activated. Expose through CLI and compiled desktop selected-source intent.

See [contract and exact local evidence](../../docs/PULSAR_PORTABLE_STASH_ASSOCIATIONS.md). Native authored tests exercise inert successive clones, restart, corruption/omission refusal, changed-byte refusal, exact active rebinding, fresh generation/publication and old-reader refusal. Fresh generation does not close publication of an edited imported historical candidate or migration of archived clock proofs. Actual Stash, model/media, Handy, GUI/platform/human/release gates remain open.
