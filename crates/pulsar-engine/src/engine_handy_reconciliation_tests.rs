fn reconciliation_contract_fixture(input: &mut HandyQualificationInput) {
    input.subject.schema_version = 5;
    input.subject.server_clock = Some(HandyServerClockQualification {
        model: HandyServerClockModel::RequestBracketMonotonic,
        timestamp_error_ns: 1,
    });
    input.subject.device_clock = Some(HandyDeviceClockQualification {
        model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
        user_offset_ms: 0,
        device_error_ns: 1,
        maximum_sync_age_ns: 1_000_000,
        maximum_sync_round_trip_ms: 1,
        drift_ppm: 1,
    });
    input.subject.recovery = Some(HandyRecoveryQualification {
        transport: HandyQualificationTransport::Hssp,
        model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
        remote_state: HandyRecoveryRemoteState::Stopped,
        prior_command_settle_ns: 1_000_000,
        stopped_sample_separation_ns: 1_000_000,
        maximum_sample_span_ns: 100_000_000,
    });
}
fn reconciliation_fixture() -> (Fixture, Request, HandyRecoveryPlan) {
    let f = fixture();
    // Physical is a simulated qualification origin in this fixture, not real
    // measured evidence or acceptance of a device outside this test process.
    let (setup, accepted) =
        setup_send_request_with(&f, "recovery-setup", reconciliation_contract_fixture);
    setup_send_call(
        &f,
        &setup,
        |h, _| Ok(h.downloaded.clone()),
        |_, _| Ok(setup_sample()),
        |_, _, _, _| crate::device_effect_journal::SetupCompletion::Unknown,
    )
    .unwrap();
    let Command::SetupHandyDevice {
        engine_epoch,
        device_session,
        ..
    } = setup.command
    else {
        panic!("setup selection");
    };
    let registration = handy_registration_catalog(&f).registrations.remove(0);
    let plan_request = request(
        &f.session,
        Some(&f.project),
        None,
        Command::PlanHandyReconciliation {
            selection: HandyRecoverySelection {
                device_id: registration.device_id,
                registration_revision: registration.revision,
                engine_epoch,
                device_session,
                qualification: HandyQualificationBinding {
                    dossier_id: accepted.dossier_id,
                    revision: accepted.revision,
                },
            },
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    let ResponseBody::HandyRecoveryPlan(plan) =
        f.engine.handle(plan_request.clone()).result.unwrap()
    else {
        panic!("recovery plan");
    };
    plan.validate().unwrap();
    (f, plan_request, *plan)
}
fn reconciliation_start(f: &Fixture, plan: &HandyRecoveryPlan, id: &str) -> Request {
    let mut r = request(
        &f.session,
        Some(&f.project),
        None,
        Command::ReconcileHandyEffects {
            plan_id: plan.plan_id.clone(),
            allow_online_inspection: true,
            allow_resolution: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
fn reconciliation_call<F>(f: &Fixture, r: &Request, inspector: F) -> PResult<HandyRecoveryStatus>
where
    F: Fn(
            Arc<crate::handy_credentials::Credentials>,
            std::time::Instant,
        ) -> PResult<HandySetupDeviceObservation>
        + Send
        + Sync
        + 'static,
{
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    let ResponseBody::HandyRecovery(status) =
        f.engine
            .handy_reconciliation_with(r, &f.session, &fingerprint, Arc::new(inspector))?
    else {
        panic!("recovery status");
    };
    status.validate()?;
    Ok(*status)
}
fn reconciliation_status(f: &Fixture, original: &RequestId) -> HandyRecoveryStatus {
    let r = request(
        &f.session,
        Some(&f.project),
        None,
        Command::HandyReconciliationStatus {
            reconciliation_request_id: original.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    let ResponseBody::HandyRecovery(status) = f.engine.handle(r).result.unwrap() else {
        panic!("recovery status");
    };
    status.validate().unwrap();
    *status
}
fn reconciliation_wait(f: &Fixture, original: &RequestId) -> HandyRecoveryStatus {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let status = reconciliation_status(f, original);
        if status.state != HandyRecoveryState::Pending {
            return status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "reconciliation did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
fn reconciliation_cancel(f: &Fixture, original: &RequestId, id: &str) -> Request {
    let mut r = request(
        &f.session,
        Some(&f.project),
        None,
        Command::CancelHandyReconciliation {
            reconciliation_request_id: original.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}

fn reconciliation_export_fixture(f: &Fixture, start: &Request, completed: &HandyRecoveryStatus) {
    let Some(directory) = std::env::var_os("PULSAR_RECONCILIATION_FIXTURE_DIR") else {
        return;
    };
    let directory = PathBuf::from(directory);
    assert!(
        directory.is_absolute(),
        "fixture output must be an explicitly selected absolute directory"
    );
    if directory.exists() {
        assert!(!fs::symlink_metadata(&directory)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(
            fs::read_dir(&directory).unwrap().next().is_none(),
            "fixture output must be empty"
        );
    } else {
        fs::create_dir_all(&directory).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let journal_directory = directory.join("device-effects");
    fs::create_dir(&journal_directory).unwrap();
    let backup = journal_directory.join("journal.sqlite3");
    let journal = Connection::open_with_flags(
        f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    journal
        .busy_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    journal
        .execute("VACUUM INTO ?1", [backup.to_str().unwrap()])
        .unwrap();
    let snapshot =
        rusqlite::Connection::open_with_flags(&backup, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let journal_schema_version: u32 = snapshot
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    drop(snapshot);
    drop(journal);
    let resolution = completed.resolution.as_ref().unwrap();
    let verified = crate::device_effect_journal::DeviceEffectJournal::verify_resolution(
        &directory,
        &f.session,
        &start.request_id,
        resolution.record_id.as_str(),
    )
    .unwrap();
    assert_eq!(verified.plan.plan_id, completed.plan.plan_id);
    assert_eq!(verified.request_id, start.request_id);
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(start).unwrap()));
    assert_eq!(verified.request_fingerprint, fingerprint);
    let mut pending = completed.clone();
    pending.state = HandyRecoveryState::Pending;
    pending.resolution = None;
    pending.error = None;
    pending.validate().unwrap();
    let mut interrupted = pending.clone();
    interrupted.state = HandyRecoveryState::Interrupted;
    interrupted.error = Some(ProtocolError::new(
        ErrorCode::Unavailable,
        "synthetic absent original worker",
    ));
    interrupted.validate().unwrap();
    let bytes = fs::read(&backup).unwrap();
    assert!(
        bytes.len() <= 64 * 1024 * 1024,
        "fixture journal exceeds export bound"
    );
    let export = serde_json::json!({
        "format_version":1,"synthetic_history_only":true,"qualification_acceptance_claim":false,
        "playback_enabled":false,"authority_restored":false,"real_device_contacted":false,
        "actor_session":f.session,"project_id":f.project,"original_request_id":start.request_id,
        "original_request_fingerprint":fingerprint,"completed_status":completed,
        "original_request_row":{"session":f.session,"request":start.request_id,"fingerprint":fingerprint,
            "response_json":encode(&ResponseBody::HandyRecovery(Box::new(completed.clone()))).unwrap()},
        "pending_response_json":encode(&ResponseBody::HandyRecovery(Box::new(pending))).unwrap(),
        "interrupted_response_json":encode(&ResponseBody::HandyRecovery(Box::new(interrupted))).unwrap(),
        "journal":{"relative_path":"device-effects/journal.sqlite3","sha256":format!("{:x}",Sha256::digest(&bytes)),"byte_len":bytes.len(),"schema_version":journal_schema_version},
        "native_fixture_instruction":"Rebind only newly generated private session/project identities to these synthetic IDs; retain the native app's own authentication material and empty project program. Export grants read/export for the selected project. No qualification dossier or acceptance is exported."
    });
    let encoded = serde_json::to_vec_pretty(&export).unwrap();
    assert!(encoded.len() <= 256 * 1024);
    for secret in std::iter::once(f.engine.token.as_str())
        .chain(std::iter::once(f.engine.handy_owner_token.as_str()))
        .chain(start.auth_token.as_deref())
    {
        assert!(!secret.is_empty());
        assert!(
            !encoded
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "capability appeared in public fixture"
        );
        assert!(
            !bytes
                .windows(secret.len())
                .any(|window| window == secret.as_bytes()),
            "capability appeared in journal fixture"
        );
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("fixture.json"))
        .unwrap();
    output.write_all(&encoded).unwrap();
    output.write_all(b"\n").unwrap();
    output.sync_all().unwrap();
}

#[test]
fn handy_reconciliation_samples_once_preserves_original_effects_and_repairs_only_metadata() {
    let (f, _, plan) = reconciliation_fixture();
    let original_rows: Vec<(String, String, String)> = {
        let db = Connection::open(
            f.engine
                .config
                .state_dir
                .join("device-effects/journal.sqlite3"),
        )
        .unwrap();
        let mut q = db
            .prepare("SELECT id,record,state FROM setup_effects ORDER BY id")
            .unwrap();
        let rows = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        rows
    };
    let samples = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = samples.clone();
    let start = reconciliation_start(&f, &plan, "recovery-once");
    let accepted = reconciliation_call(&f, &start, move |_, _| {
        observed.fetch_add(1, Ordering::AcqRel);
        Ok(setup_sample())
    })
    .unwrap();
    assert_eq!(accepted.state, HandyRecoveryState::Pending);
    let finished = reconciliation_wait(&f, &start.request_id);
    assert_eq!(
        finished.state,
        HandyRecoveryState::Completed,
        "{:?}",
        finished.error
    );
    assert_eq!(samples.load(Ordering::Acquire), 2);
    assert!(!finished.authority_restored && !finished.playback_enabled);
    reconciliation_export_fixture(&f, &start, &finished);
    let replayed = reconciliation_call(&f, &start, |_, _| {
        panic!("historical retry must not sample")
    })
    .unwrap();
    assert_eq!(encode(&replayed).unwrap(), encode(&finished).unwrap());
    let cancellation = reconciliation_cancel(&f, &start.request_id, "cancel-after-commit");
    let ResponseBody::HandyRecovery(cancelled) = f.engine.handle(cancellation).result.unwrap()
    else {
        panic!();
    };
    assert_eq!(cancelled.state, HandyRecoveryState::Completed);
    assert_eq!(encode(&*cancelled).unwrap(), encode(&finished).unwrap());
    {
        let db = Connection::open(
            f.engine
                .config
                .state_dir
                .join("device-effects/journal.sqlite3"),
        )
        .unwrap();
        let mut q = db
            .prepare("SELECT id,record,state FROM setup_effects ORDER BY id")
            .unwrap();
        let rows: Vec<(String, String, String)> = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(rows, original_rows);
    }
    // Simulate a lost main completion response after the independent journal commit.
    let mut stale = finished.clone();
    stale.state = HandyRecoveryState::Pending;
    stale.resolution = None;
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
            params![
                encode(&ResponseBody::HandyRecovery(Box::new(stale))).unwrap(),
                f.session.as_str(),
                start.request_id.as_str()
            ],
        )
        .unwrap();
    assert_eq!(
        encode(&reconciliation_status(&f, &start.request_id)).unwrap(),
        encode(&finished).unwrap()
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM requests WHERE session=?1 AND request=?2",
            params![f.session.as_str(), start.request_id.as_str()],
        )
        .unwrap();
    assert_eq!(
        encode(&reconciliation_status(&f, &start.request_id)).unwrap(),
        encode(&finished).unwrap()
    );
    assert_eq!(samples.load(Ordering::Acquire), 2);
    f.engine.finish_shutdown().unwrap();
    let Fixture {
        engine,
        session,
        project,
        dir,
    } = f;
    let config = Engine::retire_for_test(engine);
    let reopened = Engine::open(config).unwrap();
    let mut query = request(
        &session,
        Some(&project),
        None,
        Command::HandyReconciliationStatus {
            reconciliation_request_id: start.request_id,
            owner_proof: reopened.handy_owner_token.clone(),
        },
    );
    query.auth_token = start.auth_token;
    let ResponseBody::HandyRecovery(after) = reopened.handle(query).result.unwrap() else {
        panic!();
    };
    assert_eq!(encode(&*after).unwrap(), encode(&finished).unwrap());
    reopened.finish_shutdown().unwrap();
    Engine::retire_for_test(reopened);
    drop(dir);
}

#[test]
fn handy_reconciliation_cancellation_receipt_failure_preserves_original_and_flag() {
    let (f, _, plan) = reconciliation_fixture();
    let admission = f.engine.handy_reconciliation_admission.write().unwrap();
    let start = reconciliation_start(&f, &plan, "recovery-cancel");
    reconciliation_call(&f, &start, |_, _| {
        panic!("cancelled before admission cannot sample")
    })
    .unwrap();
    let cancel = reconciliation_cancel(&f, &start.request_id, "cancel-write-fails");
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER reject_recovery_cancel BEFORE INSERT ON requests WHEN NEW.request='cancel-write-fails' BEGIN SELECT RAISE(ABORT,'synthetic cancellation receipt failure'); END;").unwrap();
    assert!(f.engine.handle(cancel.clone()).result.is_err());
    assert_eq!(
        reconciliation_status(&f, &start.request_id).state,
        HandyRecoveryState::Pending
    );
    assert_eq!(
        f.engine
            .handy_reconciliations
            .lock()
            .unwrap()
            .worker_cancelled(&f.session, &start.request_id),
        Some(false)
    );
    {
        let db = f.engine.db.lock().unwrap();
        let count: u64 = db
            .query_row(
                "SELECT count(*) FROM request_identities WHERE session=?1 AND request=?2",
                params![f.session.as_str(), cancel.request_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        db.execute_batch("DROP TRIGGER reject_recovery_cancel")
            .unwrap();
    }
    let ResponseBody::HandyRecovery(cancelled) = f.engine.handle(cancel).result.unwrap() else {
        panic!();
    };
    assert_eq!(cancelled.state, HandyRecoveryState::Cancelled);
    assert_ne!(
        f.engine
            .handy_reconciliations
            .lock()
            .unwrap()
            .worker_cancelled(&f.session, &start.request_id),
        Some(false)
    );
    drop(admission);
    assert_eq!(
        reconciliation_wait(&f, &start.request_id).state,
        HandyRecoveryState::Cancelled
    );
    f.engine.finish_shutdown().unwrap();
    assert!(f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .resolution_history(&f.session, &start.request_id)
        .unwrap()
        .is_none());
}

#[test]
fn handy_reconciliation_missing_worker_is_interrupted_and_never_recreated_by_retry() {
    let (f, _, plan) = reconciliation_fixture();
    let start = reconciliation_start(&f, &plan, "recovery-orphan");
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&start).unwrap()));
    let pending = HandyRecoveryStatus {
        project_id: f.project.clone(),
        reconciliation_request_id: start.request_id.clone(),
        plan,
        state: HandyRecoveryState::Pending,
        resolution: None,
        error: None,
        playback_enabled: false,
        authority_restored: false,
    };
    {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        remember(
            &tx,
            f.session.as_str(),
            &start,
            &fingerprint,
            &ResponseBody::HandyRecovery(Box::new(pending)),
        )
        .unwrap();
        tx.commit().unwrap();
    }
    let first = reconciliation_status(&f, &start.request_id);
    assert_eq!(first.state, HandyRecoveryState::Interrupted);
    let replayed =
        reconciliation_call(&f, &start, |_, _| panic!("orphan history cannot sample")).unwrap();
    assert_eq!(encode(&first).unwrap(), encode(&replayed).unwrap());
    assert!(first.resolution.is_none() && first.error.is_some());
    f.engine.finish_shutdown().unwrap();
}

#[test]
fn handy_reconciliation_changed_samples_fail_without_resolving_custody() {
    let (f, _, plan) = reconciliation_fixture();
    let start = reconciliation_start(&f, &plan, "recovery-wrong-identity");
    reconciliation_call(&f, &start, |_, _| {
        let mut observation = setup_sample();
        observation.identity.firmware.push('x');
        Ok(observation)
    })
    .unwrap();
    let failed = reconciliation_wait(&f, &start.request_id);
    assert_eq!(failed.state, HandyRecoveryState::Failed);
    assert!(failed.resolution.is_none() && failed.error.is_some());
    assert!(f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .resolution_history(&f.session, &start.request_id)
        .unwrap()
        .is_none());
    let snapshot = f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .custody_snapshot(
            &plan.selection.device_id,
            &plan.route_sha256,
            &plan.route_namespace_sha256,
        )
        .unwrap();
    assert_eq!(snapshot.id().unwrap(), plan.snapshot_id.as_str());
    f.engine.finish_shutdown().unwrap();
}

#[test]
fn handy_reconciliation_final_current_acceptance_is_rechecked_after_sampling() {
    let (f, _, plan) = reconciliation_fixture();
    let revoke = qualification_withdraw(
        &f,
        &plan.selection.qualification.dossier_id,
        plan.selection.qualification.revision,
    );
    let engine = f.engine.clone();
    let count = std::sync::atomic::AtomicUsize::new(0);
    let start = reconciliation_start(&f, &plan, "recovery-revoked");
    reconciliation_call(&f, &start, move |_, _| {
        if count.fetch_add(1, Ordering::AcqRel) == 1 {
            engine.handle(revoke.clone()).result.unwrap();
        }
        Ok(setup_sample())
    })
    .unwrap();
    let failed = reconciliation_wait(&f, &start.request_id);
    assert_eq!(failed.state, HandyRecoveryState::Failed);
    assert!(failed.resolution.is_none());
    assert!(f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .resolution_history(&f.session, &start.request_id)
        .unwrap()
        .is_none());
    f.engine.finish_shutdown().unwrap();
}

#[test]
fn handy_reconciliation_admission_excludes_producers_but_not_history_and_release() {
    let (f, _, plan) = reconciliation_fixture();
    let guard = f.engine.handy_reconciliation_admission.write().unwrap();
    let setup = Command::SetupHandyDevice {
        qualification: plan.selection.qualification.clone(),
        engine_epoch: plan.selection.engine_epoch.clone(),
        device_session: plan.selection.device_session.clone(),
        hosting_request_id: RequestId::new("host").unwrap(),
        allow_online_setup: true,
        owner_proof: "proof".into(),
    };
    assert!(f.engine.handy_reconciliation_admission(&setup).is_err());
    for command in [
        Command::HandyReconciliationStatus {
            reconciliation_request_id: RequestId::new("original").unwrap(),
            owner_proof: "proof".into(),
        },
        Command::CancelHandyReconciliation {
            reconciliation_request_id: RequestId::new("original").unwrap(),
            owner_proof: "proof".into(),
        },
        Command::ReleaseHandyController {
            acquisition_request_id: RequestId::new("acquire").unwrap(),
            controller_id: ClientId::new("controller").unwrap(),
            owner_proof: "proof".into(),
        },
    ] {
        assert!(f
            .engine
            .handy_reconciliation_admission(&command)
            .unwrap()
            .is_none());
    }
    drop(guard);
    f.engine.finish_shutdown().unwrap();
}

#[test]
fn handy_reconciliation_startup_rejects_a_changed_completed_witness_without_repairing_it() {
    let (f, _, plan) = reconciliation_fixture();
    let start = reconciliation_start(&f, &plan, "recovery-startup-witness");
    reconciliation_call(&f, &start, |_, _| Ok(setup_sample())).unwrap();
    let completed = reconciliation_wait(&f, &start.request_id);
    assert_eq!(completed.state, HandyRecoveryState::Completed);
    f.engine.finish_shutdown().unwrap();
    let Fixture {
        engine,
        session,
        dir,
        ..
    } = f;
    let config = Engine::retire_for_test(engine);
    let db = Connection::open(config.state_dir.join("projects.sqlite3")).unwrap();
    let original = encode(&ResponseBody::HandyRecovery(Box::new(completed.clone()))).unwrap();
    let mut substituted = completed;
    substituted.resolution.as_mut().unwrap().snapshot_id = ArtifactId::new("9".repeat(64)).unwrap();
    substituted.validate().unwrap();
    let corrupt = encode(&ResponseBody::HandyRecovery(Box::new(substituted))).unwrap();
    db.execute(
        "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
        params![&corrupt, session.as_str(), start.request_id.as_str()],
    )
    .unwrap();
    let error = Engine::open(config.clone())
        .err()
        .expect("changed witness must refuse startup");
    assert!(
        error.to_string().contains("recovery witness differs"),
        "{error:#}"
    );
    let after: String = db
        .query_row(
            "SELECT response FROM requests WHERE session=?1 AND request=?2",
            params![session.as_str(), start.request_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after, corrupt);
    db.execute(
        "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
        params![original, session.as_str(), start.request_id.as_str()],
    )
    .unwrap();
    drop(db);
    let recovered = Engine::open(config).unwrap();
    recovered.finish_shutdown().unwrap();
    Engine::retire_for_test(recovered);
    drop(dir);
}

#[test]
fn handy_reconciliation_historical_physical_subject_cannot_be_replaced_by_route_equality() {
    let (f, _, plan) = reconciliation_fixture();
    let db = f.engine.db.lock().unwrap();
    let accepted = handy_qualification::accepted_recovery_record(
        &db,
        &plan.selection.qualification,
        &plan.registration,
    )
    .unwrap();
    let snapshot = f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .custody_snapshot(
            &plan.selection.device_id,
            &plan.route_sha256,
            &plan.route_namespace_sha256,
        )
        .unwrap();
    handy_reconciliation::historical_subjects(&db, &snapshot, &accepted.manifest.subject).unwrap();
    let mut changed = accepted.manifest.subject.clone();
    changed.identity.firmware.push('x');
    assert!(handy_reconciliation::historical_subjects(&db, &snapshot, &changed).is_err());
    let mut changed = accepted.manifest.subject;
    let mut settings = serde_json::to_value(changed.operative_settings.as_ref().unwrap()).unwrap();
    settings["slider"]["x_limit_stop"] = serde_json::json!(99.0);
    changed.operative_settings = Some(serde_json::from_value(settings).unwrap());
    assert!(handy_reconciliation::historical_subjects(&db, &snapshot, &changed).is_err());
    drop(db);
    f.engine.finish_shutdown().unwrap();
}

fn reconciliation_other_controller(
    f: &Fixture,
    source: &HandyControllerLease,
) -> (
    HandyControllerLease,
    Arc<crate::handy_credentials::Credentials>,
) {
    let mut register = handy_registration_request(
        f,
        "liveness-target-registration",
        0,
        "separate recovery target",
    );
    let Command::SetHandyRegistration { device_id, .. } = &mut register.command else {
        panic!();
    };
    *device_id = DeviceId::new("liveness-recovery-target").unwrap();
    let registration = handy_registration_saved(f.engine.handle(register));
    let credentials = Arc::new(crate::handy_credentials::Credentials {
        api: "synthetic-target-api".into(),
        connection: "synthetic-target-route".into(),
    });
    let open = handy_session_open_request(f, "liveness-target-session", &registration);
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&open).unwrap()));
    let connection = handy_session_report(
        f.engine
            .handy_session_control_with(
                &open,
                &f.session,
                &fingerprint,
                |_| Ok(credentials.clone()),
                |_| Ok(handy_session_observation()),
            )
            .unwrap(),
    );
    let (_, mut input) = qualification_input(f, HandyEvidenceOrigin::Physical);
    input.subject.device_id = registration.device_id;
    input.subject.registration_revision = registration.revision;
    input.subject.registration_binding_sha256 = registration.binding_sha256;
    input.subject.unit_label =
        "synthetic second unit for renewal liveness; not physical evidence".into();
    let imported = qualification_saved(
        f.engine
            .handle(qualification_import(
                f,
                "liveness-target-import",
                &f.dir.path().join("liveness-target-qualification.json"),
                &input,
            ))
            .result
            .unwrap(),
    );
    let accepted = qualification_saved(
        qualification_call(
            f,
            &qualification_accept(f, &imported.dossier_id, &connection),
            |_| Ok(setup_sample()),
        )
        .unwrap(),
    );
    let prior = playback_preparation_report(
        f.engine
            .handle(preparation_status_request(
                f,
                &source.preparation_request_id,
            ))
            .result
            .unwrap(),
    );
    let mut setup = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::SetupHandyDevice {
            qualification: HandyQualificationBinding {
                dossier_id: accepted.dossier_id,
                revision: accepted.revision,
            },
            engine_epoch: connection.engine_epoch,
            device_session: connection.device_session,
            hosting_request_id: prior.correlation.inspection.hosting_request_id,
            allow_online_setup: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    setup.request_id = RequestId::new("liveness-target-setup").unwrap();
    let history = setup_status_report(
        setup_send_call(
            f,
            &setup,
            |h, _| Ok(h.downloaded.clone()),
            |_, _| Ok(setup_sample()),
            |_, _, _, _| {
                crate::device_effect_journal::SetupCompletion::Acknowledged(
                    crate::handy_wire::fixture_setup_acknowledgment(),
                )
            },
        )
        .unwrap(),
    )
    .history
    .unwrap();
    let mut prepare = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::PrepareHandyPlayback {
            program: HandyProgramSelection {
                setup_request_id: setup.request_id,
                record_id: history.record_id,
                qualification: history.qualification,
                engine_epoch: history.engine_epoch,
                device_session: history.device_session,
                hosting_request_id: history.hosting_request_id,
            },
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    prepare.request_id = RequestId::new("liveness-target-preparation").unwrap();
    let preparation = playback_preparation_report(playback_preparation_call(f, &prepare).unwrap());
    let mut acquire = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::AcquireHandyController {
            preparation_request_id: preparation.request_id,
            preparation_id: preparation.preparation_id,
            duration_ms: 1000,
            allow_arm: true,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    acquire.request_id = RequestId::new("liveness-target-acquisition").unwrap();
    let lease = controller_report(controller_call(f, &acquire, 1_000_000_030).unwrap()).lease;
    (lease, credentials)
}

#[test]
fn handy_reconciliation_allows_unrelated_playing_renewal_but_retired_target_cannot_revive() {
    let x = handy_play_fixture();
    let (target, credentials) = reconciliation_other_controller(&x.f, &x.lease);
    assert_ne!(target.device_id, x.lease.device_id);
    let server = handy_play_server(x.listener.try_clone().unwrap(), true);
    handy_play_call(&x, Arc::new(|| Ok(1_000_000_040))).unwrap();
    handy_execution_wait(|| {
        handy_play_status(&x)
            .history
            .is_some_and(|h| h.state == HandyPlaybackOperationState::Acknowledged)
    });
    assert_eq!(
        x.runtime.lock().unwrap().session().state(),
        DeviceState::Playing
    );

    let admission = x.f.engine.handy_reconciliation_admission.write().unwrap();
    let retirement =
        x.f.engine
            .retire_handy_for_reconciliation(&target.device_id, &credentials)
            .unwrap();
    assert_eq!(retirement.ids(), vec![target.preparation_id.clone()]);
    let renew = controller_renew_request(&x.f, &x.lease, "unrelated-renewal-during-recovery");
    // The admission check and actual renewal handler both run while exclusive
    // recovery admission is held for a distinct device and credential route.
    assert!(x
        .f
        .engine
        .handy_reconciliation_admission(&renew.command)
        .unwrap()
        .is_none());
    let renewed = controller_report(controller_call(&x.f, &renew, 1_050_000_000).unwrap()).lease;
    assert_eq!(renewed.state, HandyControllerState::Playing);
    assert_eq!(renewed.generation, x.lease.generation + 1);
    assert!(renewed.deadline_boottime_ns > x.lease.deadline_boottime_ns);
    assert_eq!(
        x.runtime.lock().unwrap().session().state(),
        DeviceState::Playing
    );

    let selected_renew = controller_renew_request(&x.f, &target, "retired-target-renewal");
    assert!(x
        .f
        .engine
        .handy_reconciliation_admission(&selected_renew.command)
        .unwrap()
        .is_none());
    assert_eq!(
        controller_call(&x.f, &selected_renew, 1_050_000_001)
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    let selected = controller_report(
        controller_call(
            &x.f,
            &controller_status_request(&x.f, &target),
            1_050_000_002,
        )
        .unwrap(),
    )
    .lease;
    assert_eq!(selected.state, HandyControllerState::Retired);
    assert!(!selected.armed && !selected.playback_enabled);
    assert_eq!(
        x.runtime.lock().unwrap().session().state(),
        DeviceState::Playing
    );
    drop(admission);
    x.f.engine
        .retire_handy_execution_for_test(&x.lease.preparation_id);
    assert_eq!(server.join().unwrap().len(), 2);
    handy_execution_wait(|| !x.pending.load(Ordering::Acquire));
    x.f.engine.finish_shutdown().unwrap();
}

#[test]
fn handy_reconciliation_timing_refresh_remains_available_but_retirement_fences_installation() {
    let f = fixture();
    let (combined, lease, _) = playback_clock_inputs(&f);
    let mut server = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::SampleHandyServerClock {
            acquisition_request_id: lease.acquisition_request_id.clone(),
            controller_id: lease.controller_id.clone(),
            expected_generation: lease.generation,
            allow_online_sampling: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    server.request_id = RequestId::new("server-refresh-during-recovery").unwrap();
    let mut timing = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::InspectHandyDeviceTiming {
            acquisition_request_id: lease.acquisition_request_id.clone(),
            controller_id: lease.controller_id.clone(),
            expected_generation: lease.generation,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    timing.request_id = RequestId::new("device-read-during-recovery").unwrap();
    let admission = f.engine.handy_reconciliation_admission.write().unwrap();
    for command in [&server.command, &timing.command, &combined.command] {
        assert!(f
            .engine
            .handy_reconciliation_admission(command)
            .unwrap()
            .is_none());
    }
    assert!(
        server_clock_report(
            server_clock_call(&f, &server, 1_000_000_020, server_clock_sample).unwrap()
        )
        .current
    );
    f.engine
        .handy_device_timing_with(
            &timing,
            &f.session,
            |_, _, clock| qualified_device_read(clock),
            || Ok(1_000_000_021),
        )
        .unwrap();
    assert!(
        playback_clock_report(playback_clock_call(&f, &combined, 1_000_000_022).unwrap()).current
    );
    let (_, credentials) = f
        .engine
        .handy_sessions
        .lock()
        .unwrap()
        .capture(&f.session, &lease.device_session)
        .unwrap();
    let retirement = f
        .engine
        .retire_handy_for_reconciliation(&lease.device_id, &credentials)
        .unwrap();
    assert!(retirement.ids().contains(&lease.preparation_id));
    assert_eq!(
        f.engine
            .handy_preparations
            .lock()
            .unwrap()
            .native_sync_counts()
            .1,
        0
    );
    server.request_id = RequestId::new("server-read-after-target-retired").unwrap();
    assert!(server_clock_call(&f, &server, 1_000_000_023, |_| panic!(
        "retired controller cannot resample"
    ))
    .is_err());
    timing.request_id = RequestId::new("device-read-after-target-retired").unwrap();
    assert!(f
        .engine
        .handy_device_timing_with(
            &timing,
            &f.session,
            |_, _, _| panic!("retired controller cannot inspect"),
            || Ok(1_000_000_024)
        )
        .is_err());
    let mut new_combined = combined.clone();
    new_combined.request_id = RequestId::new("combined-clock-after-target-retired").unwrap();
    assert!(playback_clock_call(&f, &new_combined, 1_000_000_025).is_err());
    assert!(
        !playback_clock_report(playback_clock_call(&f, &combined, 1_000_000_026).unwrap()).current
    );
    assert_eq!(
        controller_report(
            controller_call(&f, &controller_status_request(&f, &lease), 1_000_000_027).unwrap()
        )
        .lease
        .state,
        HandyControllerState::Retired
    );
    drop(admission);
    f.engine.finish_shutdown().unwrap();
}

#[test]
fn handy_reconciliation_retirement_during_busy_renewal_cannot_rearm_selected_controller() {
    let f = fixture();
    let (acquire, _) = controller_inputs(&f, 1000);
    let lease = controller_report(controller_call(&f, &acquire, 1_000_000_000).unwrap()).lease;
    let (_, credentials) = f
        .engine
        .handy_sessions
        .lock()
        .unwrap()
        .capture(&f.session, &lease.device_session)
        .unwrap();
    let renewal = controller_renew_request(&f, &lease, "renewal-racing-target-retirement");
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(request_fingerprint(&renewal).unwrap())
    );
    let admission = f.engine.handy_reconciliation_admission.write().unwrap();
    assert!(f
        .engine
        .handy_reconciliation_admission(&renewal.command)
        .unwrap()
        .is_none());
    let writer = Connection::open(f.engine.config.state_dir.join("projects.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (reached, entered) = std::sync::mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let task = scope.spawn(|| {
            let mut clocks = 0;
            f.engine.handy_controller_with(
                &renewal,
                &f.session,
                &fingerprint,
                |_, _| panic!("renewal cannot fetch a script"),
                |_, _| panic!("renewal cannot inspect a device"),
                || {
                    clocks += 1;
                    if clocks == 3 {
                        reached.send(()).unwrap();
                    }
                    Ok(1_050_000_000)
                },
            )
        });
        // The third clock read follows the live-controller checks and precedes
        // the receipt write. SQLite refuses promotion of that read transaction
        // while our external writer remains held. Native retirement must survive
        // the failed renewal, including a later retry of the same request.
        entered
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let retirement = f
            .engine
            .retire_handy_for_reconciliation(&lease.device_id, &credentials)
            .unwrap();
        assert_eq!(retirement.ids(), vec![lease.preparation_id.clone()]);
        let error = task.join().unwrap().unwrap_err();
        assert_eq!(error.code, ErrorCode::Internal);
        assert_eq!(error.message, "database is locked");
        writer.execute_batch("COMMIT").unwrap();
    });
    assert_eq!(
        controller_call(&f, &renewal, 1_050_000_001)
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    let status = controller_report(
        controller_call(&f, &controller_status_request(&f, &lease), 1_050_000_001).unwrap(),
    );
    assert_eq!(status.lease.state, HandyControllerState::Retired);
    assert!(!status.lease.armed && !status.lease.playback_enabled);
    drop(admission);
    f.engine.finish_shutdown().unwrap();
}
