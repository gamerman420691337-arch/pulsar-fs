//! Boot-local preparation and controller custody. Physical dispatch remains disabled.
use super::*;
use crate::{
    handy_credentials::Credentials,
    handy_hosting_wire::VerifiedHosted,
    playback_runtime::{
        dispatch::{HandyTransport, TransportBinding},
        HandyPlaybackRuntime,
    },
};
use std::time::Instant;
#[path = "handy_controller.rs"]
mod controller;
#[path = "handy_execution.rs"]
mod execution;
#[path = "handy_native_sync.rs"]
mod native_sync;
#[path = "handy_reconciliation_retirement.rs"]
mod reconciliation_retirement;
pub(super) use controller::clock_sync::validate_sync_journal_links;
pub(super) use controller::play::validate_play_journal_links;
pub(super) use controller::ControllerMonitor;
pub(super) use execution::ExecutionPump;
struct Entry {
    owner: SessionId,
    report: HandyPlaybackPreparation,
    captured: handy_setup::Captured,
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    transport: Arc<HandyTransport>,
    execution_pending: Arc<std::sync::atomic::AtomicBool>,
    stop_activity: Arc<Mutex<Option<HandyStopActivity>>>,
    controller: Option<controller::Lease>,
    server_clock: Option<controller::server_clock::RetainedClock>,
    playback_clock: Option<controller::playback_clock::RetainedPlaybackClock>,
    continuous_timing: Option<controller::continuous_timing::RetainedContinuousTiming>,
    player_guard: Option<controller::player_guard::RetainedPlayerGuard>,
    continuous_attempt: u64,
    clock_attempt: u64,
    pending_controller_stop: Option<crate::playback_runtime::PlaybackEffect>,
    play: Option<Arc<controller::play::PlayPins>>,
    pending_play: Option<controller::play::PlayRequest>,
    browser_owner: Option<Arc<controller::browser_play::Owner>>,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: HashMap<ArtifactId, Entry>,
    controller_tick: Option<u64>,
    controller_clock_failed: bool,
    syncs: HashMap<ArtifactId, native_sync::Slot>,
    browser_owners: HashMap<(SessionId, RequestId), Arc<controller::browser_play::Owner>>,
}
#[cfg(test)]
impl Registry {
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }
    pub(super) fn native_sync_counts(&self) -> (usize, usize) {
        (
            self.syncs.len(),
            self.syncs.values().filter(|s| s.native.is_some()).count(),
        )
    }
}
#[derive(Clone, Copy)]
pub(super) enum Retirement<'a> {
    Id(&'a ArtifactId),
    Device(&'a DeviceId),
    Connection(&'a DeviceSessionId),
    Qualification(&'a ArtifactId),
    Hosting(&'a SessionId, &'a RequestId),
}
fn retired(mut report: HandyPlaybackPreparation) -> HandyPlaybackPreparation {
    report.state = HandyPlaybackPreparationState::Retired;
    report
}
fn replay_matches(r: &Request, saved: &HandyPlaybackPreparation) -> PResult<()> {
    saved.validate()?;
    let Command::PrepareHandyPlayback { program, .. } = &r.command else {
        return Err(internal("preparation command differs"));
    };
    if saved.request_id != r.request_id
        || saved.correlation.inspection.project_id != *project(r)?
        || Some(saved.correlation.inspection.revision) != r.expected_revision
        || saved.selection()? != *program
    {
        return Err(internal("preparation replay changed selection"));
    }
    Ok(())
}
impl Engine {
    pub(in crate::authority) fn retire_handy_for_shutdown(&self) {
        let mut registry = self
            .handy_preparations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        registry.syncs.clear();
        registry
            .entries
            .retain(|_, entry| !controller::retire_entry(entry));
    }
    /// Every caller holds the project DB guard. Unresolved physical uncertainty is retained.
    pub(super) fn retire_handy_preparations(&self, reason: Retirement<'_>) {
        self.handy_hsp_preparations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .invalidate(reason);
        let mut registry = self.handy_preparations.lock().unwrap_or_else(|e| {
            let mut g = e.into_inner();
            g.controller_clock_failed = true;
            g.syncs.clear();
            g.entries.retain(|_, e| !controller::retire_entry(e));
            g
        });
        registry.retire_syncs(reason);
        registry.entries.retain(|id, e| {
            let i = &e.report.correlation.inspection;
            let remove = match reason {
                Retirement::Id(wanted) => id == wanted,
                Retirement::Device(wanted) => &i.registration.device_id == wanted,
                Retirement::Connection(wanted) => &i.device_session == wanted,
                Retirement::Qualification(wanted) => i
                    .accepted_qualification
                    .as_ref()
                    .is_some_and(|q| &q.dossier_id == wanted),
                Retirement::Hosting(owner, request) => {
                    &e.owner == owner && &i.hosting_request_id == request
                }
            };
            !remove || !controller::retire_entry(e)
        });
    }
    pub(super) fn retire_changed_handy_preparations(&self, db: &Connection) {
        self.handy_hsp_preparations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .invalidate_changed(db);
        let subjects = {
            let registry = self
                .handy_preparations
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            registry
                .entries
                .iter()
                .map(|(id, e)| {
                    (
                        id.clone(),
                        e.owner.clone(),
                        e.report.correlation.inspection.project_id.clone(),
                        e.report.correlation.inspection.revision,
                        e.player_guard.as_ref().map(|g| g.auth_hash.clone()),
                    )
                })
                .chain(registry.syncs.values().map(|s| {
                    let c = &s.context;
                    (
                        c.preparation.preparation_id.clone(),
                        c.record.actor_session.clone(),
                        c.record.original_controller.project_id.clone(),
                        c.record.original_controller.revision,
                        None,
                    )
                }))
                .collect::<Vec<_>>()
        };
        // The caller holds DB ownership; do not hold the expiry registry during database reads.
        for (id, owner, project, revision, auth_hash) in subjects {
            let mut request = Request::new(
                RequestId::new("preparation-retirement-check").expect("constant ID"),
                Command::GetSnapshot,
            );
            request.project = Some(project.clone());
            request.expected_revision = Some(revision);
            // Native fast requests cannot discover arbitrary external SQL edits.
            // Public mutations complete this invalidation before returning.
            let session_changed = auth_hash.is_some_and(|expected| {
                db.query_row(
                    "SELECT token_hash FROM sessions WHERE id=?1",
                    [owner.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_or(true, |current| current.as_ref() != Some(&expected))
            });
            if session_changed
                || check_revision(db, &request).is_err()
                || require_grant(db, &owner, &project, Scope::Read).is_err()
                || require_grant(db, &owner, &project, Scope::Export).is_err()
            {
                self.retire_handy_preparations(Retirement::Id(&id));
            }
        }
    }
    fn preparation_inspection(&self, r: &Request, selection: &HandyProgramSelection) -> Request {
        let mut query = r.clone();
        query.command = selection.inspection_command(self.handy_owner_token.clone());
        query
    }
    fn saved_preparation(
        db: &Connection,
        actor: &SessionId,
        project_id: &ProjectId,
        original: &RequestId,
    ) -> PResult<HandyPlaybackPreparation> {
        let raw:Option<String>=db.query_row("SELECT CASE WHEN length(CAST(response AS BLOB))<=65536 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",params![actor.as_str(),original.as_str()],|r|r.get(0)).optional().map_err(internal)?;
        let raw = raw.ok_or_else(|| {
            ProtocolError::new(ErrorCode::NotFound, "original Handy preparation not found")
        })?;
        let ResponseBody::HandyPlaybackPreparation(report) = decode(&raw)? else {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "original request is not a Handy preparation",
            ));
        };
        report.validate()?;
        if report.request_id != *original || report.correlation.inspection.project_id != *project_id
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "preparation belongs to another request or project",
            ));
        }
        Ok(*report)
    }
    fn preparation_references(
        &self,
        db: &Connection,
        actor: &SessionId,
        saved: &HandyPlaybackPreparation,
        authenticated: &Request,
    ) -> PResult<(handy_setup::Captured, bool)> {
        let _memory = self.pool.reserve_memory(16 * 1024 * 1024)?;
        let i = &saved.correlation.inspection;
        let mut q = Request::new(
            saved.request_id.clone(),
            saved
                .selection()?
                .inspection_command(self.handy_owner_token.clone()),
        );
        q.session = Some(actor.clone());
        q.auth_token = authenticated.auth_token.clone();
        q.project = Some(i.project_id.clone());
        q.expected_revision = Some(i.revision);
        let (captured, _) = self.capture_program_setup(db, &q, actor)?;
        let timing = self.accepted_handy_timing(
            db,
            i.accepted_qualification
                .as_ref()
                .ok_or_else(|| internal("preparation qualification missing"))?,
            &captured.connection.registration,
            &i.device,
            &captured.hosted,
        )?;
        if timing.configuration() != &saved.admission.configuration {
            return Err(internal("prepared qualification configuration changed"));
        }
        let history = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .has_device_effects(&i.registration.device_id)
            .map_err(internal)?;
        Ok::<_, ProtocolError>((captured, history))
    }
    fn preparation_view(
        &self,
        db: &Connection,
        actor: &SessionId,
        saved: HandyPlaybackPreparation,
        authenticated: &Request,
    ) -> PResult<HandyPlaybackPreparation> {
        saved.validate()?;
        self.retire_changed_handy_preparations(db);
        let present = self
            .handy_preparations
            .lock()
            .map_err(internal)?
            .entries
            .get(&saved.preparation_id)
            .is_some_and(|e| e.owner == *actor && e.controller.is_none());
        if !present {
            return Ok(retired(saved));
        }
        let checked = self.preparation_references(db, actor, &saved, authenticated);
        let Ok((current, history)) = checked else {
            self.retire_handy_preparations(Retirement::Id(&saved.preparation_id));
            return Ok(retired(saved));
        };
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        let valid = registry
            .entries
            .get(&saved.preparation_id)
            .is_some_and(|e| {
                let Ok(runtime) = e.runtime.lock() else {
                    return false;
                };
                e.owner == *actor
                    && encode(&e.report)
                        .is_ok_and(|text| encode(&saved).is_ok_and(|other| text == other))
                    && e.captured.hosted.same_location(&current.hosted)
                    && Arc::ptr_eq(&e.captured.credentials, &current.credentials)
                    && runtime.session().state() == DeviceState::Disarmed
                    && runtime.session().controller().is_none()
                    && runtime.session().admission().is_none()
                    && runtime.session().physical_stop_pending() == history
                    && e.transport.preparation_matches(
                        &saved.admission,
                        &current.connection.device_session,
                        saved.correlation.acknowledgment.stream_id,
                        &current.credentials,
                    )
            });
        if !valid {
            registry.entries.remove(&saved.preparation_id);
            return Ok(retired(saved));
        }
        Ok(saved)
    }
    pub(super) fn handy_preparation_control(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        self.handy_preparation_with(
            r,
            actor,
            fingerprint,
            |h, d| {
                h.recheck(d).map_err(|_| {
                    ProtocolError::new(
                        ErrorCode::Unavailable,
                        "hosted preparation could not be verified",
                    )
                })
            },
            crate::handy_online::inspect_program,
        )
    }
    pub(super) fn handy_preparation_with<R, D>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        recheck: R,
        device: D,
    ) -> PResult<ResponseBody>
    where
        R: FnOnce(&VerifiedHosted, Instant) -> PResult<ArtifactIdentity>,
        D: FnMut(
            Arc<Credentials>,
            Instant,
        ) -> PResult<(HandySetupDeviceObservation, HandyHspBuffer)>,
    {
        r.validate()?;
        {
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            if let Command::HandyPlaybackPreparationStatus {
                preparation_request_id,
                ..
            } = &r.command
            {
                let saved =
                    Self::saved_preparation(&db, actor, project(r)?, preparation_request_id)?;
                return Ok(ResponseBody::HandyPlaybackPreparation(Box::new(
                    self.preparation_view(&db, actor, saved, r)?,
                )));
            }
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                return match (&r.command, body) {
                    (
                        Command::PrepareHandyPlayback { .. },
                        ResponseBody::HandyPlaybackPreparation(saved),
                    ) => {
                        replay_matches(r, &saved)?;
                        Ok(ResponseBody::HandyPlaybackPreparation(Box::new(
                            self.preparation_view(&db, actor, *saved, r)?,
                        )))
                    }
                    (
                        Command::RetireHandyPlaybackPreparation {
                            preparation_request_id,
                            preparation_id,
                            ..
                        },
                        ResponseBody::HandyPlaybackRetirement(saved),
                    ) => {
                        saved.validate()?;
                        if saved.request_id != r.request_id
                            || saved.preparation.request_id != *preparation_request_id
                            || saved.preparation.preparation_id != *preparation_id
                            || saved.preparation.correlation.inspection.project_id != *project(r)?
                        {
                            return Err(internal("retirement replay differs"));
                        }
                        self.retire_handy_preparations(Retirement::Id(preparation_id));
                        Ok(ResponseBody::HandyPlaybackRetirement(saved))
                    }
                    _ => Err(internal("preparation replay type differs")),
                };
            }
            if let Command::RetireHandyPlaybackPreparation {
                preparation_request_id,
                preparation_id,
                ..
            } = &r.command
            {
                let saved =
                    Self::saved_preparation(&db, actor, project(r)?, preparation_request_id)?;
                if saved.preparation_id != *preparation_id {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "preparation retirement pin differs",
                    ));
                }
                let receipt = HandyPlaybackRetirement {
                    request_id: r.request_id.clone(),
                    preparation: retired(saved),
                };
                receipt.validate()?;
                let body = ResponseBody::HandyPlaybackRetirement(Box::new(receipt));
                let tx = db.transaction().map_err(internal)?;
                remember(&tx, actor.as_str(), r, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                self.retire_handy_preparations(Retirement::Id(preparation_id));
                return Ok(body);
            }
        }
        let Command::PrepareHandyPlayback { program, .. } = &r.command else {
            return Err(ProtocolError::unsupported("Handy preparation"));
        };
        let query = self.preparation_inspection(r, program);
        let initial = {
            let db = self.db.lock().map_err(internal)?;
            self.capture_program_setup(&db, &query, actor)?.0
        };
        let ResponseBody::HandyProgramCorrelation(correlation) =
            self.inspect_handy_program_with(&query, actor, recheck, device)?
        else {
            return Err(internal("preparation correlation differs"));
        };
        let _memory = self.pool.reserve_memory(16 * 1024 * 1024)?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, actor)?;
        check_request_identity(&db, actor.as_str(), r, fingerprint)?;
        if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
            let ResponseBody::HandyPlaybackPreparation(saved) = body else {
                return Err(internal("preparation race receipt differs"));
            };
            replay_matches(r, &saved)?;
            return Ok(ResponseBody::HandyPlaybackPreparation(Box::new(
                self.preparation_view(&db, actor, *saved, r)?,
            )));
        }
        let (captured, _) = self.capture_program_setup(&db, &query, actor)?;
        if !Arc::ptr_eq(&initial.credentials, &captured.credentials)
            || !initial.hosted.same_location(&captured.hosted)
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "preparation references changed during inspection",
            ));
        }
        let timing = self.accepted_handy_timing(
            &db,
            &program.qualification,
            &captured.connection.registration,
            &correlation.inspection.device,
            &captured.hosted,
        )?;
        let admission = PlaybackAdmission {
            project: project(r)?.clone(),
            revision: correlation.inspection.revision,
            configuration: timing.configuration().clone(),
            program_artifact: captured.hosted.downloaded.artifact_id.clone(),
        };
        let physical_stop_pending = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .has_device_effects(&admission.configuration.device)
            .map_err(internal)?;
        let runtime = if physical_stop_pending {
            HandyPlaybackRuntime::recover_unconfirmed(
                admission.configuration.device.clone(),
                program.device_session.clone(),
                timing,
            )
        } else {
            HandyPlaybackRuntime::new(
                admission.configuration.device.clone(),
                program.device_session.clone(),
                timing,
            )
        }
        .map_err(|_| internal("native preparation runtime construction failed"))?;
        let transport = HandyTransport::from_snapshot(
            TransportBinding {
                admission: admission.clone(),
                session: program.device_session.clone(),
                stream_id: correlation.acknowledgment.stream_id,
            },
            captured.credentials.clone(),
        )
        .map_err(|_| internal("native preparation transport construction failed"))?;
        let id = ArtifactId::new(format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(actor, &self.engine_epoch, &r.request_id, &program.record_id))
                    .map_err(internal)?
            )
        ))
        .map_err(internal)?;
        let report = HandyPlaybackPreparation {
            request_id: r.request_id.clone(),
            preparation_id: id.clone(),
            correlation: *correlation,
            admission,
            state: HandyPlaybackPreparationState::Prepared,
            physical_stop_pending,
            armed: false,
            playback_enabled: false,
        };
        report.validate()?;
        self.retire_changed_handy_preparations(&db);
        let registry = self.handy_preparations.lock().map_err(internal)?;
        if registry.entries.len() >= HANDY_REGISTRATION_LIMIT
            || registry.entries.values().any(|e| {
                e.report.admission.configuration.device == report.admission.configuration.device
            })
        {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "device already has a native preparation or preparation capacity is exhausted",
            ));
        }
        drop(registry);
        let body = ResponseBody::HandyPlaybackPreparation(Box::new(report.clone()));
        let tx = db.transaction().map_err(internal)?;
        remember(&tx, actor.as_str(), r, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        self.handy_preparations
            .lock()
            .map_err(internal)?
            .entries
            .insert(
                id,
                Entry {
                    owner: actor.clone(),
                    report,
                    captured,
                    runtime: Arc::new(Mutex::new(runtime)),
                    transport: Arc::new(transport),
                    execution_pending: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    stop_activity: Arc::new(Mutex::new(None)),
                    controller: None,
                    server_clock: None,
                    playback_clock: None,
                    continuous_timing: None,
                    player_guard: None,
                    continuous_attempt: 0,
                    clock_attempt: 0,
                    pending_controller_stop: None,
                    play: None,
                    pending_play: None,
                    browser_owner: None,
                },
            );
        Ok(body)
    }
}

#[cfg(test)]
pub(super) fn controller_test_clock() -> PResult<u64> {
    controller::boottime_ns()
}
