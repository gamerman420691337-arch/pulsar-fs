//! Controller-scoped timing inspection. No device synchronization or timing grant is created.
use super::*;
use crate::handy_online::DeviceTimingRead;
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "device timing controller or context changed",
    )
}
fn selected(r: &Request, lease: &HandyControllerLease) -> bool {
    matches!(&r.command,Command::InspectHandyDeviceTiming {acquisition_request_id,controller_id,expected_generation,..}
        if lease.acquisition_request_id==*acquisition_request_id && lease.controller_id==*controller_id
        && lease.generation==*expected_generation && lease.state==HandyControllerState::Active
        && r.project.as_ref()==Some(&lease.project_id) && r.expected_revision==Some(lease.revision))
}
impl Engine {
    pub(in crate::authority) fn handy_device_timing_control(
        &self,
        r: &Request,
        actor: &SessionId,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
        }
        if !self.check_handy_controller_monitor()? {
            return Err(mismatch());
        }
        self.handy_device_timing_with(
            r,
            actor,
            crate::handy_online::inspect_device_timing,
            boottime_ns,
        )
    }
    pub(in crate::authority) fn handy_device_timing_with<D, F>(
        &self,
        r: &Request,
        actor: &SessionId,
        inspect: D,
        mut clock: F,
    ) -> PResult<ResponseBody>
    where
        D: FnOnce(
            Arc<Credentials>,
            Instant,
            &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<DeviceTimingRead>,
        F: FnMut() -> PResult<u64>,
    {
        r.validate()?;
        let Command::InspectHandyDeviceTiming {
            acquisition_request_id,
            ..
        } = &r.command
        else {
            return Err(ProtocolError::unsupported("device timing inspection"));
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let (lease, prep, captured, begin) = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_revision(&db, r)?;
            let saved = load(&db, actor, project(r)?, acquisition_request_id)?;
            let live = self.controller_view(&db, actor, saved, r, &mut clock)?;
            if !selected(r, &live) {
                return Err(mismatch());
            }
            let prep = self
                .handy_preparations
                .lock()
                .map_err(internal)?
                .entries
                .get(&live.preparation_id)
                .ok_or_else(mismatch)?
                .report
                .clone();
            let (captured, _) = self.preparation_references(&db, actor, &prep, r)?;
            let begin = self
                .handy_preparations
                .lock()
                .map_err(internal)?
                .sweep_controllers(&mut clock)?;
            (live, prep, captured, begin)
        };
        let result = (|| {
            let observed = inspect(captured.credentials.clone(), deadline, &mut || {
                let mut registry = self.handy_preparations.lock().map_err(internal)?;
                let now = registry.sweep_controllers(&mut clock)?;
                if !registry
                    .entries
                    .get(&lease.preparation_id)
                    .and_then(|e| e.controller.as_ref())
                    .is_some_and(|l| selected(r, &l.report))
                {
                    return Err(mismatch());
                }
                Ok(now)
            })?;
            if Instant::now() >= deadline {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "device timing inspection deadline expired",
                ));
            }
            let expected = &prep.correlation.inspection.device;
            if !expected.same_state(&observed.before) || !expected.same_state(&observed.after) {
                return Err(mismatch());
            }
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_revision(&db, r)?;
            let live = self.controller_view(&db, actor, lease.clone(), r, &mut clock)?;
            if !selected(r, &live) {
                return Err(mismatch());
            }
            let (current, _) = self.preparation_references(&db, actor, &prep, r)?;
            if !Arc::ptr_eq(&current.credentials, &captured.credentials)
                || !current.hosted.same_location(&captured.hosted)
            {
                return Err(mismatch());
            }
            let binding = prep
                .correlation
                .inspection
                .accepted_qualification
                .clone()
                .ok_or_else(mismatch)?;
            self.accepted_handy_timing(
                &db,
                &binding,
                &current.connection.registration,
                &observed.after,
                &current.hosted,
            )?;
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let end = registry.sweep_controllers(&mut clock)?;
            if !registry
                .entries
                .get(&live.preparation_id)
                .and_then(|e| e.controller.as_ref())
                .is_some_and(|l| selected(r, &l.report))
            {
                return Err(mismatch());
            }
            let report = HandyDeviceTimingInspection {
                request_id: r.request_id.clone(),
                controller: live,
                preparation_qualification: binding,
                before: observed.before,
                first: observed.first,
                second: observed.second,
                after: observed.after,
                inspection_started_boottime_ns: begin,
                inspection_finished_boottime_ns: end,
                synchronization_verified: false,
                playback_enabled: false,
            };
            report.validate()?;
            // Numeric vendor fields must not echo even a numeric credential.
            if captured
                .credentials
                .echoed_text(&serde_json::to_string(&report).map_err(internal)?)
            {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "credential-bearing timing report withheld",
                ));
            }
            Ok(ResponseBody::HandyDeviceTiming(Box::new(report)))
        })();
        if result.is_err() {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            if registry
                .entries
                .get(&lease.preparation_id)
                .and_then(|e| e.controller.as_ref())
                .is_some_and(|l| selected(r, &l.report))
            {
                registry.retire_controller(&lease.preparation_id, &lease.controller_id);
            }
        }
        result
    }
}
