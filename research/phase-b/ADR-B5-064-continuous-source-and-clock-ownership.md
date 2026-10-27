# Preserve stream origin while transferring source and clock ownership

Implementation is in progress; composed and physical qualification remain open.
A continuous browser-analysis stream keeps its original HSP identity, time zero,
Setup and Play, while explicit native transfers replace the active capture,
media and timing capabilities. Each successor preserves its original local
sequence and timestamps. This avoids treating worker readiness, copied descriptors
or sequence offsets as evidence that the same source motion continued.

The transfer joins the predecessor's actual final input, worker and delivery to
the successor's actual accepted pair through shared native pixel custody. A warming
worker cannot become active. Final input closure remains distinct from intermediate
transfer. One bounded overlap and one retained opaque transfer token are allowed;
all old pending effects must settle before commit. Per-epoch weak consumers make
late predecessor callbacks local to their original epoch. The producer, source
capabilities and Core cursor promote under the same checked operation; failure
retires the stream rather than reconstructing an old authority.

Preparation remains a finite operation with its original history and deadline.
An explicit Enable request admits a separate session observation owner while that
original scope is current. Its requested duration is a ceiling bounded by the
original synchronization acknowledgment and qualified error feasibility; it does
not guarantee future readable clocks. Each renewal checks actual read timing,
identity, settings and error bounds, and requires the predecessor observation to
remain current through publication. No refresh changes the original synchronization
age or sends a script-time correction. Immutable observation pins remain attached
to the effects and Play references that used them.

The visible player keeps its identity, video elements and URL. Future bounded reads
may select a committed successor capability; already-issued reads retain their
original deadline. The client switches only after native transfer confirmation.
The existing hosted HSSP guard is a different ownership contract and cannot be
silently transferred by this HSP path. The full roadmap retains real-media/model,
platform, physical-device, formal, human and release obligations.

The [frozen interface contract](../../docs/evidence/roadmap-2026-10-23/hsp-continuity/integration-contract.json)
and [current integration checkpoint](../../docs/evidence/roadmap-2026-10-23/hsp-continuity/checkpoint.json)
record implementation ownership and required native/rendered scenarios. Neither
this decision nor authored fixture evidence supplies qualification.
