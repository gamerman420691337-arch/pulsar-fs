//! One bounded read-only inspection across current script and device references.
use super::*;
use crate::{handy_credentials::Credentials, handy_hosting_wire::VerifiedHosted};
use std::time::{Duration, Instant};
const INSPECTION_BUDGET: Duration = Duration::from_secs(30);
#[derive(Clone)]
pub(super) struct Captured {
    pub(super) connection: HandyConnectionSession,
    pub(super) credentials: Arc<Credentials>,
    pub(super) hosting: HandyHostingReport,
    pub(super) hosted: VerifiedHosted,
}
fn changed(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::DependencyMismatch, message)
}
pub(super) fn status_from_observation(
    project_id: ProjectId,
    setup_request_id: RequestId,
    observed: Option<crate::device_effect_journal::SetupEffectObservation>,
) -> PResult<HandySetupStatus> {
    let history = observed
        .map(|value| -> PResult<HandySetupHistory> {
            let record = value.record;
            if record.project_id != project_id {
                return Err(changed("original setup belongs to another project"));
            }
            use crate::device_effect_journal::SetupJournalState as J;
            let state = match value.state {
                J::Prepared => HandySetupState::Prepared,
                J::Claimed => HandySetupState::Claimed,
                J::Acknowledged => HandySetupState::Acknowledged,
                J::Unknown => HandySetupState::Unknown,
                J::Cancelled => HandySetupState::Cancelled,
            };
            Ok(HandySetupHistory {
                record_id: ArtifactId::new(value.id).map_err(internal)?,
                engine_epoch: record.boot,
                device_session: record.device_session,
                revision: record.revision,
                device_id: record.registration.device_id,
                registration_revision: record.registration.revision,
                qualification: record.qualification,
                hosting_request_id: record.hosting_request_id,
                master_sha256: record.master_sha256,
                script: record.script,
                hosted: record.hosted,
                state,
                acknowledgment: value.acknowledgment,
            })
        })
        .transpose()?;
    let status = HandySetupStatus {
        project_id,
        setup_request_id: setup_request_id.clone(),
        history,
        loaded: false,
        qualified: false,
    };
    status.validate()?;
    Ok(status)
}
impl Engine {
    pub(super) fn handy_setup_status(
        &self,
        request: &Request,
        session: &SessionId,
    ) -> PResult<ResponseBody> {
        request.validate()?;
        let Command::HandySetupStatus {
            setup_request_id, ..
        } = &request.command
        else {
            return Err(ProtocolError::unsupported("setup status"));
        };
        let db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, session)?;
        let project_id = project(request)?.clone();
        let observed = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .setup_request_history(session, setup_request_id)
            .map_err(internal)?;
        let status = status_from_observation(project_id, setup_request_id.clone(), observed)?;
        Ok(ResponseBody::HandySetupStatus(Box::new(status)))
    }
    pub(super) fn capture_setup(
        &self,
        db: &Connection,
        request: &Request,
        session: &SessionId,
    ) -> PResult<Captured> {
        self.capture_setup_during_sync(db, request, session, None)
    }
    pub(super) fn capture_setup_during_sync(
        &self,
        db: &Connection,
        request: &Request,
        session: &SessionId,
        claim: Option<&crate::device_effect_journal::ClaimedClockSyncReceipt>,
    ) -> PResult<Captured> {
        self.authorize(db, request, session)?;
        check_revision(db, request)?;
        let (engine_epoch, device_session, hosting_request_id) = match &request.command {
            Command::InspectHandySetup {
                engine_epoch,
                device_session,
                hosting_request_id,
                ..
            }
            | Command::InspectHandyProgram {
                engine_epoch,
                device_session,
                hosting_request_id,
                ..
            }
            | Command::SetupHandyDevice {
                engine_epoch,
                device_session,
                hosting_request_id,
                ..
            } => (engine_epoch, device_session, hosting_request_id),
            _ => return Err(ProtocolError::unsupported("Handy setup selection")),
        };
        if engine_epoch.as_str() != self.engine_epoch {
            return Err(changed(
                "Handy setup inspection belongs to a retired Engine boot",
            ));
        }
        let revision = request
            .expected_revision
            .ok_or_else(|| ProtocolError::invalid("setup inspection needs a project revision"))?;
        let (connection, credentials) = self
            .handy_sessions
            .lock()
            .map_err(internal)?
            .capture(session, device_session)?;
        if connection.engine_epoch != *engine_epoch {
            return Err(changed("Handy connection epoch changed"));
        }
        let route = crate::HandyRouteIdentity::from_snapshot(
            &self.handy_owner_token,
            &credentials.connection,
        )?;
        self._device_effect_journal
            .lock()
            .map_err(internal)?
            .clock_sync_route_available(
                route.route_sha256(),
                route.namespace_sha256(),
                &connection.registration.device_id,
                claim,
            )
            .map_err(|_| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "device route has unresolved clock synchronization",
                )
            })?;
        if handy_registrations::current(
            db,
            &connection.registration.device_id,
            connection.registration.revision,
        )? != connection.registration
        {
            return Err(changed("Handy registration changed"));
        }
        let (hosting, hosted) = self.capture_handy_hosted(
            db,
            session,
            project(request)?,
            revision,
            hosting_request_id,
        )?;
        Ok(Captured {
            connection,
            credentials,
            hosting,
            hosted,
        })
    }
    pub(super) fn handy_setup_inspection(
        &self,
        request: &Request,
        session: &SessionId,
    ) -> PResult<ResponseBody> {
        self.inspect_handy_setup_with(
            request,
            session,
            |hosted, deadline| {
                hosted.recheck(deadline).map_err(|_| {
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        "hosted script could not be freshly verified",
                    )
                })
            },
            crate::handy_online::inspect_setup,
        )
    }
    pub(super) fn inspect_handy_setup_with<R, D>(
        &self,
        request: &Request,
        session: &SessionId,
        recheck: R,
        mut device: D,
    ) -> PResult<ResponseBody>
    where
        R: FnOnce(&VerifiedHosted, Instant) -> PResult<ArtifactIdentity>,
        D: FnMut(Arc<Credentials>, Instant) -> PResult<HandySetupDeviceObservation>,
    {
        request.validate()?;
        let selection = match &request.command {
            Command::InspectHandySetup { qualification, .. } => qualification.clone(),
            Command::SetupHandyDevice { qualification, .. }
            | Command::InspectHandyProgram { qualification, .. } => Some(qualification.clone()),
            _ => return Err(ProtocolError::unsupported("Handy setup inspection")),
        };
        let _qualification_memory = selection
            .as_ref()
            .map(|_| self.pool.reserve_memory(16 * 1024 * 1024))
            .transpose()?;
        let started = Instant::now();
        let deadline = started + INSPECTION_BUDGET;
        let captured = {
            let db = self.db.lock().map_err(internal)?;
            self.capture_setup(&db, request, session)?
        };
        let _memory = self.pool.reserve_memory(20 * 1024 * 1024)?;
        let expected_settings = if let Some(binding) = &selection {
            let db = self.db.lock().map_err(internal)?;
            let identity = captured
                .connection
                .inspection
                .as_ref()
                .and_then(|i| i.identity.as_ref())
                .ok_or_else(|| changed("connection identity missing"))?;
            Some(self.accepted_handy_settings(
                &db,
                binding,
                &captured.connection.registration,
                identity,
                &captured.hosted,
            )?)
        } else {
            None
        };
        let first = device(captured.credentials.clone(), deadline)?;
        first.validate()?;
        if expected_settings
            .as_ref()
            .is_some_and(|s| *s != first.operative_settings)
        {
            return Err(changed(
                "observed Handy settings differ from accepted qualification",
            ));
        }
        if captured
            .connection
            .inspection
            .as_ref()
            .and_then(|i| i.identity.as_ref())
            != Some(&first.identity)
        {
            return Err(changed(
                "Handy identity changed; reopen the connection observation",
            ));
        }
        let hosted = recheck(&captured.hosted, deadline)?;
        if captured.hosting.downloaded.as_ref() != Some(&hosted) {
            return Err(changed("hosted script observation changed"));
        }
        let second = device(captured.credentials.clone(), deadline)?;
        second.validate()?;
        if !first.same_state(&second) {
            return Err(changed(
                "Handy identity, mode generation, operative settings or remote playback state changed during inspection",
            ));
        }
        let db = self.db.lock().map_err(internal)?;
        let current = self.capture_setup(&db, request, session)?;
        if current.connection.device_session != captured.connection.device_session
            || current.connection.registration != captured.connection.registration
            || !Arc::ptr_eq(&current.credentials, &captured.credentials)
            || !current.hosted.same_location(&captured.hosted)
            || current.hosting.master_sha256 != captured.hosting.master_sha256
            || current.hosting.script != captured.hosting.script
        {
            return Err(changed("Handy setup references changed during inspection"));
        }
        if Instant::now() >= deadline {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "Handy setup inspection deadline expired",
            ));
        }
        if let Some(binding) = &selection {
            let _facts = self.accepted_handy_timing(
                &db,
                binding,
                &current.connection.registration,
                &second,
                &current.hosted,
            )?;
        }
        if Instant::now() >= deadline {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "qualification inspection deadline expired",
            ));
        }
        let result = HandySetupInspection {
            accepted_qualification: selection,
            request_id: request.request_id.clone(),
            engine_epoch: captured.connection.engine_epoch,
            project_id: project(request)?.clone(),
            revision: captured.hosting.revision,
            device_session: captured.connection.device_session,
            registration: captured.connection.registration,
            hosting_request_id: captured.hosting.request_id,
            master_sha256: captured.hosting.master_sha256,
            script: captured.hosting.script,
            hosted,
            device: second,
            observation_span_ns: started.elapsed().as_nanos().try_into().map_err(internal)?,
            setup_performed: false,
            qualified: false,
        };
        result.validate()?;
        Ok(ResponseBody::HandySetupInspection(Box::new(result)))
    }
}
