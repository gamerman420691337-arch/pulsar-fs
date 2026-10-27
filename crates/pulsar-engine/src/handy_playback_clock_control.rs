//! Compose qualified live device timing and a fresh server sample for one controller.
use super::*;
use crate::{
    handy_online::DeviceTimingRead,
    handy_playback_clock::NativePlaybackClock,
    handy_server_clock::{self, NativeServerClock, Sample},
};
pub(in crate::authority::handy_playback_preparation) struct RetainedPlaybackClock {
    pub(in crate::authority::handy_playback_preparation) report: HandyPlaybackClockObservation,
    pub(in crate::authority::handy_playback_preparation) native: NativePlaybackClock,
    pub(in crate::authority::handy_playback_preparation) sync: Arc<native_sync::Context>,
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "combined clock controller or native synchronization is unavailable or changed",
    )
}
fn selected(r: &Request, l: &HandyControllerLease) -> bool {
    matches!(&r.command,Command::AcquireHandyPlaybackClock{acquisition_request_id,controller_id,expected_generation,..}
        if l.acquisition_request_id==*acquisition_request_id && l.controller_id==*controller_id && l.generation==*expected_generation
        && l.state==HandyControllerState::Active && r.project.as_ref()==Some(&l.project_id) && r.expected_revision==Some(l.revision))
}
fn sync_selected(r: &Request, s: &native_sync::Context) -> bool {
    matches!(&r.command,Command::AcquireHandyPlaybackClock{synchronization,..}
        if synchronization.record_id==s.id && synchronization.sync_request_id==s.record.request_id)
}
pub(super) fn sync_current(registry: &Registry, context: &Arc<native_sync::Context>) -> bool {
    registry
        .syncs
        .get(&context.id)
        .is_some_and(|s| s.native.is_some() && Arc::ptr_eq(&s.context, context))
}
impl Engine {
    pub(in crate::authority) fn handy_playback_clock_control(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
        }
        if !self.check_handy_controller_monitor()? {
            return Err(mismatch());
        }
        self.handy_playback_clock_with(
            r,
            actor,
            fingerprint,
            crate::handy_online::inspect_device_timing,
            |clock| handy_server_clock::sample(clock),
            boottime_ns,
        )
    }
    fn playback_clock_view<F: FnMut() -> PResult<u64>>(
        &self,
        db: &Connection,
        actor: &SessionId,
        r: &Request,
        mut report: HandyPlaybackClockObservation,
        clock: &mut F,
    ) -> PResult<ResponseBody> {
        report.validate()?;
        let Command::AcquireHandyPlaybackClock {
            synchronization, ..
        } = &r.command
        else {
            return Err(mismatch());
        };
        if report.request_id != r.request_id
            || !selected(r, &report.controller)
            || report.synchronization != *synchronization
        {
            return Err(mismatch());
        }
        let live = self.controller_view(db, actor, report.controller.clone(), r, clock)?;
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        let now = registry.sweep_controllers(clock);
        report.current = now.is_ok_and(|now| {
            selected(r, &live)
                && registry
                    .entries
                    .get(&live.preparation_id)
                    .and_then(|e| e.playback_clock.as_ref())
                    .is_some_and(|c| {
                        c.report.request_id == report.request_id
                            && c.report.clock_domain == report.clock_domain
                            && sync_current(&registry, &c.sync)
                            && c.native.at(now).is_ok()
                    })
        });
        Ok(ResponseBody::HandyPlaybackClock(Box::new(report)))
    }
    pub(in crate::authority) fn handy_playback_clock_with<D, S, F>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        inspect: D,
        sample: S,
        mut clock: F,
    ) -> PResult<ResponseBody>
    where
        D: FnOnce(
            Arc<Credentials>,
            Instant,
            &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<DeviceTimingRead>,
        S: FnOnce(&mut dyn FnMut() -> PResult<u64>) -> PResult<Sample>,
        F: FnMut() -> PResult<u64>,
    {
        r.validate()?;
        let Command::AcquireHandyPlaybackClock {
            acquisition_request_id,
            controller_id,
            expected_generation,
            synchronization,
            owner_proof,
            ..
        } = &r.command
        else {
            return Err(ProtocolError::unsupported("combined playback clock"));
        };
        let (lease, context, device, server_qualification, attempt, prior, begin) = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::HandyPlaybackClock(v) = body else {
                    return Err(mismatch());
                };
                return self.playback_clock_view(&db, actor, r, *v, &mut clock);
            }
            check_revision(&db, r)?;
            let saved = load(&db, actor, project(r)?, acquisition_request_id)?;
            let lease = self.controller_view(&db, actor, saved, r, &mut clock)?;
            if !selected(r, &lease) {
                return Err(mismatch());
            }
            let (context, device, prep) = {
                let mut registry = self.handy_preparations.lock().map_err(internal)?;
                let now = registry.sweep_controllers(&mut clock)?;
                let slot = registry
                    .syncs
                    .get(&synchronization.record_id)
                    .ok_or_else(mismatch)?;
                let native = slot.native.as_ref().ok_or_else(mismatch)?;
                native.error_at(now)?;
                (
                    slot.context.clone(),
                    native.clone(),
                    registry
                        .entries
                        .get(&lease.preparation_id)
                        .ok_or_else(mismatch)?
                        .report
                        .clone(),
                )
            };
            let (captured, _) = self.preparation_references(&db, actor, &prep, r)?;
            if !sync_selected(r, &context)
                || context.record.actor_session != *actor
                || context.record.original_controller.project_id != *project(r)?
                || context.record.original_controller.revision != lease.revision
                || context.preparation.selection()? != prep.selection()?
                || context.record.registration != captured.connection.registration
                || !Arc::ptr_eq(&context.captured.credentials, &captured.credentials)
                || !context.captured.hosted.same_location(&captured.hosted)
            {
                return Err(mismatch());
            }
            let i = &prep.correlation.inspection;
            let qualified = self
                .accepted_handy_device_clock(
                    &db,
                    &context.record.preparation_qualification,
                    &captured.connection.registration,
                    &i.device,
                    &captured.hosted,
                )?
                .ok_or_else(mismatch)?;
            if qualified != device.qualified {
                return Err(mismatch());
            }
            let server = self.accepted_handy_server_clock(
                &db,
                &context.record.preparation_qualification,
                &captured.connection.registration,
                &i.device,
                &captured.hosted,
            )?;
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let begin = registry.sweep_controllers(&mut clock)?;
            if !sync_current(&registry, &context) {
                return Err(mismatch());
            }
            let e = registry
                .entries
                .get_mut(&lease.preparation_id)
                .ok_or_else(mismatch)?;
            e.clock_attempt = e.clock_attempt.checked_add(1).ok_or_else(mismatch)?;
            e.server_clock = None;
            (
                lease,
                context,
                device,
                server,
                e.clock_attempt,
                e.playback_clock.take(),
                begin,
            )
        };
        let mut query = r.clone();
        query.command = Command::InspectHandyDeviceTiming {
            acquisition_request_id: acquisition_request_id.clone(),
            controller_id: controller_id.clone(),
            expected_generation: *expected_generation,
            allow_online_inspection: true,
            owner_proof: owner_proof.clone(),
        };
        let ResponseBody::HandyDeviceTiming(observed) =
            self.handy_device_timing_with(&query, actor, inspect, &mut clock)?
        else {
            return Err(mismatch());
        };
        if let Err(error) = device
            .check_observation(&observed.first)
            .and_then(|_| device.check_observation(&observed.second))
        {
            self.handy_preparations
                .lock()
                .map_err(internal)?
                .retain_syncs(|s| !Arc::ptr_eq(&s.context, &context));
            return Err(error);
        }
        let observation = sample(&mut || {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let now = registry.sweep_controllers(&mut clock)?;
            if !sync_current(&registry, &context)
                || !registry
                    .entries
                    .get(&lease.preparation_id)
                    .is_some_and(|e| {
                        e.clock_attempt == attempt
                            && e.controller
                                .as_ref()
                                .is_some_and(|l| selected(r, &l.report))
                    })
            {
                return Err(mismatch());
            }
            device.error_at(now)?;
            Ok(now)
        })?;
        if observation.sent_ns < begin
            || observation.sent_ns < observed.inspection_finished_boottime_ns
            || observation.received_ns < observation.sent_ns
        {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            registry.controller_clock_failed = true;
            let _ = registry.sweep_controllers(&mut clock);
            return Err(mismatch());
        }
        let domain = ArtifactId::new(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    "handy-playback-clock-v1",
                    &self.engine_epoch,
                    &lease.controller_id,
                    lease.generation,
                    attempt,
                    &r.request_id,
                    &context.id
                ))
                .map_err(internal)?
            )
        ))
        .map_err(internal)?;
        let server = NativeServerClock::new(domain, observation, server_qualification)?;
        if let Some(previous) = prior {
            server.agrees_with(&previous.native.server)?;
        }
        let native = NativePlaybackClock { server, device };
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, actor)?;
        check_request_identity(&db, actor.as_str(), r, fingerprint)?;
        if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
            let ResponseBody::HandyPlaybackClock(v) = body else {
                return Err(mismatch());
            };
            return self.playback_clock_view(&db, actor, r, *v, &mut clock);
        }
        let live = self.controller_view(&db, actor, lease.clone(), r, &mut clock)?;
        if !selected(r, &live) {
            return Err(mismatch());
        }
        {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            native.at(registry.sweep_controllers(&mut clock)?)?;
            if !sync_current(&registry, &context)
                || !registry
                    .entries
                    .get(&lease.preparation_id)
                    .is_some_and(|e| e.clock_attempt == attempt)
            {
                return Err(mismatch());
            }
        }
        let at = native.server.clock.monotonic_ns;
        let combined = native.at(at)?;
        let mut report = HandyPlaybackClockObservation {
            request_id: r.request_id.clone(),
            controller: lease.clone(),
            qualification: context.record.preparation_qualification.clone(),
            synchronization: synchronization.clone(),
            synchronization_sent_boottime_ns: native.device.acknowledgment.sent_boottime_ns,
            synchronization_received_boottime_ns: native.device.acknowledgment.received_boottime_ns,
            server_sent_boottime_ns: native.server.sent_ns,
            server_received_boottime_ns: at,
            clock_domain: combined.domain,
            estimated_server_ms: combined.estimated_server_ms,
            server_uncertainty_ns: native.server.clock.uncertainty_ns,
            device_uncertainty_ns: native.device.error_at(at)?,
            combined_uncertainty_ns: combined.uncertainty_ns,
            admitted_user_offset_ms: native.device.qualified.contract.user_offset_ms,
            current: true,
            playback_enabled: false,
        };
        report.validate()?;
        let tx = db.transaction().map_err(internal)?;
        remember(
            &tx,
            actor.as_str(),
            r,
            fingerprint,
            &ResponseBody::HandyPlaybackClock(Box::new(report.clone())),
        )?;
        tx.commit().map_err(internal)?;
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        report.current = registry
            .sweep_controllers(&mut clock)
            .is_ok_and(|now| native.at(now).is_ok())
            && sync_current(&registry, &context)
            && registry
                .entries
                .get(&lease.preparation_id)
                .is_some_and(|e| {
                    e.clock_attempt == attempt
                        && e.controller
                            .as_ref()
                            .is_some_and(|l| selected(r, &l.report))
                });
        if report.current {
            registry
                .entries
                .get_mut(&lease.preparation_id)
                .expect("checked controller")
                .playback_clock = Some(RetainedPlaybackClock {
                report: report.clone(),
                native,
                sync: context,
            });
        }
        Ok(ResponseBody::HandyPlaybackClock(Box::new(report)))
    }
}
