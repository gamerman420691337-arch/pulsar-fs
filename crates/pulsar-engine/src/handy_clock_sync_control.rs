//! Explicit synchronization handoff. Durable custody outlives controller retirement.
use super::*;
use crate::device_effect_journal::{
    ClockSyncCompletion, ClockSyncEffectObservation, ClockSyncEffectRecord,
};
use crate::handy_online::DeviceTimingRead;
#[cfg(test)]
thread_local! {
    static TEST_SYNC_INSTANT: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}
fn sync_now() -> Instant {
    #[cfg(test)]
    if let Some(now) = TEST_SYNC_INSTANT.with(|clock| clock.get()) {
        return now;
    }
    Instant::now()
}
#[cfg(test)]
impl Engine {
    pub(in crate::authority) fn with_handy_sync_instant_for_test<T>(
        now: Instant,
        action: impl FnOnce() -> T,
    ) -> T {
        struct Restore(Option<Instant>);
        impl Drop for Restore {
            fn drop(&mut self) {
                TEST_SYNC_INSTANT.with(|clock| clock.set(self.0));
            }
        }
        let _restore = Restore(TEST_SYNC_INSTANT.with(|clock| clock.replace(Some(now))));
        action()
    }
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "clock synchronization selection changed",
    )
}
fn selected(r: &Request, l: &HandyControllerLease) -> bool {
    matches!(&r.command,Command::SynchronizeHandyClock{acquisition_request_id,controller_id,expected_generation,..}
        if l.acquisition_request_id==*acquisition_request_id && l.controller_id==*controller_id && l.generation==*expected_generation
        && l.state==HandyControllerState::Active && r.project.as_ref()==Some(&l.project_id) && r.expected_revision==Some(l.revision))
}
fn status(
    project: ProjectId,
    request: RequestId,
    value: Option<ClockSyncEffectObservation>,
) -> PResult<HandyClockSyncStatus> {
    let history = value.map(|v| HandyClockSyncHistory {
        record_id: ArtifactId::new(v.id).expect("validated journal digest"),
        original_controller: v.record.original_controller,
        preparation_qualification: v.record.preparation_qualification,
        setup_record_id: v.record.setup_record_id,
        user_offset_before: v.record.user_offset_before,
        state: v.state,
        acknowledgment: v.acknowledgment,
    });
    let value = HandyClockSyncStatus {
        project_id: project,
        sync_request_id: request,
        history,
        synchronization_verified: false,
        playback_enabled: false,
    };
    value.validate()?;
    Ok(value)
}
pub(in crate::authority) fn validate_sync_journal_links(
    db: &Connection,
    state_dir: &std::path::Path,
) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='requests')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let mut q=db.prepare("SELECT DISTINCT CASE WHEN length(CAST(session AS BLOB))<=128 THEN session ELSE NULL END,CASE WHEN length(CAST(response AS BLOB))<=16384 THEN response ELSE NULL END FROM requests WHERE CASE WHEN json_valid(response) THEN json_extract(response,'$.kind')='handy_clock_sync' AND json_type(response,'$.value.history') IS NOT 'null' ELSE response LIKE '%handy_clock_sync%' END LIMIT 2049")?;
    let mut witnesses = Vec::new();
    for r in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (actor, raw) = r?;
        let ResponseBody::HandyClockSync(v) = serde_json::from_str(&raw)? else {
            anyhow::bail!("invalid synchronization witness")
        };
        v.validate()?;
        let h = v
            .history
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing synchronization witness"))?;
        witnesses.push((
            SessionId::new(actor)?,
            v.sync_request_id.clone(),
            h.record_id.as_str().to_owned(),
        ));
    }
    anyhow::ensure!(
        witnesses.len() <= 2048,
        "synchronization witnesses exceed capacity"
    );
    crate::device_effect_journal::DeviceEffectJournal::verify_clock_sync_records(
        state_dir, &witnesses,
    )
}
impl Engine {
    fn sync_status(&self, r: &Request, actor: &SessionId, id: &RequestId) -> PResult<ResponseBody> {
        let value = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .clock_sync_request_history(actor, id)
            .map_err(internal)?;
        Ok(ResponseBody::HandyClockSync(Box::new(status(
            project(r)?.clone(),
            id.clone(),
            value,
        )?)))
    }
    fn reserve_sync(
        db: &mut Connection,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        value: &HandyClockSyncStatus,
    ) -> PResult<()> {
        if let Some(body) = replay(db, actor.as_str(), r, fingerprint)? {
            let ResponseBody::HandyClockSync(saved) = body else {
                return Err(mismatch());
            };
            saved.validate()?;
            if saved.project_id != value.project_id
                || saved.sync_request_id != value.sync_request_id
                || saved.history.as_ref().is_some_and(|h| {
                    value
                        .history
                        .as_ref()
                        .is_none_or(|v| v.record_id != h.record_id)
                })
            {
                return Err(mismatch());
            }
        } else {
            let tx = db.transaction().map_err(internal)?;
            remember(
                &tx,
                actor.as_str(),
                r,
                fingerprint,
                &ResponseBody::HandyClockSync(Box::new(value.clone())),
            )?;
            tx.commit().map_err(internal)?;
        }
        if value.history.is_some() {
            let tx = db.transaction().map_err(internal)?;
            let n = tx
                .execute(
                    "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
                    params![
                        encode(&ResponseBody::HandyClockSync(Box::new(value.clone())))?,
                        actor.as_str(),
                        r.request_id.as_str()
                    ],
                )
                .map_err(internal)?;
            if n != 1 {
                return Err(internal("synchronization witness disappeared"));
            }
            tx.commit().map_err(internal)?;
        }
        Ok(())
    }
    pub(in crate::authority) fn handy_clock_sync_control(
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
        if matches!(r.command, Command::SynchronizeHandyClock { .. })
            && !self.check_handy_controller_monitor()?
        {
            return Err(mismatch());
        }
        self.handy_clock_sync_with(
            r,
            actor,
            fingerprint,
            crate::handy_online::inspect_device_timing,
            crate::handy_clock_sync_wire::send,
            boottime_ns,
        )
    }
    pub(in crate::authority) fn handy_clock_sync_with<D, S, F>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        inspect: D,
        send: S,
        mut clock: F,
    ) -> PResult<ResponseBody>
    where
        D: FnOnce(
            Arc<Credentials>,
            Instant,
            &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<DeviceTimingRead>,
        S: FnOnce(
            Arc<Credentials>,
            Instant,
            &mut dyn FnMut() -> PResult<u64>,
        ) -> ClockSyncCompletion,
        F: FnMut() -> PResult<u64>,
    {
        r.validate()?;
        {
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            if let Command::HandyClockSyncStatus {
                sync_request_id, ..
            } = &r.command
            {
                return self.sync_status(r, actor, sync_request_id);
            }
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Command::CancelHandyClockSync {
                sync_request_id,
                record_id,
                ..
            } = &r.command
            {
                if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                    let ResponseBody::HandyClockSync(v) = &body else {
                        return Err(mismatch());
                    };
                    v.validate()?;
                    if v.sync_request_id != *sync_request_id
                        || v.project_id != *project(r)?
                        || !v.history.as_ref().is_some_and(|h| {
                            h.record_id == *record_id && h.state == HandyClockSyncState::Cancelled
                        })
                    {
                        return Err(mismatch());
                    }
                    return Ok(body);
                }
                let existing = self
                    ._device_effect_journal
                    .lock()
                    .map_err(internal)?
                    .clock_sync_request_history(actor, sync_request_id)
                    .map_err(internal)?
                    .ok_or_else(mismatch)?;
                if existing.id != record_id.as_str()
                    || existing.record.original_controller.project_id != *project(r)?
                {
                    return Err(mismatch());
                }
                let tx = db.transaction().map_err(internal)?;
                claim_request_identity(&tx, actor.as_str(), r, fingerprint)?;
                tx.commit().map_err(internal)?;
                self._device_effect_journal
                    .lock()
                    .map_err(internal)?
                    .cancel_clock_sync_unclaimed(record_id.as_str())
                    .map_err(|_| {
                        ProtocolError::new(
                            ErrorCode::Forbidden,
                            "only unclaimed synchronization can be cancelled",
                        )
                    })?;
                self.handy_preparations
                    .lock()
                    .map_err(internal)?
                    .retain_syncs(|s| s.context.id != *record_id);
                let body = self.sync_status(r, actor, sync_request_id)?;
                let tx = db.transaction().map_err(internal)?;
                remember(&tx, actor.as_str(), r, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                return Ok(body);
            }
            if let Some(existing) = self
                ._device_effect_journal
                .lock()
                .map_err(internal)?
                .clock_sync_request_history(actor, &r.request_id)
                .map_err(internal)?
            {
                if !selected(r, &existing.record.original_controller) {
                    return Err(mismatch());
                }
                let value = status(project(r)?.clone(), r.request_id.clone(), Some(existing))?;
                Self::reserve_sync(&mut db, r, actor, fingerprint, &value)?;
                return Ok(ResponseBody::HandyClockSync(Box::new(value)));
            }
            validate_sync_journal_links(&db, &self.config.state_dir).map_err(internal)?;
        }
        let Command::SynchronizeHandyClock {
            acquisition_request_id,
            controller_id,
            expected_generation,
            owner_proof,
            ..
        } = &r.command
        else {
            return Err(ProtocolError::unsupported("clock synchronization"));
        };
        let mut query = r.clone();
        query.command = Command::InspectHandyDeviceTiming {
            acquisition_request_id: acquisition_request_id.clone(),
            controller_id: controller_id.clone(),
            expected_generation: *expected_generation,
            allow_online_inspection: true,
            owner_proof: owner_proof.clone(),
        };
        let ResponseBody::HandyDeviceTiming(preflight) =
            self.handy_device_timing_with(&query, actor, inspect, &mut clock)?
        else {
            return Err(internal("synchronization preflight differs"));
        };
        if preflight.first.user_offset != preflight.second.user_offset {
            return Err(mismatch());
        }
        let (captured, prep, claim, deadline, native_context) = {
            let mut db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Some(v) = self
                ._device_effect_journal
                .lock()
                .map_err(internal)?
                .clock_sync_request_history(actor, &r.request_id)
                .map_err(internal)?
            {
                let value = status(project(r)?.clone(), r.request_id.clone(), Some(v))?;
                Self::reserve_sync(&mut db, r, actor, fingerprint, &value)?;
                return Ok(ResponseBody::HandyClockSync(Box::new(value)));
            }
            let live =
                self.controller_view(&db, actor, preflight.controller.clone(), r, &mut clock)?;
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
            let qualified = self.accepted_handy_device_clock(
                &db,
                &preflight.preparation_qualification,
                &captured.connection.registration,
                &preflight.after,
                &captured.hosted,
            )?;
            if qualified.as_ref().is_some_and(|q| {
                q.contract.user_offset_ms != preflight.second.user_offset.offset_ms
            }) {
                return Err(mismatch());
            }
            let route = crate::HandyRouteIdentity::from_snapshot(
                &self.handy_owner_token,
                &captured.credentials.connection,
            )?;
            let record = ClockSyncEffectRecord {
                actor_session: actor.clone(),
                request_id: r.request_id.clone(),
                original_controller: live.clone(),
                registration: captured.connection.registration.clone(),
                preparation_qualification: preflight.preparation_qualification.clone(),
                setup_record_id: prep.correlation.record_id.clone(),
                user_offset_before: preflight.second.user_offset.clone(),
                route_sha256: route.route_sha256().into(),
                route_namespace_sha256: route.namespace_sha256().into(),
            };
            Self::reserve_sync(
                &mut db,
                r,
                actor,
                fingerprint,
                &status(project(r)?.clone(), r.request_id.clone(), None)?,
            )?;
            let mut journal = self._device_effect_journal.lock().map_err(internal)?;
            let prepared = journal
                .prepare_clock_sync(record.clone())
                .map_err(internal)?;
            let existing = journal
                .clock_sync_request_history(actor, &r.request_id)
                .map_err(internal)?
                .ok_or_else(mismatch)?;
            Self::reserve_sync(
                &mut db,
                r,
                actor,
                fingerprint,
                &status(project(r)?.clone(), r.request_id.clone(), Some(existing))?,
            )?;
            let native_context = {
                let mut registry = self.handy_preparations.lock().map_err(internal)?;
                let now = registry.sweep_controllers(&mut clock)?;
                let context = registry.begin_sync_context(
                    ArtifactId::new(prepared.id()).map_err(internal)?,
                    record,
                    prep.clone(),
                    captured.clone(),
                    qualified,
                    now,
                )?;
                let retired = registry
                    .retire_controller(&live.preparation_id, &live.controller_id)
                    .ok_or_else(mismatch)?;
                if retired.physical_stop_pending {
                    return Err(mismatch());
                }
                context
            };
            let now = clock()?;
            let remaining = live
                .deadline_boottime_ns
                .checked_sub(now)
                .filter(|v| *v > 0)
                .ok_or_else(mismatch)?;
            let deadline = sync_now() + Duration::from_nanos(remaining.min(10_000_000_000));
            let claim = journal.claim_clock_sync(&prepared).map_err(internal)?;
            (captured, prep, claim, deadline, native_context)
        };
        let current = (|| {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_revision(&db, r)?;
            let q = self.preparation_inspection(r, &prep.selection()?);
            let fresh = self.capture_setup_during_sync(&db, &q, actor, Some(&claim))?;
            if !Arc::ptr_eq(&captured.credentials, &fresh.credentials)
                || captured.connection.registration != fresh.connection.registration
                || !captured.hosted.same_location(&fresh.hosted)
            {
                return Err(mismatch());
            }
            self.accepted_handy_timing(
                &db,
                &preflight.preparation_qualification,
                &fresh.connection.registration,
                &preflight.after,
                &fresh.hosted,
            )?;
            if clock()? >= preflight.controller.deadline_boottime_ns {
                return Err(mismatch());
            }
            Ok(())
        })();
        let outcome = if current.is_ok() && sync_now() < deadline {
            send(captured.credentials.clone(), deadline, &mut || {
                let now = self
                    .handy_preparations
                    .lock()
                    .map_err(internal)?
                    .sweep_controllers(&mut clock)?;
                if now >= preflight.controller.deadline_boottime_ns {
                    return Err(mismatch());
                }
                Ok(now)
            })
        } else {
            #[cfg(test)]
            eprintln!(
                "clock sync withheld before sender: current_error={:?}, deadline_elapsed={}",
                current.as_ref().err().map(|error| &error.code),
                sync_now() >= deadline,
            );
            ClockSyncCompletion::Unknown
        };
        let acknowledgment = match &outcome {
            ClockSyncCompletion::Acknowledged(a) => Some(a.clone()),
            ClockSyncCompletion::Unknown => None,
        };
        self._device_effect_journal
            .lock()
            .map_err(internal)?
            .complete_clock_sync(&claim, outcome)
            .map_err(|_| {
                ProtocolError::new(
                    ErrorCode::UnknownOutcome,
                    "synchronization outcome could not be recorded; do not repeat the effect",
                )
            })?;
        let mut query = r.clone();
        query.expected_revision = None;
        query.command = Command::HandyClockSyncStatus {
            sync_request_id: r.request_id.clone(),
            owner_proof: owner_proof.clone(),
        };
        let db = self.db.lock().map_err(internal)?;
        if let Some(context) = native_context {
            let native = (|| {
                let ack = acknowledgment.ok_or_else(mismatch)?;
                let query = self.preparation_inspection(r, &context.preparation.selection()?);
                let fresh = self.capture_setup(&db, &query, actor)?;
                if !Arc::ptr_eq(&fresh.credentials, &context.captured.credentials)
                    || !fresh.hosted.same_location(&context.captured.hosted)
                {
                    return Err(mismatch());
                }
                let qualified = self
                    .accepted_handy_device_clock(
                        &db,
                        &context.record.preparation_qualification,
                        &fresh.connection.registration,
                        &preflight.after,
                        &fresh.hosted,
                    )?
                    .ok_or_else(mismatch)?;
                if qualified != context.qualified {
                    return Err(mismatch());
                }
                crate::handy_playback_clock::NativeDeviceClock::new(ack, qualified)
            })()
            .ok();
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            if let Ok(now) = registry.sweep_controllers(&mut clock) {
                registry.finish_sync_context(&context, native, now);
            }
        }
        self.authorize(&db, &query, actor)?;
        self.sync_status(&query, actor, &r.request_id)
    }
}
