//! Native-only server-clock custody beneath an exact controller lease.
use super::*;
use crate::handy_server_clock::{self, NativeServerClock, Sample};

pub(in crate::authority::handy_playback_preparation) struct RetainedClock {
    report: HandyServerClockObservation,
    native: NativeServerClock,
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "server-clock controller selection expired or changed",
    )
}
fn matches_request(r: &Request, lease: &HandyControllerLease) -> bool {
    matches!(&r.command, Command::SampleHandyServerClock { acquisition_request_id, controller_id, expected_generation, .. }
        if lease.acquisition_request_id == *acquisition_request_id && lease.controller_id == *controller_id
        && lease.generation == *expected_generation && Some(lease.revision) == r.expected_revision
        && r.project.as_ref() == Some(&lease.project_id) && lease.state == HandyControllerState::Active)
}
impl Engine {
    pub(in crate::authority) fn handy_server_clock_control(
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
        self.handy_server_clock_with(
            r,
            actor,
            fingerprint,
            |clock| handy_server_clock::sample(clock),
            boottime_ns,
        )
    }
    fn server_clock_view<F: FnMut() -> PResult<u64>>(
        &self,
        db: &Connection,
        actor: &SessionId,
        r: &Request,
        mut report: HandyServerClockObservation,
        clock: &mut F,
    ) -> PResult<ResponseBody> {
        report.validate()?;
        if report.request_id != r.request_id || !matches_request(r, &report.controller) {
            return Err(mismatch());
        }
        let live = self.controller_view(db, actor, report.controller.clone(), r, clock)?;
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        let now = registry.sweep_controllers(clock);
        report.current = now.is_ok_and(|now| {
            matches_request(r, &live)
                && registry
                    .entries
                    .get(&live.preparation_id)
                    .and_then(|e| e.server_clock.as_ref())
                    .is_some_and(|c| {
                        c.report.request_id == report.request_id
                            && c.report.clock_domain == report.clock_domain
                            && c.native.at(now).is_ok()
                    })
        });
        Ok(ResponseBody::HandyServerClock(Box::new(report)))
    }
    pub(in crate::authority) fn handy_server_clock_with<S, F>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        sample: S,
        mut clock: F,
    ) -> PResult<ResponseBody>
    where
        S: FnOnce(&mut dyn FnMut() -> PResult<u64>) -> PResult<Sample>,
        F: FnMut() -> PResult<u64>,
    {
        r.validate()?;
        let Command::SampleHandyServerClock {
            acquisition_request_id,
            ..
        } = &r.command
        else {
            return Err(ProtocolError::unsupported("server-clock command"));
        };
        let (lease, qualification, qualified, attempt, prior, begin) = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::HandyServerClock(report) = body else {
                    return Err(internal("server-clock replay type differs"));
                };
                return self.server_clock_view(&db, actor, r, *report, &mut clock);
            }
            check_revision(&db, r)?;
            let saved = load(&db, actor, project(r)?, acquisition_request_id)?;
            let live = self.controller_view(&db, actor, saved, r, &mut clock)?;
            if !matches_request(r, &live) {
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
            let i = &prep.correlation.inspection;
            let binding = i.accepted_qualification.clone().ok_or_else(mismatch)?;
            let qualified = self.accepted_handy_server_clock(
                &db,
                &binding,
                &captured.connection.registration,
                &i.device,
                &captured.hosted,
            )?;
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let begin = registry.sweep_controllers(&mut clock)?;
            let entry = registry
                .entries
                .get_mut(&live.preparation_id)
                .ok_or_else(mismatch)?;
            entry.clock_attempt = entry.clock_attempt.checked_add(1).ok_or_else(mismatch)?;
            entry.playback_clock = None;
            (
                live,
                binding,
                qualified,
                entry.clock_attempt,
                entry.server_clock.take(),
                begin,
            )
        };
        // No project, registry, credentials or journal lock spans network I/O.
        let observation = sample(&mut || {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let now = registry.sweep_controllers(&mut clock)?;
            if !registry
                .entries
                .get(&lease.preparation_id)
                .is_some_and(|e| {
                    e.clock_attempt == attempt
                        && e.controller
                            .as_ref()
                            .is_some_and(|l| matches_request(r, &l.report))
                })
            {
                return Err(mismatch());
            }
            Ok(now)
        })?;
        if observation.sent_ns < begin || observation.received_ns < observation.sent_ns {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            registry.controller_clock_failed = true;
            let _ = registry.sweep_controllers(&mut clock);
            return Err(mismatch());
        }
        let domain = ArtifactId::new(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    "handy-server-clock-v1",
                    &self.engine_epoch,
                    &lease.controller_id,
                    lease.generation,
                    attempt,
                    &r.request_id
                ))
                .map_err(internal)?
            )
        ))
        .map_err(internal)?;
        let native = NativeServerClock::new(domain, observation, qualified)?;
        if let Some(previous) = prior {
            native.agrees_with(&previous.native)?;
        }
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, actor)?;
        check_request_identity(&db, actor.as_str(), r, fingerprint)?;
        if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
            let ResponseBody::HandyServerClock(report) = body else {
                return Err(internal("server-clock replay type differs"));
            };
            return self.server_clock_view(&db, actor, r, *report, &mut clock);
        }
        let live = self.controller_view(&db, actor, lease.clone(), r, &mut clock)?;
        if !matches_request(r, &live) {
            return Err(mismatch());
        }
        {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            native.at(registry.sweep_controllers(&mut clock)?)?;
            if !registry
                .entries
                .get(&lease.preparation_id)
                .is_some_and(|e| e.clock_attempt == attempt)
            {
                return Err(mismatch());
            }
        }
        let mut report = HandyServerClockObservation {
            request_id: r.request_id.clone(),
            controller: lease.clone(),
            qualification,
            clock_domain: native.clock.domain.clone(),
            sent_boottime_ns: native.sent_ns,
            received_boottime_ns: native.clock.monotonic_ns,
            estimated_server_ms: native.clock.estimated_server_ms,
            uncertainty_ns: native.clock.uncertainty_ns,
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
            &ResponseBody::HandyServerClock(Box::new(report.clone())),
        )?;
        tx.commit().map_err(internal)?;
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        let now = registry.sweep_controllers(&mut clock);
        report.current = now.is_ok_and(|now| native.at(now).is_ok())
            && registry
                .entries
                .get(&lease.preparation_id)
                .is_some_and(|e| {
                    e.clock_attempt == attempt
                        && e.controller
                            .as_ref()
                            .is_some_and(|l| matches_request(r, &l.report))
                });
        if report.current {
            registry
                .entries
                .get_mut(&lease.preparation_id)
                .expect("checked entry")
                .server_clock = Some(RetainedClock {
                report: report.clone(),
                native,
            });
        }
        Ok(ResponseBody::HandyServerClock(Box::new(report)))
    }
}
