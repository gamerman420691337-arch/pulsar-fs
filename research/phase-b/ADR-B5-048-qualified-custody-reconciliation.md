# ADR B5-048 — Qualified custody reconciliation

Preserve original device-command records and outcomes, and record resolution as a separate,
exact-membership transaction. Recovery requires a current accepted physical qualification with
an explicit prior-command settling and stopped-observation contract; older qualification subjects
and REST state reports alone cannot supply it. These bounds are conditional qualification
requirements, not vendor guarantees or default stopping assumptions.

Retire native preparations and drain only the selected logical device and credential-route
aliases before taking fresh bounded observations. Keep rejection custody until the durable
resolution covers its exact original effect; a rejected handoff does not become an attempted or
acknowledged effect. Separate final runtime cleanup from journal I/O so other devices' stop owners
can continue. Remove only captured retired runtime identities, never replacement preparations.

Resolution leaves playback disarmed. Resumption requires new explicit setup, preparation,
controller and timing operations. Status/replay may repair metadata but cannot reconstruct native
proof, retry device I/O, or restore authority. Local software and synthetic native-history checks
pass; no physical recovery qualification is established by this decision.
