//! Fresh association to one acknowledged setup. This path performs read-only remote I/O.
use super::*;
use crate::{
    device_effect_journal::{SetupEffectObservation, SetupJournalState},
    handy_credentials::Credentials,
    handy_hosting_wire::VerifiedHosted,
};
use std::time::{Duration, Instant};
fn changed(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
impl Engine {
    pub(super) fn capture_program_setup(
        &self,
        db: &Connection,
        request: &Request,
        actor: &SessionId,
    ) -> PResult<(handy_setup::Captured, SetupEffectObservation)> {
        self.authorize(db, request, actor)?;
        let Command::InspectHandyProgram {
            setup_request_id,
            record_id,
            qualification,
            engine_epoch,
            device_session,
            hosting_request_id,
            ..
        } = &request.command
        else {
            return Err(ProtocolError::unsupported("Handy program inspection"));
        };
        let observed = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .setup_request_history(actor, setup_request_id)
            .map_err(internal)?
            .ok_or_else(|| {
                ProtocolError::new(ErrorCode::NotFound, "original setup history not found")
            })?;
        let record = &observed.record;
        if observed.id != record_id.as_str()
            || observed.state != SetupJournalState::Acknowledged
            || observed.acknowledgment.is_none()
            || record.preflight_mode.is_none()
            || record.project_id != *project(request)?
            || Some(record.revision) != request.expected_revision
            || record.boot != *engine_epoch
            || record.device_session != *device_session
            || record.hosting_request_id != *hosting_request_id
            || record.qualification != *qualification
        {
            return Err(changed(
                "program selection lacks matching acknowledged setup and captured mode history",
            ));
        }
        let captured = self.capture_setup(db, request, actor)?;
        let route = crate::HandyRouteIdentity::from_snapshot(
            &self.handy_owner_token,
            &captured.credentials.connection,
        )?;
        if record.registration != captured.connection.registration
            || record.master_sha256 != captured.hosting.master_sha256
            || record.script != captured.hosting.script
            || record.hosted != captured.hosted.downloaded
            || record.route_sha256 != route.route_sha256()
            || record.route_namespace_sha256.as_deref() != Some(route.namespace_sha256())
        {
            return Err(changed(
                "program references differ from the acknowledged setup",
            ));
        }
        Ok((captured, observed))
    }
    pub(super) fn handy_program_inspection(
        &self,
        request: &Request,
        actor: &SessionId,
    ) -> PResult<ResponseBody> {
        self.inspect_handy_program_with(
            request,
            actor,
            |hosted, deadline| {
                hosted.recheck(deadline).map_err(|_| {
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        "hosted program could not be freshly verified",
                    )
                })
            },
            crate::handy_online::inspect_program,
        )
    }
    pub(super) fn inspect_handy_program_with<R, D>(
        &self,
        request: &Request,
        actor: &SessionId,
        recheck: R,
        mut device: D,
    ) -> PResult<ResponseBody>
    where
        R: FnOnce(&VerifiedHosted, Instant) -> PResult<ArtifactIdentity>,
        D: FnMut(
            Arc<Credentials>,
            Instant,
        ) -> PResult<(HandySetupDeviceObservation, HandyHspBuffer)>,
    {
        request.validate()?;
        let started = Instant::now();
        let (captured, original) = {
            let db = self.db.lock().map_err(internal)?;
            self.capture_program_setup(&db, request, actor)?
        };
        let ack = original
            .acknowledgment
            .clone()
            .ok_or_else(|| changed("setup response missing"))?;
        let mode = original
            .record
            .preflight_mode
            .clone()
            .ok_or_else(|| changed("setup mode generation missing"))?;
        let mut buffers = Vec::with_capacity(2);
        let ResponseBody::HandySetupInspection(mut inspection) =
            self.inspect_handy_setup_with(request, actor, recheck, |keys, deadline| {
                let (sample, buffer) = device(keys, deadline)?;
                HandyProgramCorrelation::check_buffer(&sample, &buffer)?;
                if sample.mode != mode || buffer.stream_id != ack.stream_id {
                    return Err(changed(
                        "current mode generation or stream differs from acknowledged setup",
                    ));
                }
                if buffers.len() >= 2 {
                    return Err(internal("program inspection exceeded sample count"));
                }
                buffers.push(buffer);
                Ok(sample)
            })?
        else {
            return Err(internal("program inspection response differs"));
        };
        let _qualification_memory = self.pool.reserve_memory(16 * 1024 * 1024)?;
        let db = self.db.lock().map_err(internal)?;
        let (current, final_history) = self.capture_program_setup(&db, request, actor)?;
        if !Arc::ptr_eq(&captured.credentials, &current.credentials)
            || !captured.hosted.same_location(&current.hosted)
            || final_history.id != original.id
            || final_history.acknowledgment != original.acknowledgment
        {
            return Err(changed("program correspondence changed during inspection"));
        }
        let qualification = inspection
            .accepted_qualification
            .as_ref()
            .ok_or_else(|| internal("program inspection qualification missing"))?;
        let _facts = self.accepted_handy_timing(
            &db,
            qualification,
            &current.connection.registration,
            &inspection.device,
            &current.hosted,
        )?;
        if started.elapsed() >= Duration::from_secs(30) {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "program correlation deadline expired",
            ));
        }
        inspection.observation_span_ns =
            started.elapsed().as_nanos().try_into().map_err(internal)?;
        let [first_buffer, second_buffer]: [HandyHspBuffer; 2] = buffers
            .try_into()
            .map_err(|_| internal("program observation count differs"))?;
        let report = HandyProgramCorrelation {
            inspection: *inspection,
            setup_request_id: original.record.request_id,
            record_id: ArtifactId::new(original.id).map_err(internal)?,
            setup_mode: mode,
            acknowledgment: ack,
            expected_samples: current.hosted.sample_shape(),
            first_buffer,
            second_buffer,
            content_verified: false,
            playback_enabled: false,
        };
        report.validate()?;
        Ok(ResponseBody::HandyProgramCorrelation(Box::new(report)))
    }
}
