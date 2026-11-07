//! Independent qualified observations for an already acknowledged Playing stream.
//! Original play/synchronization records and their timestamps are never changed.
use super::*;
use crate::{
    device_effect_journal::PlayOperationState,
    handy_continuous_timing::{
        ContinuousTimingSamples, NativeContinuousTiming, QualifiedContinuousTiming,
    },
    handy_online::PlayingTimingRead,
    handy_server_clock::{self, Sample},
};

pub(in crate::authority::handy_playback_preparation) struct RetainedContinuousTiming {
    pub(in crate::authority::handy_playback_preparation) report: HandyContinuousTimingObservation,
    pub(in crate::authority::handy_playback_preparation) native: NativeContinuousTiming,
    pub(in crate::authority::handy_playback_preparation) pins: Arc<play::PlayPins>,
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    attempt: u64,
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "continuous timing play, controller or native lineage changed",
    )
}
fn same_controller(a: &HandyControllerLease, b: &HandyControllerLease) -> bool {
    a.acquisition_request_id == b.acquisition_request_id
        && a.controller_id == b.controller_id
        && a.preparation_request_id == b.preparation_request_id
        && a.preparation_id == b.preparation_id
        && a.project_id == b.project_id
        && a.revision == b.revision
        && a.engine_epoch == b.engine_epoch
        && a.device_session == b.device_session
        && a.device_id == b.device_id
        && a.duration_ms == b.duration_ms
}
fn runtime_matches(entry: &Entry, pins: &play::PlayPins) -> bool {
    let Some(lease) = &entry.controller else {
        return false;
    };
    let Ok(runtime) = entry.runtime.lock() else {
        return false;
    };
    lease.report.state == HandyControllerState::Playing
        && runtime.session().state() == DeviceState::Playing
        && runtime.session().controller() == Some(&lease.report.controller_id)
        && runtime.session().session() == &lease.report.device_session
        && runtime.session().admission() == Some(&pins.record.effect.admission)
        && entry.report.admission == pins.record.effect.admission
        && entry.pending_controller_stop.is_none()
}
impl RetainedContinuousTiming {
    pub(in crate::authority::handy_playback_preparation) fn current(
        &self,
        entry: &Entry,
        now: u64,
        syncs: &HashMap<ArtifactId, native_sync::Slot>,
    ) -> bool {
        entry.controller.as_ref().is_some_and(|lease| {
            same_controller(&lease.report, &self.report.controller)
                && lease.report.generation >= self.report.controller.generation
                && now < lease.report.deadline_boottime_ns
        }) && entry
            .play
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(p, &self.pins))
            && Arc::ptr_eq(&entry.runtime, &self.runtime)
            && self.attempt == self.report.sequence
            && self.attempt <= entry.continuous_attempt
            && self.report.play_record_id.as_str() == self.pins.id
            && self.report.play_request_id == self.pins.record.request_id
            && self.report.qualification == self.pins.record.qualification
            && syncs
                .get(&self.pins.sync.id)
                .is_some_and(|s| s.native.is_some() && Arc::ptr_eq(&s.context, &self.pins.sync))
            && runtime_matches(entry, &self.pins)
            && self.native.at(now).is_ok()
    }
}
impl Entry {
    pub(in crate::authority::handy_playback_preparation) fn playing_clock_current(
        &self,
        now: u64,
        syncs: &HashMap<ArtifactId, native_sync::Slot>,
    ) -> bool {
        let device_current = match &self.continuous_timing {
            Some(timing) => timing.current(self, now, syncs),
            None => self
                .play
                .as_ref()
                .is_some_and(|p| p.clock_current(now, syncs)),
        };
        device_current
            && self.play.as_ref().is_some_and(|pins| {
                pins.browser.as_ref().is_none_or(|browser| {
                    self.controller.as_ref().is_some_and(|lease| {
                        browser.owner.current(self, now)
                            && browser
                                .alignment(
                                    pins,
                                    self.continuous_timing.as_ref().map(|t| &t.native),
                                    now,
                                    lease.report.deadline_boottime_ns,
                                )
                                .is_ok()
                    })
                })
            })
    }
}
struct Captured {
    lease: HandyControllerLease,
    pins: Arc<play::PlayPins>,
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    qualified: QualifiedContinuousTiming,
    attempt: u64,
    prior: Option<NativeContinuousTiming>,
    begin: u64,
}
fn selected(
    r: &Request,
    selection: &HandyContinuousTimingSelection,
    lease: &HandyControllerLease,
    pins: &play::PlayPins,
) -> bool {
    selection.acquisition_request_id == lease.acquisition_request_id
        && selection.controller_id == lease.controller_id
        && (selection.expected_generation == lease.generation
            || pins.browser.as_ref().is_some_and(|browser| {
                selection.expected_generation <= lease.generation
                    && browser.owner.selected_generation() == lease.generation
            }))
        && selection.play_request_id == pins.record.request_id
        && selection.play_record_id.as_str() == pins.id
        && lease.state == HandyControllerState::Playing
        && r.project.as_ref() == Some(&lease.project_id)
        && r.expected_revision == Some(lease.revision)
}
fn same_qualification(a: &QualifiedContinuousTiming, b: &QualifiedContinuousTiming) -> bool {
    a.contract == b.contract
        && a.maximum_total_error_ns == b.maximum_total_error_ns
        && a.server.contract == b.server.contract
        && a.server.max_age_ns == b.server.max_age_ns
        && a.server.max_uncertainty_ns == b.server.max_uncertainty_ns
        && a.server.drift_ppm == b.server.drift_ppm
}
fn exact_entry(
    registry: &Registry,
    r: &Request,
    selection: &HandyContinuousTimingSelection,
    captured: &Captured,
) -> bool {
    playback_clock::sync_current(registry, &captured.pins.sync)
        && registry
            .entries
            .get(&captured.lease.preparation_id)
            .is_some_and(|entry| {
                entry.continuous_attempt == captured.attempt
                    && Arc::ptr_eq(&entry.runtime, &captured.runtime)
                    && entry
                        .play
                        .as_ref()
                        .is_some_and(|p| Arc::ptr_eq(p, &captured.pins))
                    && entry.controller.as_ref().is_some_and(|lease| {
                        same_controller(&lease.report, &captured.lease)
                            && (selected(r, selection, &lease.report, &captured.pins)
                                || captured.pins.browser.as_ref().is_some_and(|browser| {
                                    let mut original = lease.report.clone();
                                    original.generation = captured.lease.generation;
                                    lease.report.generation >= captured.lease.generation
                                        && browser.owner.current(
                                            entry,
                                            registry.controller_tick.unwrap_or(u64::MAX),
                                        )
                                        && selected(r, selection, &original, &captured.pins)
                                }))
                    })
                    && runtime_matches(entry, &captured.pins)
            })
}
fn check_device(captured: &Captured, read: &PlayingTimingRead) -> PResult<()> {
    let device = &read.device;
    validate_handy_playing_context(&device.before, &device.after)?;
    let expected = &captured.pins.preparation.correlation.inspection.device;
    let original = expected.remote_hssp.as_ref().ok_or_else(mismatch)?;
    for value in [&device.before, &device.after] {
        let hssp = value.remote_hssp.as_ref().ok_or_else(mismatch)?;
        if value.identity != expected.identity
            || value.mode != expected.mode
            || value.operative_settings != expected.operative_settings
            || hssp.stream_id
                != captured
                    .pins
                    .preparation
                    .correlation
                    .acknowledgment
                    .stream_id
            || hssp.pause_on_starving != original.pause_on_starving
        {
            return Err(mismatch());
        }
    }
    let original_clock = &captured.pins.device_clock;
    for value in [&device.first, &device.second] {
        value.validate()?;
        if value.read_started_boottime_ns < original_clock.acknowledgment.received_boottime_ns
            || value.info.device_uptime_ms < original_clock.acknowledgment.info.device_uptime_ms
            || value.info.reported_clock_offset_ms
                != original_clock.acknowledgment.info.reported_clock_offset_ms
            || value.info.last_sync_round_trip_ms
                != original_clock.acknowledgment.info.last_sync_round_trip_ms
            || value.user_offset.offset_ms != original_clock.qualified.contract.user_offset_ms
        {
            return Err(mismatch());
        }
    }
    if let Some(previous) = &captured.prior {
        if device.first.info.device_uptime_ms
            < previous
                .samples()
                .device
                .device
                .second
                .info
                .device_uptime_ms
        {
            return Err(mismatch());
        }
    }
    Ok(())
}
fn server_report(sample: &Sample) -> HandyContinuousServerSample {
    HandyContinuousServerSample {
        sent_boottime_ns: sample.sent_ns,
        received_boottime_ns: sample.received_ns,
        server_time_ms: sample.server_ms,
    }
}
impl Engine {
    pub(in crate::authority::handy_playback_preparation) fn continuous_acknowledged(
        &self,
        db: &Connection,
        actor: &SessionId,
        pins: &play::PlayPins,
    ) -> PResult<()> {
        if pins.record.actor_session != *actor {
            return Err(mismatch());
        }
        let witness = play::owned_references(db, &self.pool, pins)?;
        let history = self
            ._device_effect_journal
            .lock()
            .map_err(internal)?
            .play_request_history(actor, &pins.record.request_id)
            .map_err(internal)?
            .ok_or_else(mismatch)?;
        history.check_witness(&witness).map_err(internal)?;
        if history.id != pins.id || history.state != PlayOperationState::Acknowledged {
            return Err(mismatch());
        }
        Ok(())
    }
    fn continuous_references(
        &self,
        db: &Connection,
        r: &Request,
        actor: &SessionId,
        pins: &play::PlayPins,
        observation: &HandySetupDeviceObservation,
    ) -> PResult<QualifiedContinuousTiming> {
        self.authorize(db, r, actor)?;
        check_revision(db, r)?;
        self.continuous_acknowledged(db, actor, pins)?;
        let (current, _) = self.preparation_references(db, actor, &pins.preparation, r)?;
        let route = crate::HandyRouteIdentity::from_snapshot(
            &self.handy_owner_token,
            &current.credentials.connection,
        )?;
        if encode(&current.connection)? != encode(&pins.captured.connection)?
            || !Arc::ptr_eq(&current.credentials, &pins.captured.credentials)
            || !current.hosted.same_location(&pins.captured.hosted)
            || route.route_sha256() != pins.route
            || route.namespace_sha256() != pins.namespace
        {
            return Err(mismatch());
        }
        self.accepted_handy_continuous_timing(
            db,
            &pins.record.qualification,
            &current.connection.registration,
            observation,
            &current.hosted,
        )
    }
    fn continuous_view<F: FnMut() -> PResult<u64>>(
        &self,
        db: &Connection,
        r: &Request,
        actor: &SessionId,
        mut report: HandyContinuousTimingObservation,
        clock: &mut F,
    ) -> PResult<ResponseBody> {
        report.validate()?;
        if report.controller.project_id != *project(r)? {
            return Err(mismatch());
        }
        let live = self.controller_view(db, actor, report.controller.clone(), r, clock)?;
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        report.current = registry.sweep_controllers(clock).is_ok_and(|now| {
            same_controller(&live, &report.controller)
                && registry
                    .entries
                    .get(&live.preparation_id)
                    .is_some_and(|entry| {
                        entry.continuous_timing.as_ref().is_some_and(|timing| {
                            timing.report.request_id == report.request_id
                                && timing.report.sequence == report.sequence
                                && timing.current(entry, now, &registry.syncs)
                        })
                    })
        });
        Ok(ResponseBody::HandyContinuousTiming(Box::new(report)))
    }
    pub(in crate::authority) fn handy_continuous_timing_control(
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
        if matches!(r.command, Command::RefreshHandyPlaybackTiming { .. })
            && !self.check_handy_controller_monitor()?
        {
            return Err(mismatch());
        }
        self.handy_continuous_timing_with(
            r,
            actor,
            fingerprint,
            (
                |hosted, deadline| hosted.recheck(deadline).map_err(|_| mismatch()),
                crate::handy_online::inspect_playing_timing,
                handy_server_clock::sample_until,
            ),
            boottime_ns,
        )
    }
    pub(in crate::authority) fn handy_continuous_timing_with<R, D, S, F>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        io: (R, D, S),
        mut clock: F,
    ) -> PResult<ResponseBody>
    where
        R: FnOnce(&VerifiedHosted, Instant) -> PResult<ArtifactIdentity>,
        D: FnOnce(
            Arc<Credentials>,
            Instant,
            &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<PlayingTimingRead>,
        S: FnMut(Instant, &mut dyn FnMut() -> PResult<u64>) -> PResult<Sample>,
        F: FnMut() -> PResult<u64>,
    {
        r.validate()?;
        let (recheck, inspect, mut sample) = io;
        let deadline = Instant::now() + Duration::from_secs(30);
        let captured = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, r, actor)?;
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Command::HandyContinuousTimingStatus {
                refresh_request_id, ..
            } = &r.command
            {
                let raw: Option<String> = db.query_row(
                    "SELECT CASE WHEN length(CAST(response AS BLOB))<=131072 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",
                    params![actor.as_str(), refresh_request_id.as_str()], |row| row.get(0),
                ).optional().map_err(internal)?;
                let raw = raw.ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::NotFound,
                        "continuous timing observation not found",
                    )
                })?;
                let ResponseBody::HandyContinuousTiming(report) = decode(&raw)? else {
                    return Err(mismatch());
                };
                if report.request_id != *refresh_request_id {
                    return Err(mismatch());
                }
                return self.continuous_view(&db, r, actor, *report, &mut clock);
            }
            let Command::RefreshHandyPlaybackTiming { selection, .. } = &r.command else {
                return Err(ProtocolError::unsupported("continuous playback timing"));
            };
            if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::HandyContinuousTiming(report) = body else {
                    return Err(mismatch());
                };
                if report.request_id != r.request_id || report.selection() != *selection {
                    return Err(mismatch());
                }
                return self.continuous_view(&db, r, actor, *report, &mut clock);
            }
            check_revision(&db, r)?;
            let saved = load(&db, actor, project(r)?, &selection.acquisition_request_id)?;
            let lease = self.controller_view(&db, actor, saved, r, &mut clock)?;
            let (pins, runtime) = {
                let mut registry = self.handy_preparations.lock().map_err(internal)?;
                registry.sweep_controllers(&mut clock)?;
                let entry = registry
                    .entries
                    .get(&lease.preparation_id)
                    .ok_or_else(mismatch)?;
                let pins = entry.play.clone().ok_or_else(mismatch)?;
                if !selected(r, selection, &lease, &pins) || !runtime_matches(entry, &pins) {
                    return Err(mismatch());
                }
                (pins, entry.runtime.clone())
            };
            let qualified = self.continuous_references(
                &db,
                r,
                actor,
                &pins,
                &pins.preparation.correlation.inspection.device,
            )?;
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let begin = registry.sweep_controllers(&mut clock)?;
            if !playback_clock::sync_current(&registry, &pins.sync) {
                return Err(mismatch());
            }
            let entry = registry
                .entries
                .get_mut(&lease.preparation_id)
                .ok_or_else(mismatch)?;
            if !entry
                .controller
                .as_ref()
                .is_some_and(|l| selected(r, selection, &l.report, &pins))
                || !entry.play.as_ref().is_some_and(|p| Arc::ptr_eq(p, &pins))
                || !Arc::ptr_eq(&entry.runtime, &runtime)
            {
                return Err(mismatch());
            }
            entry.continuous_attempt = entry
                .continuous_attempt
                .checked_add(1)
                .ok_or_else(mismatch)?;
            Captured {
                lease,
                pins,
                runtime,
                qualified,
                attempt: entry.continuous_attempt,
                prior: entry.continuous_timing.as_ref().map(|v| v.native.clone()),
                begin,
            }
        };
        let Command::RefreshHandyPlaybackTiming { selection, .. } = &r.command else {
            return Err(mismatch());
        };
        let result = (|| {
            let mut sampled_clock = || {
                if Instant::now() >= deadline {
                    return Err(mismatch());
                }
                let mut registry = self.handy_preparations.lock().map_err(internal)?;
                let now = registry.sweep_controllers(&mut clock)?;
                if !exact_entry(&registry, r, selection, &captured) {
                    return Err(mismatch());
                }
                Ok(now)
            };
            let server_before = sample(deadline, &mut sampled_clock)?;
            let device = inspect(
                captured.pins.captured.credentials.clone(),
                deadline,
                &mut sampled_clock,
            )?;
            let server_after = sample(deadline, &mut sampled_clock)?;
            check_device(&captured, &device)?;
            if server_before.sent_ns < captured.begin {
                return Err(mismatch());
            }
            let downloaded = recheck(&captured.pins.captured.hosted, deadline)?;
            if downloaded != captured.pins.captured.hosted.downloaded || Instant::now() >= deadline
            {
                return Err(mismatch());
            }
            let domain = ArtifactId::new(format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&(
                        "handy-continuous-timing-v1",
                        &self.engine_epoch,
                        &captured.lease.controller_id,
                        captured.lease.generation,
                        captured.attempt,
                        &r.request_id,
                        &captured.pins.id,
                    ))
                    .map_err(internal)?
                )
            ))
            .map_err(internal)?;
            let native = NativeContinuousTiming::new(
                domain,
                captured.pins.continuous_reference(),
                ContinuousTimingSamples {
                    device,
                    server_before,
                    server_after,
                },
                captured.qualified.clone(),
            )?;
            if let Some(previous) = &captured.prior {
                native.agrees_with(previous)?;
            }
            let mut db = self.db.lock().map_err(internal)?;
            let qualified = self.continuous_references(
                &db,
                r,
                actor,
                &captured.pins,
                &native.samples().device.device.after,
            )?;
            if !same_qualification(&qualified, &captured.qualified) {
                return Err(mismatch());
            }
            check_request_identity(&db, actor.as_str(), r, fingerprint)?;
            if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
                let ResponseBody::HandyContinuousTiming(report) = body else {
                    return Err(mismatch());
                };
                if report.request_id != r.request_id || report.selection() != *selection {
                    return Err(mismatch());
                }
                return self.continuous_view(&db, r, actor, *report, &mut clock);
            }
            {
                let mut registry = self.handy_preparations.lock().map_err(internal)?;
                let now = registry.sweep_controllers(&mut clock)?;
                if !exact_entry(&registry, r, selection, &captured) {
                    return Err(mismatch());
                }
                native.at(now)?;
                if let Some(browser) = &captured.pins.browser {
                    let entry = registry
                        .entries
                        .get(&captured.lease.preparation_id)
                        .ok_or_else(mismatch)?;
                    browser.alignment(
                        &captured.pins,
                        Some(&native),
                        now,
                        entry
                            .controller
                            .as_ref()
                            .ok_or_else(mismatch)?
                            .report
                            .deadline_boottime_ns,
                    )?;
                }
            }
            let at = native.observed_at();
            let estimate = native.at(at)?;
            let samples = native.samples();
            let mut report = HandyContinuousTimingObservation {
                request_id: r.request_id.clone(),
                controller: captured.lease.clone(),
                play_request_id: captured.pins.record.request_id.clone(),
                play_record_id: ArtifactId::new(captured.pins.id.clone()).map_err(internal)?,
                qualification: captured.pins.record.qualification.clone(),
                synchronization: HandyClockSyncBinding {
                    sync_request_id: captured.pins.sync.record.request_id.clone(),
                    record_id: captured.pins.sync.id.clone(),
                },
                sequence: captured.attempt,
                contract: captured.qualified.contract.clone(),
                reference: captured.pins.continuous_reference(),
                before: samples.device.device.before.clone(),
                first: samples.device.device.first.clone(),
                second: samples.device.device.second.clone(),
                after: samples.device.device.after.clone(),
                inspection_started_boottime_ns: samples.device.started_boottime_ns,
                inspection_finished_boottime_ns: samples.device.finished_boottime_ns,
                server_before: server_report(&samples.server_before),
                server_after: server_report(&samples.server_after),
                observed_at_boottime_ns: at,
                valid_until_boottime_ns: estimate.valid_until_boottime_ns,
                observed_phase_ms: estimate.observed_phase_ms,
                phase_uncertainty_ns: estimate.phase_uncertainty_ns,
                estimated_server_ms: estimate.estimated_server_ms,
                server_uncertainty_ns: estimate.server_uncertainty_ns,
                alignment_error_ns: estimate.alignment_error_ns,
                current: true,
                playback_enabled: false,
            };
            report.validate()?;
            if captured
                .pins
                .captured
                .credentials
                .echoed_text(&encode(&report)?)
            {
                return Err(mismatch());
            }
            let tx = db.transaction().map_err(internal)?;
            remember(
                &tx,
                actor.as_str(),
                r,
                fingerprint,
                &ResponseBody::HandyContinuousTiming(Box::new(report.clone())),
            )?;
            tx.commit().map_err(internal)?;
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            report.current = registry.sweep_controllers(&mut clock).is_ok_and(|now| {
                exact_entry(&registry, r, selection, &captured)
                    && native.at(now).is_ok()
                    && captured.pins.browser.as_ref().is_none_or(|browser| {
                        registry
                            .entries
                            .get(&captured.lease.preparation_id)
                            .and_then(|entry| entry.controller.as_ref())
                            .is_some_and(|lease| {
                                browser
                                    .alignment(
                                        &captured.pins,
                                        Some(&native),
                                        now,
                                        lease.report.deadline_boottime_ns,
                                    )
                                    .is_ok()
                            })
                    })
            });
            if report.current {
                registry
                    .entries
                    .get_mut(&captured.lease.preparation_id)
                    .expect("checked continuous target")
                    .continuous_timing = Some(RetainedContinuousTiming {
                    report: report.clone(),
                    native,
                    pins: captured.pins.clone(),
                    runtime: captured.runtime.clone(),
                    attempt: captured.attempt,
                });
            }
            Ok(ResponseBody::HandyContinuousTiming(Box::new(report)))
        })();
        if result.is_err() {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            if exact_entry(&registry, r, selection, &captured) {
                registry.retire_controller(
                    &captured.lease.preparation_id,
                    &captured.lease.controller_id,
                );
            }
        }
        result
    }
}

#[cfg(test)]
impl Engine {
    pub(in crate::authority) fn continuous_lineage_for_test(
        &self,
        preparation: &ArtifactId,
        now: u64,
    ) -> Option<(HandyClockSyncAcknowledgment, bool)> {
        let registry = self.handy_preparations.lock().unwrap();
        let pins = registry.entries.get(preparation)?.play.as_ref()?;
        Some((
            pins.device_clock.acknowledgment.clone(),
            pins.device_clock.error_at(now).is_ok(),
        ))
    }
    pub(in crate::authority) fn remove_continuous_lineage_for_test(
        &self,
        preparation: &ArtifactId,
    ) {
        let mut registry = self.handy_preparations.lock().unwrap();
        let context = registry
            .entries
            .get(preparation)
            .unwrap()
            .play
            .as_ref()
            .unwrap()
            .sync
            .clone();
        registry.retain_syncs(|s| !Arc::ptr_eq(&s.context, &context));
    }
}
