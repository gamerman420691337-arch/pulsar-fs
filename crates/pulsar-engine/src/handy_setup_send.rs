//! Explicit setup handoff. Journal claims precede I/O; acknowledgments grant no playback.
use super::*;
use crate::{
    device_effect_journal::SetupCompletion, device_effect_journal::SetupEffectRecord,
    handy_credentials::Credentials, handy_hosting_wire::VerifiedHosted,
};
use std::time::{Duration, Instant};
pub(super) fn validate_journal_links(db: &Connection, state_dir: &std::path::Path) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='requests')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let mut q=db.prepare("SELECT CASE WHEN length(CAST(session AS BLOB))<=128 THEN session ELSE NULL END,CASE WHEN length(CAST(request AS BLOB))<=128 THEN request ELSE NULL END,CASE WHEN length(CAST(response AS BLOB))<=16384 THEN response ELSE NULL END FROM requests WHERE CASE WHEN json_valid(response) THEN json_extract(response,'$.kind')='handy_setup_status' AND json_type(response,'$.value.history') IS NOT 'null' ELSE response LIKE '%handy_setup_status%' END LIMIT 1025")?;
    let rows = q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut witnesses = Vec::new();
    for row in rows {
        let (actor, id, raw) = row?;
        let actor = SessionId::new(actor)?;
        let id = RequestId::new(id)?;
        let ResponseBody::HandySetupStatus(status) = serde_json::from_str(&raw)? else {
            anyhow::bail!("invalid setup journal witness")
        };
        status.validate()?;
        anyhow::ensure!(
            status.setup_request_id == id,
            "setup witness request differs"
        );
        let h = status
            .history
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("setup witness is incomplete"))?;
        witnesses.push((actor, id, h.record_id.as_str().to_owned()));
    }
    anyhow::ensure!(
        witnesses.len() <= 1024,
        "setup witnesses exceed journal capacity"
    );
    crate::device_effect_journal::DeviceEffectJournal::verify_setup_records(state_dir, &witnesses)
}
impl Engine {
    fn reserve_setup_request(
        db: &mut Connection,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
        known_record: Option<&str>,
    ) -> PResult<()> {
        if let Some(body) = replay(db, session.as_str(), request, fingerprint)? {
            let ResponseBody::HandySetupStatus(value) = body else {
                return Err(internal("setup intent reservation type differs"));
            };
            value.validate()?;
            if value.project_id != *project(request)?
                || value.setup_request_id != request.request_id
                || value
                    .history
                    .as_ref()
                    .is_some_and(|h| Some(h.record_id.as_str()) != known_record)
            {
                return Err(internal("setup intent reservation differs"));
            }
        } else {
            let tx = db.transaction().map_err(internal)?;
            let reserved = ResponseBody::HandySetupStatus(Box::new(HandySetupStatus {
                project_id: project(request)?.clone(),
                setup_request_id: request.request_id.clone(),
                history: None,
                loaded: false,
                qualified: false,
            }));
            remember(&tx, session.as_str(), request, fingerprint, &reserved)?;
            tx.commit().map_err(internal)?;
        }
        Ok(())
    }
    fn link_setup_witness(
        db: &mut Connection,
        request: &Request,
        session: &SessionId,
        observed: crate::device_effect_journal::SetupEffectObservation,
    ) -> PResult<()> {
        let tx = db.transaction().map_err(internal)?;
        let raw: String = tx
            .query_row(
                "SELECT response FROM requests WHERE session=?1 AND request=?2",
                params![session.as_str(), request.request_id.as_str()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        let ResponseBody::HandySetupStatus(previous) = decode(&raw)? else {
            return Err(internal("setup witness reservation type differs"));
        };
        if let Some(history) = &previous.history {
            if history.record_id.as_str() != observed.id {
                return Err(internal("setup witness identity differs"));
            }
            return Ok(());
        }
        let witness = handy_setup::status_from_observation(
            project(request)?.clone(),
            request.request_id.clone(),
            Some(observed),
        )?;
        let changed = tx
            .execute(
                "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                params![
                    encode(&ResponseBody::HandySetupStatus(Box::new(witness)))?,
                    session.as_str(),
                    request.request_id.as_str()
                ],
            )
            .map_err(internal)?;
        if changed != 1 {
            return Err(internal("setup intent reservation disappeared"));
        }
        tx.commit().map_err(internal)?;
        Ok(())
    }
    fn setup_history_reply(&self, request: &Request, session: &SessionId) -> PResult<ResponseBody> {
        let Command::SetupHandyDevice { owner_proof, .. } = &request.command else {
            return Err(ProtocolError::unsupported("setup operation"));
        };
        let mut query = request.clone();
        query.expected_revision = None;
        query.command = Command::HandySetupStatus {
            setup_request_id: request.request_id.clone(),
            owner_proof: owner_proof.clone(),
        };
        self.handy_setup_status(&query, session)
    }
    fn validate_setup_retry(request: &Request, record: &SetupEffectRecord) -> PResult<()> {
        let Command::SetupHandyDevice {
            qualification,
            engine_epoch,
            device_session,
            hosting_request_id,
            ..
        } = &request.command
        else {
            return Err(ProtocolError::unsupported("setup operation"));
        };
        if record.project_id != *project(request)?
            || Some(record.revision) != request.expected_revision
            || record.boot != *engine_epoch
            || record.device_session != *device_session
            || record.qualification != *qualification
            || record.hosting_request_id != *hosting_request_id
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "setup retry differs from original selection",
            ));
        }
        Ok(())
    }
    fn current_setup_sender(
        &self,
        db: &Connection,
        request: &Request,
        session: &SessionId,
        prior: &handy_setup::Captured,
        observed: &HandySetupDeviceObservation,
    ) -> PResult<()> {
        let current = self.capture_setup(db, request, session)?;
        if current.connection.device_session != prior.connection.device_session
            || current.connection.registration != prior.connection.registration
            || !Arc::ptr_eq(&current.credentials, &prior.credentials)
            || !current.hosted.same_location(&prior.hosted)
            || current.hosting.request_id != prior.hosting.request_id
            || current.hosting.script != prior.hosting.script
            || current.hosting.master_sha256 != prior.hosting.master_sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "setup references changed before handoff",
            ));
        }
        let Command::SetupHandyDevice { qualification, .. } = &request.command else {
            return Err(ProtocolError::unsupported("setup operation"));
        };
        let _facts = self.accepted_handy_timing(
            db,
            qualification,
            &current.connection.registration,
            observed,
            &current.hosted,
        )?;
        Ok(())
    }
    pub(super) fn handy_setup_send(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        self.handy_setup_send_with(
            request,
            session,
            fingerprint,
            |hosted, deadline| {
                hosted.recheck(deadline).map_err(|_| {
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        "hosted script could not be freshly verified",
                    )
                })
            },
            crate::handy_online::inspect_setup,
            crate::handy_setup_wire::send,
        )
    }
    pub(super) fn handy_setup_send_with<R, D, S>(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
        recheck: R,
        device: D,
        send: S,
    ) -> PResult<ResponseBody>
    where
        R: FnOnce(&VerifiedHosted, Instant) -> PResult<ArtifactIdentity>,
        D: FnMut(Arc<Credentials>, Instant) -> PResult<HandySetupDeviceObservation>,
        S: FnOnce(&VerifiedHosted, Arc<Credentials>, u64, Instant) -> SetupCompletion,
    {
        request.validate()?;
        let Command::SetupHandyDevice { qualification, .. } = &request.command else {
            return Err(ProtocolError::unsupported("setup operation"));
        };
        {
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            check_request_identity(&db, session.as_str(), request, fingerprint)?;
            let existing = self
                ._device_effect_journal
                .lock()
                .map_err(internal)?
                .setup_request_history(session, &request.request_id)
                .map_err(internal)?;
            if let Some(existing) = existing {
                Self::validate_setup_retry(request, &existing.record)?;
                Self::reserve_setup_request(
                    &mut db,
                    request,
                    session,
                    fingerprint,
                    Some(&existing.id),
                )?;
                Self::link_setup_witness(&mut db, request, session, existing)?;
                drop(db);
                return self.setup_history_reply(request, session);
            }
        }
        {
            let db = self.db.lock().map_err(internal)?;
            validate_journal_links(&db, &self.config.state_dir).map_err(internal)?;
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        let ResponseBody::HandySetupInspection(inspection) =
            self.inspect_handy_setup_with(request, session, recheck, device)?
        else {
            return Err(internal("setup preflight response mismatch"));
        };
        let remote=inspection.device.remote_hssp.as_ref().filter(|state| inspection.device.mode.mode==1 && matches!(state.play_state,HandyRemotePlayState::NotInitialized|HandyRemotePlayState::Stopped) && state.playback_rate==1.0 && !state.looping)
            .ok_or_else(||ProtocolError::new(ErrorCode::Forbidden,"setup requires an existing HSSP mode with no active playback, normal rate and no looping"))?;
        let previous_stream = remote.stream_id;
        let _memory = self.pool.reserve_memory(16 * 1024 * 1024)?;
        let (captured, claim) = {
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            check_request_identity(&db, session.as_str(), request, fingerprint)?;
            let captured = self.capture_setup(&db, request, session)?;
            if captured.connection.registration != inspection.registration
                || captured.hosting.script != inspection.script
                || captured.hosted.downloaded != inspection.hosted
                || captured.hosting.master_sha256 != inspection.master_sha256
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "setup preflight binding changed",
                ));
            }
            self.current_setup_sender(&db, request, session, &captured, &inspection.device)?;
            if Instant::now() >= deadline {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "setup deadline expired before preparation",
                ));
            }
            captured.hosted.setup_body().map_err(|_| {
                ProtocolError::new(ErrorCode::Unavailable, "setup hosting location unavailable")
            })?;
            let route = crate::HandyRouteIdentity::from_snapshot(
                &self.handy_owner_token,
                &captured.credentials.connection,
            )?;
            let record = SetupEffectRecord {
                boot: captured.connection.engine_epoch.clone(),
                device_session: captured.connection.device_session.clone(),
                actor_session: session.clone(),
                request_id: request.request_id.clone(),
                hosting_request_id: captured.hosting.request_id.clone(),
                project_id: project(request)?.clone(),
                revision: captured.hosting.revision,
                registration: captured.connection.registration.clone(),
                qualification: qualification.clone(),
                master_sha256: captured.hosting.master_sha256.clone(),
                script: captured.hosting.script.clone(),
                hosted: captured.hosted.downloaded.clone(),
                route_sha256: route.route_sha256().into(),
                route_namespace_sha256: Some(route.namespace_sha256().into()),
                acknowledgment_required: true,
                preflight_mode: Some(inspection.device.mode.clone()),
            };
            let mut journal = self._device_effect_journal.lock().map_err(internal)?;
            if let Some(existing) = journal
                .setup_request_history(session, &request.request_id)
                .map_err(internal)?
            {
                Self::validate_setup_retry(request, &existing.record)?;
                Self::reserve_setup_request(
                    &mut db,
                    request,
                    session,
                    fingerprint,
                    Some(&existing.id),
                )?;
                Self::link_setup_witness(&mut db, request, session, existing)?;
                drop(journal);
                drop(db);
                return self.setup_history_reply(request, session);
            }
            // Reserve the global actor/request fingerprint before the separate journal.
            // A crash in the gap cannot send: no setup claim has been produced.
            Self::reserve_setup_request(&mut db, request, session, fingerprint, None)?;
            let prepared = journal.prepare_setup(record).map_err(|_| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "setup preparation refused; inspect original history and reservations",
                )
            })?;
            let observed = journal
                .setup_request_history(session, &request.request_id)
                .map_err(internal)?
                .ok_or_else(|| internal("prepared setup disappeared"))?;
            Self::link_setup_witness(&mut db, request, session, observed)?;
            if Instant::now() >= deadline {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "setup deadline expired before claim; preparation retained",
                ));
            }
            let claim = journal.claim_setup(&prepared).map_err(|_| {
                ProtocolError::new(
                    ErrorCode::UnknownOutcome,
                    "setup claim unavailable; inspect original history",
                )
            })?;
            (captured, claim)
        };
        // No project, session-registry or journal lock is held during HTTP I/O.
        // Once claimed, late revocation cannot retract a request already handed off.
        let current = {
            let db = self.db.lock().map_err(internal)?;
            self.current_setup_sender(&db, request, session, &captured, &inspection.device)
        };
        let outcome = if current.is_ok() && Instant::now() < deadline {
            send(
                &captured.hosted,
                captured.credentials,
                previous_stream,
                deadline,
            )
        } else {
            SetupCompletion::Unknown
        };
        self._device_effect_journal
            .lock()
            .map_err(internal)?
            .complete_setup(&claim, outcome)
            .map_err(|_| {
                ProtocolError::new(
                    ErrorCode::UnknownOutcome,
                    "setup observation could not be recorded; do not repeat the effect",
                )
            })?;
        // Return only historical metadata, even if grants or qualification changed during I/O.
        self.setup_history_reply(request, session)
    }
}
