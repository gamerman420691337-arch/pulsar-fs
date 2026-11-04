//! Targeted native retirement, separate from durable recovery and fresh admission.
use super::*;
use crate::device_effect_journal::EffectRecord;
use crate::handy_credentials::HandyRouteIdentity;

/// Captured before retirement. Only matching stop effects may extend the plan's
/// original custody snapshot; no new play or controller authority is admitted.
pub(in crate::authority) struct ReconciliationSubject {
    pub boot: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub admission: PlaybackAdmission,
    pub controller: Option<ClientId>,
    pub qualification_record: ArtifactId,
}
impl ReconciliationSubject {
    pub(in crate::authority) fn permits_stop(&self, effect: &EffectRecord) -> bool {
        effect.stop
            && effect.boot == self.boot
            && effect.device_session == self.device_session
            && effect.admission == self.admission
            // Revocation clears the runtime controller before constructing its
            // stop. A stop captured before retirement may retain the exact old
            // controller; neither case admits a new controller identity.
            && (effect.controller.is_none() || effect.controller == self.controller)
            && effect.qualification_record == self.qualification_record
    }
}

/// Native identities only. Neither historical status nor a deserialized plan
/// can construct this token or cause removal of a replacement preparation.
pub(in crate::authority) struct ReconciliationRetirement {
    device: DeviceId,
    route: HandyRouteIdentity,
    entries: Vec<(ArtifactId, Arc<Mutex<HandyPlaybackRuntime>>)>,
    subjects: Vec<ReconciliationSubject>,
}
impl ReconciliationRetirement {
    pub(in crate::authority) fn ids(&self) -> Vec<ArtifactId> {
        self.entries.iter().map(|(id, _)| id.clone()).collect()
    }
    pub(in crate::authority) fn permits_stop(&self, effect: &EffectRecord) -> bool {
        self.subjects
            .iter()
            .any(|subject| subject.permits_stop(effect))
    }
    pub(in crate::authority) fn check_proof(
        &self,
        proof: &handy_reconciliation::NativeRecoveryProof,
    ) -> PResult<()> {
        if !proof.matches_target(
            &self.device,
            self.route.route_sha256(),
            self.route.namespace_sha256(),
        ) {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "native recovery proof belongs to a different retirement target",
            ));
        }
        Ok(())
    }
}

impl Engine {
    /// Caller holds exclusive Handy admission, after synchronous setup/sync
    /// senders have returned. The same registry guard fences final play claims.
    /// Stop dispatch remains independent of this admission guard and the DB.
    pub(in crate::authority) fn retire_handy_for_reconciliation(
        &self,
        device: &DeviceId,
        credentials: &Credentials,
    ) -> PResult<ReconciliationRetirement> {
        let route =
            HandyRouteIdentity::from_snapshot(&self.handy_owner_token, &credentials.connection)?;
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        let mut entries = Vec::new();
        let mut subjects = Vec::new();
        // Include aliases of this route, even if registered under another
        // logical device. A different device and different route stays active.
        for (id, entry) in &registry.entries {
            let i = &entry.report.correlation.inspection;
            if i.registration.device_id != *device
                && !credentials.same_route(&entry.captured.credentials)
            {
                continue;
            }
            let qualification = i.accepted_qualification.as_ref().ok_or_else(|| {
                internal("native recovery target lacks its qualification binding")
            })?;
            subjects.push(ReconciliationSubject {
                boot: i.engine_epoch.clone(),
                device_session: i.device_session.clone(),
                admission: entry.report.admission.clone(),
                controller: entry
                    .controller
                    .as_ref()
                    .map(|c| c.report.controller_id.clone()),
                qualification_record: qualification.dossier_id.clone(),
            });
            entries.push((id.clone(), entry.runtime.clone()));
        }
        for (id, _) in &entries {
            let entry = registry
                .entries
                .get_mut(id)
                .expect("captured under registry guard");
            entry.report.state = HandyPlaybackPreparationState::Retired;
            // Retain every entry until drain/commit. Removing clean entries here
            // could hide pending work from the pump's final registry collection.
            controller::retire_entry(entry);
        }
        registry.retain_syncs(|slot| {
            slot.context.record.registration.device_id != *device
                && !credentials.same_route(&slot.context.captured.credentials)
        });
        Ok(ReconciliationRetirement {
            device: device.clone(),
            route,
            entries,
            subjects,
        })
    }

    /// Called with the still-live proof only after durable exact resolution and
    /// execution-pump release. No DB work occurs while this registry is held.
    /// Poison recovery is restricted to discarding exact retired identities; it
    /// cannot arm a device or remove a newer replacement entry.
    pub(in crate::authority) fn finish_handy_reconciliation(
        &self,
        retirement: ReconciliationRetirement,
        proof: &handy_reconciliation::NativeRecoveryProof,
    ) -> PResult<()> {
        retirement.check_proof(proof)?;
        let mut registry = self
            .handy_preparations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (id, runtime) in retirement.entries {
            if registry.entries.get(&id).is_some_and(|entry| {
                Arc::ptr_eq(&entry.runtime, &runtime)
                    && entry.report.state == HandyPlaybackPreparationState::Retired
            }) {
                registry.entries.remove(&id);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_retirement_covers_revoked_stop_without_admitting_new_controller_or_program() {
        let (mut runtime, admission, controller) = crate::playback_runtime::tests::fixture();
        let clock = crate::playback_runtime::tests::clock();
        let prepared = runtime
            .prepare_start(&controller, clock.clone(), 0)
            .unwrap();
        let play = runtime
            .dispatch_start(
                prepared,
                &admission,
                &controller,
                &clock.domain,
                clock.monotonic_ns,
            )
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let boot = DeviceSessionId::new("recovery-fixture-boot").unwrap();
        let journal =
            crate::device_effect_journal::DeviceEffectJournal::open(dir.path(), boot.clone())
                .unwrap();
        let binding = journal.receipt_binding();
        let stop = runtime.revoke_controller().unwrap().unwrap();
        let record = binding.record_for_effect(&stop).unwrap();
        let subject = ReconciliationSubject {
            boot,
            device_session: runtime.session().session().clone(),
            admission,
            controller: Some(controller),
            qualification_record: record.qualification_record.clone(),
        };
        assert!(record.controller.is_none());
        assert!(subject.permits_stop(&record));
        assert!(!subject.permits_stop(&binding.record_for_effect(&play).unwrap()));
        for case in 0..6 {
            let mut changed = record.clone();
            match case {
                0 => changed.controller = Some(ClientId::new("replacement-controller").unwrap()),
                1 => changed.device_session = DeviceSessionId::new("replacement-session").unwrap(),
                2 => changed.boot = DeviceSessionId::new("replacement-boot").unwrap(),
                3 => changed.admission.revision = RevisionId::new(999),
                4 => {
                    changed.qualification_record =
                        ArtifactId::new("replacement-qualification").unwrap()
                }
                _ => {
                    changed.admission.program_artifact =
                        ArtifactId::new("replacement-program").unwrap()
                }
            }
            assert!(!subject.permits_stop(&changed), "case {case}");
        }
    }
}
