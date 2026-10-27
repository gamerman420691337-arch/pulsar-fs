//! Boot-local stop-on-player-loss custody. Browser samples never authorize play
//! or establish source correspondence, calibrated time or physical stopping.
use super::*;

pub(in crate::authority::handy_playback_preparation) struct RetainedPlayerGuard {
    report: HandyPlayerGuardReport,
    principal: SessionId,
    pub(in crate::authority::handy_playback_preparation) auth_hash: String,
    pins: Arc<play::PlayPins>,
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    pending: Option<HandyPlayerChallenge>,
    browser_media: Option<crate::authority::browser_media::SourceMediaPin>,
}
fn mismatch() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::DependencyMismatch,
        "player guard binding is not current",
    )
}
fn same_secret(expected: &str, actual: &str) -> bool {
    let difference = expected
        .bytes()
        .zip(actual.bytes())
        .fold(0u8, |n, (a, b)| n | (a ^ b));
    expected.len() == actual.len() && difference == 0
}
fn token_hash(r: &Request) -> PResult<String> {
    let token = r.auth_token.as_deref().ok_or_else(|| {
        ProtocolError::new(
            ErrorCode::Unauthorized,
            "session authentication token required",
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(token.as_bytes())))
}
fn opaque_id() -> ArtifactId {
    ArtifactId::new(format!("{:x}", Sha256::digest(Uuid::new_v4().as_bytes())))
        .expect("SHA256 is an artifact ID")
}
fn matches_play(
    entry: &Entry,
    pins: &Arc<play::PlayPins>,
    runtime: &Arc<Mutex<HandyPlaybackRuntime>>,
) -> bool {
    let Some(lease) = &entry.controller else {
        return false;
    };
    if !Arc::ptr_eq(&entry.runtime, runtime)
        || !entry.play.as_ref().is_some_and(|p| Arc::ptr_eq(p, pins))
        || entry.pending_controller_stop.is_some()
        || lease.report.state != HandyControllerState::Playing
    {
        return false;
    }
    let Ok(runtime) = runtime.lock() else {
        return false;
    };
    runtime.session().state() == DeviceState::Playing
        && runtime.session().controller() == Some(&lease.report.controller_id)
        && runtime.session().session() == &lease.report.device_session
        && runtime.session().admission() == Some(&pins.record.effect.admission)
        && entry.report.admission == pins.record.effect.admission
}
impl RetainedPlayerGuard {
    pub(in crate::authority::handy_playback_preparation) fn current(
        &self,
        entry: &Entry,
        now: u64,
    ) -> bool {
        let original = &self.report.binding.controller;
        now >= self.report.bound_at_boottime_ns
            && now < self.report.heartbeat_deadline_boottime_ns
            && now < self.report.media_progress_deadline_boottime_ns
            && self.report.binding.browser_media.is_some() == self.browser_media.is_some()
            && self.browser_media.as_ref().is_none_or(|pin| pin.current())
            && entry.owner == self.principal
            && entry.controller.as_ref().is_some_and(|l| {
                l.report.controller_id == original.controller_id
                    && l.report.acquisition_request_id == original.acquisition_request_id
                    && l.report.preparation_id == original.preparation_id
                    && l.report.engine_epoch == original.engine_epoch
                    && l.report.project_id == original.project_id
                    && l.report.revision == original.revision
                    && l.report.device_session == original.device_session
                    && l.report.generation >= original.generation
                    && now < l.report.deadline_boottime_ns
            })
            && matches_play(entry, &self.pins, &self.runtime)
    }
}
fn selected(
    r: &Request,
    selection: &HandyContinuousTimingSelection,
    lease: &HandyControllerLease,
    pins: &play::PlayPins,
) -> bool {
    selection.acquisition_request_id == lease.acquisition_request_id
        && selection.controller_id == lease.controller_id
        && selection.expected_generation == lease.generation
        && selection.play_request_id == pins.record.request_id
        && selection.play_record_id.as_str() == pins.id
        && lease.state == HandyControllerState::Playing
        && r.project.as_ref() == Some(&lease.project_id)
        && r.expected_revision == Some(lease.revision)
}
fn report_body(report: HandyPlayerGuardReport) -> PResult<ResponseBody> {
    report.validate()?;
    Ok(ResponseBody::HandyPlayerGuard(Box::new(report)))
}
#[cfg(test)]
thread_local! {static TEST_CLOCK: std::cell::Cell<Option<u64>> = const {std::cell::Cell::new(None)};}
fn fast_clock() -> PResult<u64> {
    #[cfg(test)]
    if let Some(now) = TEST_CLOCK.with(|clock| clock.get()) {
        return Ok(now);
    }
    boottime_ns()
}
impl Engine {
    #[cfg(test)]
    pub(in crate::authority) fn handy_player_guard_count_for_test(&self) -> usize {
        self.handy_preparations
            .lock()
            .unwrap()
            .entries
            .values()
            .filter(|e| e.player_guard.is_some())
            .count()
    }
    #[cfg(test)]
    pub(in crate::authority) fn remove_handy_player_sync_for_test(&self, preparation: &ArtifactId) {
        let mut registry = self.handy_preparations.lock().unwrap();
        let id = registry.entries[preparation]
            .play
            .as_ref()
            .unwrap()
            .sync
            .id
            .clone();
        registry.syncs.remove(&id);
    }
    #[cfg(test)]
    pub(in crate::authority) fn start_handy_player_monitor_for_test(
        &self,
        clock: Arc<dyn Fn() -> PResult<u64> + Send + Sync>,
    ) -> PResult<()> {
        let mut slot = self.handy_controller_monitor.lock().map_err(internal)?;
        if slot.is_some() {
            return Err(mismatch());
        }
        *slot = Some(ControllerMonitor::start_with_clock(
            self.handy_preparations.clone(),
            move |registry| {
                // The fixture monitor follows injected request time monotonically;
                // the production monitor always reads BOOTTIME without this clamp.
                clock().map(|now| now.max(registry.controller_tick.unwrap_or(0)))
            },
        )?);
        Ok(())
    }
    #[cfg(test)]
    pub(in crate::authority) fn with_handy_player_clock_for_test<T>(
        now: u64,
        action: impl FnOnce() -> T,
    ) -> T {
        struct Restore(Option<u64>);
        impl Drop for Restore {
            fn drop(&mut self) {
                TEST_CLOCK.with(|clock| clock.set(self.0));
            }
        }
        let _restore = Restore(TEST_CLOCK.with(|clock| clock.replace(Some(now))));
        action()
    }
    /// Called before live sweeps and DB dispatch. Only process-local authority
    /// is read: no effect journal, filesystem, network or main database access.
    pub(in crate::authority) fn handy_player_guard_fast(
        &self,
        r: &Request,
    ) -> Option<PResult<ResponseBody>> {
        if !matches!(
            r.command,
            Command::IssueHandyPlayerChallenge { .. }
                | Command::ObserveHandyPlayer { .. }
                | Command::ReleaseHandyPlayerGuard { .. }
        ) {
            return None;
        }
        Some(self.handy_player_guard_fast_with(r, fast_clock))
    }
    fn handy_player_guard_fast_with(
        &self,
        r: &Request,
        clock: impl FnOnce() -> PResult<u64>,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        let (binding_id, player, owner_proof) = match &r.command {
            Command::IssueHandyPlayerChallenge {
                binding_id,
                player,
                owner_proof,
                ..
            }
            | Command::ObserveHandyPlayer {
                binding_id,
                player,
                owner_proof,
                ..
            }
            | Command::ReleaseHandyPlayerGuard {
                binding_id,
                player,
                owner_proof,
            } => (binding_id, player, owner_proof),
            _ => return Err(ProtocolError::unsupported("native player operation")),
        };
        if !same_secret(&self.handy_owner_token, owner_proof) {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "Handy owner capability required",
            ));
        }
        let principal = r
            .session
            .as_ref()
            .ok_or_else(|| ProtocolError::new(ErrorCode::Unauthorized, "pairing is required"))?;
        let hash = token_hash(r)?;
        let healthy = if matches!(r.command, Command::ReleaseHandyPlayerGuard { .. }) {
            true
        } else {
            let busy = |_| ProtocolError::new(ErrorCode::Unavailable, "native expiry owner busy");
            let monitor = self.handy_controller_monitor.try_lock().map_err(busy)?;
            let alive = monitor
                .as_ref()
                .is_some_and(|m| m.thread.as_ref().is_some_and(|t| !t.is_finished()));
            drop(monitor);
            let execution = self.handy_execution.try_lock().map_err(|_| {
                ProtocolError::new(ErrorCode::Unavailable, "native stop owner busy")
            })?;
            alive && execution.as_ref().is_some_and(ExecutionPump::is_running)
        };
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        if self.is_shutting_down() {
            return Err(mismatch());
        }
        // Authenticate exact cached scope before allowing even its expiry sweep.
        let id = registry
            .entries
            .iter()
            .find_map(|(id, entry)| {
                let guard = entry.player_guard.as_ref()?;
                (guard.report.binding.binding_id == *binding_id
                    && guard.report.binding.player == *player
                    && r.project.as_ref() == Some(&guard.report.binding.controller.project_id)
                    && guard.principal == *principal
                    && same_secret(&guard.auth_hash, &hash))
                .then(|| id.clone())
            })
            .ok_or_else(mismatch)?;
        if !healthy {
            registry.controller_clock_failed = true;
            let _ = registry.sweep_controllers(clock);
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "native expiry or stop owner stopped",
            ));
        }
        let now = registry.sweep_controllers(clock)?;
        let entry = registry.entries.get_mut(&id).ok_or_else(mismatch)?;
        let guard = entry.player_guard.as_mut().ok_or_else(mismatch)?;
        match &r.command {
            Command::IssueHandyPlayerChallenge { sequence, .. } => {
                if guard.report.last_sequence.checked_add(1) != Some(*sequence) {
                    return Err(mismatch());
                }
                if let Some(pending) = &guard.pending {
                    if pending.sequence != *sequence || now >= pending.expires_at_boottime_ns {
                        return Err(mismatch());
                    }
                    return Ok(ResponseBody::HandyPlayerChallenge(Box::new(
                        pending.clone(),
                    )));
                }
                let expires =
                    deadline(now, guard.report.binding.policy.challenge_timeout_ms.into())?
                        .min(guard.report.heartbeat_deadline_boottime_ns)
                        .min(guard.report.media_progress_deadline_boottime_ns)
                        .min(
                            entry
                                .controller
                                .as_ref()
                                .ok_or_else(mismatch)?
                                .report
                                .deadline_boottime_ns,
                        );
                if expires <= now {
                    return Err(mismatch());
                }
                let challenge = HandyPlayerChallenge {
                    binding_id: binding_id.clone(),
                    player: player.clone(),
                    sequence: *sequence,
                    challenge_id: opaque_id(),
                    issued_at_boottime_ns: now,
                    expires_at_boottime_ns: expires,
                    current: true,
                };
                challenge.validate()?;
                guard.pending = Some(challenge.clone());
                Ok(ResponseBody::HandyPlayerChallenge(Box::new(challenge)))
            }
            Command::ObserveHandyPlayer {
                sequence,
                challenge_id,
                sample,
                ..
            } => {
                let pending = guard
                    .pending
                    .as_ref()
                    .filter(|pending| {
                        pending.sequence == *sequence
                            && pending.challenge_id == *challenge_id
                            && now >= pending.issued_at_boottime_ns
                            && now < pending.expires_at_boottime_ns
                            && guard.report.last_sequence.checked_add(1) == Some(*sequence)
                    })
                    .ok_or_else(mismatch)?;
                let issued = pending.issued_at_boottime_ns;
                let backwards = guard.report.last_sample.as_ref().is_some_and(|old| {
                    sample.browser_tick_us < old.browser_tick_us
                        || sample.media_time_us < old.media_time_us
                        || sample.presented_frames < old.presented_frames
                });
                let progress =
                    guard
                        .report
                        .last_sample
                        .as_ref()
                        .map_or(sample.presented_frames > 0, |old| {
                            sample.media_time_us > old.media_time_us
                                && sample.presented_frames > old.presented_frames
                        });
                let safe = sample.playing
                    && !sample.seeking
                    && !sample.ended
                    && sample.visible
                    && sample.playback_rate_milli == 1000
                    && !backwards
                    && guard
                        .report
                        .binding
                        .browser_media
                        .as_ref()
                        .is_none_or(|binding| {
                            i64::try_from(sample.media_time_us)
                                .ok()
                                .and_then(|us| SourceTimestamp::new(us, 1_000_000).ok())
                                .is_some_and(|time| {
                                    binding
                                        .clock
                                        .project_at(time, &binding.selection.placement.spec)
                                        .is_ok()
                                })
                        });
                // Consume only an authenticated exact nonce, including a valid
                // report of a stopped player. Unknown old nonces remain inert.
                guard.pending = None;
                guard.report.last_sequence = *sequence;
                guard.report.last_sample = Some(sample.clone());
                if safe {
                    guard.report.heartbeat_deadline_boottime_ns = deadline(
                        issued,
                        guard.report.binding.policy.heartbeat_timeout_ms.into(),
                    )?;
                    if progress {
                        guard.report.media_progress_deadline_boottime_ns = deadline(
                            issued,
                            guard.report.binding.policy.media_stall_timeout_ms.into(),
                        )?;
                    }
                    return report_body(guard.report.clone());
                }
                let mut report = guard.report.clone();
                retire_entry(entry);
                report.current = false;
                report.physical_stop_pending = entry
                    .controller
                    .as_ref()
                    .is_none_or(|l| l.report.physical_stop_pending);
                report_body(report)
            }
            Command::ReleaseHandyPlayerGuard { .. } => {
                let mut report = guard.report.clone();
                retire_entry(entry);
                report.current = false;
                report.physical_stop_pending = entry
                    .controller
                    .as_ref()
                    .is_none_or(|l| l.report.physical_stop_pending);
                report_body(report)
            }
            _ => unreachable!("validated fast command"),
        }
    }
    fn player_guard_view<F: FnMut() -> PResult<u64>>(
        &self,
        db: &Connection,
        r: &Request,
        actor: &SessionId,
        mut saved: HandyPlayerGuardReport,
        clock: &mut F,
    ) -> PResult<ResponseBody> {
        saved.validate()?;
        if saved.binding.controller.project_id != *project(r)? {
            return Err(mismatch());
        }
        self.retire_changed_handy_preparations(db);
        let _ = self.controller_view(db, actor, saved.binding.controller.clone(), r, clock)?;
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        let now = registry.sweep_controllers(clock);
        if let Ok(now) = now {
            if let Some(entry) = registry
                .entries
                .get(&saved.binding.controller.preparation_id)
            {
                if let Some(guard) = &entry.player_guard {
                    if guard.principal == *actor
                        && entry.owner == *actor
                        && guard.report.binding.binding_id == saved.binding.binding_id
                        && encode(&guard.report.binding)? == encode(&saved.binding)?
                        && guard.current(entry, now)
                    {
                        return report_body(guard.report.clone());
                    }
                }
            }
        }
        saved.current = false;
        report_body(saved)
    }
    pub(in crate::authority) fn handy_player_guard_control(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        self.handy_player_guard_with(r, actor, fingerprint, boottime_ns)
    }
    pub(in crate::authority) fn handy_player_guard_with<F: FnMut() -> PResult<u64>>(
        &self,
        r: &Request,
        actor: &SessionId,
        fingerprint: &str,
        mut clock: F,
    ) -> PResult<ResponseBody> {
        r.validate()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, r, actor)?;
        let owner_current = self.check_handy_controller_monitor()?;
        check_request_identity(&db, actor.as_str(), r, fingerprint)?;
        if let Command::HandyPlayerGuardStatus {
            binding_request_id, ..
        } = &r.command
        {
            let raw: Option<String> = db.query_row(
                "SELECT CASE WHEN length(CAST(response AS BLOB))<=32768 THEN response ELSE NULL END FROM requests WHERE session=?1 AND request=?2",
                params![actor.as_str(), binding_request_id.as_str()], |row| row.get(0),
            ).optional().map_err(internal)?;
            let ResponseBody::HandyPlayerGuard(saved) = decode(&raw.ok_or_else(|| {
                ProtocolError::new(ErrorCode::NotFound, "player binding history not found")
            })?)?
            else {
                return Err(mismatch());
            };
            if saved.binding.request_id != *binding_request_id {
                return Err(mismatch());
            }
            return self.player_guard_view(&db, r, actor, *saved, &mut clock);
        }
        let Command::BindHandyPlayerGuard {
            selection,
            player,
            policy,
            browser_media,
            ..
        } = &r.command
        else {
            return Err(ProtocolError::unsupported("player guard binding"));
        };
        if let Some(body) = replay(&db, actor.as_str(), r, fingerprint)? {
            let ResponseBody::HandyPlayerGuard(saved) = body else {
                return Err(mismatch());
            };
            if saved.binding.request_id != r.request_id
                || saved.binding.selection != *selection
                || saved.binding.player != *player
                || saved.binding.policy != *policy
                || saved.binding.browser_media != *browser_media
            {
                return Err(mismatch());
            }
            return self.player_guard_view(&db, r, actor, *saved, &mut clock);
        }
        if !owner_current {
            return Err(mismatch());
        }
        check_revision(&db, r)?;
        let media_pin = browser_media
            .as_ref()
            .map(|binding| self.pin_browser_media_locked(&db, r, binding))
            .transpose()?;
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
            if entry.player_guard.is_some()
                || !selected(r, selection, &lease, &pins)
                || !matches_play(entry, &pins, &entry.runtime)
            {
                return Err(mismatch());
            }
            (pins, entry.runtime.clone())
        };
        self.continuous_acknowledged(&db, actor, &pins)?;
        let (current, _) = self.preparation_references(&db, actor, &pins.preparation, r)?;
        if encode(&current.connection)? != encode(&pins.captured.connection)?
            || !Arc::ptr_eq(&current.credentials, &pins.captured.credentials)
            || !current.hosted.same_location(&pins.captured.hosted)
        {
            return Err(mismatch());
        }
        let auth_hash = token_hash(r)?;
        if !self.check_handy_controller_monitor()? {
            return Err(mismatch());
        }
        let bound_at = {
            let mut registry = self.handy_preparations.lock().map_err(internal)?;
            let now = registry.sweep_controllers(&mut clock)?;
            let entry = registry
                .entries
                .get(&lease.preparation_id)
                .ok_or_else(mismatch)?;
            if entry.player_guard.is_some()
                || !matches_play(entry, &pins, &runtime)
                || !entry
                    .controller
                    .as_ref()
                    .is_some_and(|l| selected(r, selection, &l.report, &pins))
            {
                return Err(mismatch());
            }
            now
        };
        let mut report = HandyPlayerGuardReport {
            binding: HandyPlayerGuardBinding {
                browser_media: browser_media.clone(),
                request_id: r.request_id.clone(),
                binding_id: opaque_id(),
                selection: selection.clone(),
                player: player.clone(),
                policy: policy.clone(),
                controller: lease,
            },
            bound_at_boottime_ns: bound_at,
            heartbeat_deadline_boottime_ns: deadline(bound_at, policy.heartbeat_timeout_ms.into())?,
            media_progress_deadline_boottime_ns: deadline(
                bound_at,
                policy.media_stall_timeout_ms.into(),
            )?,
            last_sequence: 0,
            last_sample: None,
            current: true,
            synchronized: false,
            playback_enabled: false,
            physical_stop_pending: true,
        };
        report.validate()?;
        let tx = db.transaction().map_err(internal)?;
        remember(
            &tx,
            actor.as_str(),
            r,
            fingerprint,
            &ResponseBody::HandyPlayerGuard(Box::new(report.clone())),
        )?;
        tx.commit().map_err(internal)?;
        let owner_current = self.check_handy_controller_monitor()?;
        let mut registry = self.handy_preparations.lock().map_err(internal)?;
        report.current = registry.sweep_controllers(&mut clock).is_ok_and(|now| {
            owner_current
                && media_pin.as_ref().is_none_or(|pin| pin.current())
                && now < report.heartbeat_deadline_boottime_ns
                && now < report.media_progress_deadline_boottime_ns
                && registry
                    .entries
                    .get(&report.binding.controller.preparation_id)
                    .is_some_and(|entry| {
                        entry.player_guard.is_none()
                            && matches_play(entry, &pins, &runtime)
                            && entry
                                .controller
                                .as_ref()
                                .is_some_and(|l| selected(r, selection, &l.report, &pins))
                    })
        });
        if report.current {
            registry
                .entries
                .get_mut(&report.binding.controller.preparation_id)
                .expect("checked binding target")
                .player_guard = Some(RetainedPlayerGuard {
                report: report.clone(),
                principal: actor.clone(),
                auth_hash,
                pins,
                runtime,
                pending: None,
                browser_media: media_pin,
            });
        }
        report_body(report)
    }
}
