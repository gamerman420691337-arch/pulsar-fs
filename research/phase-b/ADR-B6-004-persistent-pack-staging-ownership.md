# ADR B6-004: persist pack staging identity and observe descriptor ownership

Status: implemented locally; recovery mutation and release qualification remain open.

## Decision

An abandoned-looking directory or PID record is insufficient evidence for safe recovery. New inert imports therefore persist a manifest identity marker and captured manifest before copying assets, and hold the marker's exclusive descriptor lock throughout import. Crash releases the lock while retaining exact marker/manifest evidence. Supplier paths reserve the engine marker name. No publisher/native execution authority follows from a marker.

The authenticated `ModelPackStagingStatus` API and `model pack-staging-status` CLI inspect bounded private descriptor-relative state. Recognized unlocked, locked and unrecognized observations are distinct. Locked observations claim no manifest identity; unrecognized legacy/unsafe entries remain unchanged. The client independently validates the bounded report. Absence does not create a store. Current queries may wait behind a same-engine import, and an external lock is not a proof of a particular live writer.

## Authority and remaining work

This operation only observes. It never deletes, quarantines, repairs, activates or approves assets. Future recovery must revalidate locks, ownership, complete paths and retained/published exclusions under fresh authority. Cooperative advisory locking is an explicit assumption; identity metadata is not an authenticated publisher signature. Legacy entries cannot gain recognized identity from their names. Active selection/update/rollback, quota/cleanup, native trust, rights, model/provider and all platform/human/device/release gates remain required.

## Evidence

See [offline pack documentation](../../docs/PULSAR_OFFLINE_MODEL_PACKS.md#staging-ownership-observations) and the [checkpoint](../../docs/evidence/roadmap-2026-10-01/pack-staging-ownership/validation.json). Native owned engine crashes and a separate marker-lock holder exercise reported states; legacy, alias and tampered marker cases remain unchanged. Protocol/authority tests fence identity and owner/session boundaries. No pack runtime or model is activated.
