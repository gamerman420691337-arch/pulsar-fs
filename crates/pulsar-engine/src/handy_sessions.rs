//! Boot-local, owner-authorized connection observations. No playback authority.
use super::*;
use crate::handy_credentials::Credentials;

struct Entry {
    owner: SessionId,
    report: HandyConnectionSession,
    credentials: Arc<Credentials>,
    opening: bool,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: HashMap<DeviceSessionId, Entry>,
}
impl Registry {
    /// Reference capture only; the owning control path checks current consent and grants.
    pub(super) fn capture(
        &self,
        owner: &SessionId,
        id: &DeviceSessionId,
    ) -> PResult<(HandyConnectionSession, Arc<Credentials>)> {
        let entry = self.entries.get(id).ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, "Handy connection session not found")
        })?;
        if entry.owner != *owner
            || entry.opening
            || entry.report.state != HandyConnectionState::Observed
            || entry.report.device_session != *id
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "Handy connection is not a current observation for this client",
            ));
        }
        entry.report.validate()?;
        Ok((entry.report.clone(), entry.credentials.clone()))
    }
    pub(super) fn invalidate(&mut self, device: &DeviceId) {
        self.entries.retain(|_, entry| {
            if &entry.report.registration.device_id != device {
                return true;
            }
            entry.report.state = HandyConnectionState::Retired;
            entry.report.inspection = None;
            // Keep an opening route reserved until its bounded GET returns.
            entry.opening
        });
    }
}
struct Opening<'a> {
    registry: &'a Mutex<Registry>,
    id: DeviceSessionId,
}
impl Drop for Opening<'_> {
    fn drop(&mut self) {
        // Even a panicking inspector cannot strand its credential-route reservation.
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if registry.entries.get(&self.id).is_some_and(|e| e.opening) {
            registry.entries.remove(&self.id);
        }
    }
}
pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::OpenHandySession { .. }
            | Command::CloseHandySession { .. }
            | Command::HandySessions { .. }
    )
}
fn retired(mut report: HandyConnectionSession) -> HandyConnectionSession {
    report.state = HandyConnectionState::Retired;
    report.inspection = None;
    report
}
impl Engine {
    pub(super) fn handy_session_control(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        self.handy_session_control_with(
            request,
            session,
            fingerprint,
            |r| crate::handy_credentials::Credentials::from_environment(r).map(Arc::new),
            crate::handy_online::inspect_credentials,
        )
    }
    pub(super) fn handy_session_control_with<L, F>(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
        load: L,
        inspect: F,
    ) -> PResult<ResponseBody>
    where
        L: FnOnce(&HandyInspectionRequest) -> PResult<Arc<Credentials>>,
        F: FnOnce(Arc<Credentials>) -> PResult<HandyInspection>,
    {
        request.validate()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, session)?;
        check_request_identity(&db, session.as_str(), request, fingerprint)?;
        let epoch = DeviceSessionId::new(self.engine_epoch.clone()).map_err(internal)?;
        match &request.command {
            Command::OpenHandySession { engine_epoch, .. }
            | Command::CloseHandySession { engine_epoch, .. }
                if engine_epoch != &epoch =>
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "Handy connection belongs to a retired Engine boot",
                ));
            }
            _ => {}
        }
        let mut registry = self.handy_sessions.lock().map_err(internal)?;
        if matches!(request.command, Command::HandySessions { .. }) {
            let mut sessions: Vec<_> = registry
                .entries
                .values()
                .map(|e| e.report.clone())
                .collect();
            sessions.sort_by(|a, b| a.device_session.as_str().cmp(b.device_session.as_str()));
            let catalog = HandyConnectionCatalog {
                engine_epoch: epoch,
                sessions,
            };
            catalog.validate()?;
            return Ok(ResponseBody::HandySessions(Box::new(catalog)));
        }
        if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
            let ResponseBody::HandySession(saved) = body else {
                return Err(ProtocolError::invalid("Handy connection receipt mismatch"));
            };
            saved.validate()?;
            if saved.engine_epoch != epoch {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "Handy connection boot changed",
                ));
            }
            return match &request.command {
                Command::OpenHandySession {
                    device_id,
                    expected_registration_revision,
                    binding_sha256,
                    ..
                } => {
                    if saved.open_request_id != request.request_id
                        || saved.registration.device_id != *device_id
                        || saved.registration.revision != *expected_registration_revision
                        || saved.registration.binding_sha256 != *binding_sha256
                    {
                        return Err(ProtocolError::invalid(
                            "Handy open receipt correspondence mismatch",
                        ));
                    }
                    let report = registry
                        .entries
                        .get(&saved.device_session)
                        .filter(|e| e.owner == *session)
                        .map(|e| e.report.clone())
                        .unwrap_or_else(|| retired(*saved));
                    Ok(ResponseBody::HandySession(Box::new(report)))
                }
                Command::CloseHandySession { device_session, .. }
                    if saved.device_session == *device_session
                        && saved.state == HandyConnectionState::Retired =>
                {
                    Ok(ResponseBody::HandySession(saved))
                }
                _ => Err(ProtocolError::invalid(
                    "Handy close receipt correspondence mismatch",
                )),
            };
        }
        if let Command::CloseHandySession { device_session, .. } = &request.command {
            let entry = registry.entries.get(device_session).ok_or_else(|| {
                ProtocolError::new(ErrorCode::NotFound, "Handy connection session not found")
            })?;
            let body = ResponseBody::HandySession(Box::new(retired(entry.report.clone())));
            let tx = db.transaction().map_err(internal)?;
            remember(&tx, session.as_str(), request, fingerprint, &body)?;
            tx.commit().map_err(internal)?;
            self.retire_handy_preparations(handy_playback_preparation::Retirement::Connection(
                device_session,
            ));
            let entry = registry
                .entries
                .get_mut(device_session)
                .expect("locked entry");
            if entry.opening {
                entry.report = retired(entry.report.clone());
            } else {
                registry.entries.remove(device_session);
            }
            return Ok(body);
        }
        let Command::OpenHandySession {
            device_id,
            expected_registration_revision,
            binding_sha256,
            allow_online_inspection,
            ..
        } = &request.command
        else {
            return Err(ProtocolError::unsupported("Handy connection operation"));
        };
        let registration =
            handy_registrations::current(&db, device_id, *expected_registration_revision)?;
        if registration.disabled || registration.binding_sha256 != *binding_sha256 {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "Handy registration disabled or changed",
            ));
        }
        if registry.entries.len() >= HANDY_REGISTRATION_LIMIT
            || registry
                .entries
                .values()
                .any(|e| e.report.registration.device_id == *device_id)
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "Handy connection already reserved or session capacity reached",
            ));
        }
        let credentials = load(&HandyInspectionRequest {
            api_key_env: registration.settings.api_key_env.clone(),
            connection_key_env: registration.settings.connection_key_env.clone(),
            allow_online_inspection: *allow_online_inspection,
        })?;
        credentials.validate()?;
        if registry
            .entries
            .values()
            .any(|e| e.credentials.same_route(&credentials))
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "Handy credential route already reserved",
            ));
        }
        let _reservation = self.pool.reserve_memory(1024 * 1024)?;
        let id = DeviceSessionId::new(Uuid::new_v4().to_string()).map_err(internal)?;
        let report = HandyConnectionSession {
            engine_epoch: epoch,
            device_session: id.clone(),
            open_request_id: request.request_id.clone(),
            registration: registration.clone(),
            state: HandyConnectionState::Opening,
            inspection: None,
        };
        report.validate()?;
        let body = ResponseBody::HandySession(Box::new(report.clone()));
        let tx = db.transaction().map_err(internal)?;
        remember(&tx, session.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        registry.entries.insert(
            id.clone(),
            Entry {
                owner: session.clone(),
                report,
                credentials: credentials.clone(),
                opening: true,
            },
        );
        drop(registry);
        drop(db);
        let _opening = Opening {
            registry: &self.handy_sessions,
            id: id.clone(),
        };
        let inspection = inspect(credentials.clone())?;
        inspection.validate()?;
        if !inspection.connected {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "Handy is not connected",
            ));
        }
        if inspection
            .identity
            .as_ref()
            .is_some_and(|i| credentials.echoed(i))
        {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "credential-bearing Handy identity was withheld",
            ));
        }
        let db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, session)?;
        if handy_registrations::current(&db, device_id, *expected_registration_revision)?
            != registration
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "Handy registration changed during opening",
            ));
        }
        let mut registry = self.handy_sessions.lock().map_err(internal)?;
        let entry = registry
            .entries
            .get_mut(&id)
            .ok_or_else(|| ProtocolError::new(ErrorCode::Cancelled, "Handy opening retired"))?;
        if entry.report.state != HandyConnectionState::Opening {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "Handy opening retired",
            ));
        }
        entry.report.state = HandyConnectionState::Observed;
        entry.report.inspection = Some(inspection);
        entry.report.validate()?;
        entry.opening = false;
        Ok(ResponseBody::HandySession(Box::new(entry.report.clone())))
    }
}
