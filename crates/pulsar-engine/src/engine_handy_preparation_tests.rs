fn playback_preparation_inputs(f: &Fixture) -> Request {
    let (mut r, _) = program_correlation_inputs(f);
    let Command::InspectHandyProgram {
        setup_request_id,
        record_id,
        qualification,
        engine_epoch,
        device_session,
        hosting_request_id,
        owner_proof,
        ..
    } = r.command
    else {
        unreachable!()
    };
    r.command = Command::PrepareHandyPlayback {
        program: HandyProgramSelection {
            setup_request_id,
            record_id,
            qualification,
            engine_epoch,
            device_session,
            hosting_request_id,
        },
        allow_online_inspection: true,
        owner_proof,
    };
    r.request_id = RequestId::new("prepare-program").unwrap();
    r
}
fn playback_preparation_call(f: &Fixture, r: &Request) -> PResult<ResponseBody> {
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine.handy_preparation_with(
        r,
        &f.session,
        &fingerprint,
        |h, _| Ok(h.downloaded.clone()),
        |_, _| Ok(program_device_sample()),
    )
}
fn playback_preparation_report(body: ResponseBody) -> HandyPlaybackPreparation {
    let ResponseBody::HandyPlaybackPreparation(value) = body else {
        panic!("preparation")
    };
    value.validate().unwrap();
    *value
}
fn preparation_status_request(f: &Fixture, original: &RequestId) -> Request {
    request(
        &f.session,
        Some(&f.project),
        None,
        Command::HandyPlaybackPreparationStatus {
            preparation_request_id: original.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
fn preparation_retire_request(f: &Fixture, value: &HandyPlaybackPreparation) -> Request {
    request(
        &f.session,
        Some(&f.project),
        None,
        Command::RetireHandyPlaybackPreparation {
            preparation_request_id: value.request_id.clone(),
            preparation_id: value.preparation_id.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
#[test]
fn handy_preparation_retains_native_disarmed_state_and_exact_replay_cannot_rebuild_retired_state() {
    let f = fixture();
    let r = playback_preparation_inputs(&f);
    let saved = playback_preparation_report(playback_preparation_call(&f, &r).unwrap());
    assert_eq!(saved.state, HandyPlaybackPreparationState::Prepared);
    assert!(!saved.armed && !saved.playback_enabled && !saved.physical_stop_pending);
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 1);
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&r).unwrap()));
    let repeated = playback_preparation_report(
        f.engine
            .handy_preparation_with(
                &r,
                &f.session,
                &fingerprint,
                |_, _| panic!("no repeated GET"),
                |_, _| panic!("no repeated observation"),
            )
            .unwrap(),
    );
    assert_eq!(encode(&saved).unwrap(), encode(&repeated).unwrap());
    let retirement = preparation_retire_request(&f, &saved);
    f.engine.handle(retirement.clone()).result.unwrap();
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
    let retired = playback_preparation_report(
        f.engine
            .handy_preparation_with(
                &r,
                &f.session,
                &fingerprint,
                |_, _| panic!(),
                |_, _| panic!(),
            )
            .unwrap(),
    );
    assert_eq!(retired.state, HandyPlaybackPreparationState::Retired);
    let mut next = r.clone();
    next.request_id = RequestId::new("replacement-preparation").unwrap();
    let next = playback_preparation_report(playback_preparation_call(&f, &next).unwrap());
    assert_ne!(next.preparation_id, saved.preparation_id);
    f.engine.handle(retirement).result.unwrap();
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 1);
}
#[test]
fn handy_preparation_storage_failure_installs_no_native_state_and_restart_never_reconstructs_it() {
    let f = fixture();
    let r = playback_preparation_inputs(&f);
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER fail_preparation BEFORE INSERT ON requests WHEN NEW.request='prepare-program' BEGIN SELECT RAISE(ABORT,'fixture preparation failure'); END;").unwrap();
    assert!(playback_preparation_call(&f, &r).is_err());
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_preparation")
        .unwrap();
    playback_preparation_call(&f, &r).unwrap();
    let Fixture { engine, dir, .. } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    let result = playback_preparation_report(engine.handle(r).result.unwrap());
    assert_eq!(result.state, HandyPlaybackPreparationState::Retired);
    assert_eq!(engine.handy_preparations.lock().unwrap().len(), 0);
    drop(engine);
    drop(dir);
}
#[test]
fn handy_preparation_all_change_boundaries_retire_before_reply_or_status_query() {
    for event in 0..6 {
        let f = fixture();
        let r = playback_preparation_inputs(&f);
        let saved = playback_preparation_report(playback_preparation_call(&f, &r).unwrap());
        let i = &saved.correlation.inspection;
        let command = match event {
            0 => request(
                &f.session,
                Some(&f.project),
                Some(1),
                commit_program(&f, 1, program(0.3)),
            ),
            1 => request(
                &f.session,
                None,
                None,
                Command::CloseHandySession {
                    engine_epoch: i.engine_epoch.clone(),
                    device_session: i.device_session.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            2 => request(
                &f.session,
                None,
                None,
                Command::DisableHandyRegistration {
                    device_id: i.registration.device_id.clone(),
                    expected_registration_revision: i.registration.revision,
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            3 => qualification_withdraw(
                &f,
                &i.accepted_qualification.as_ref().unwrap().dossier_id,
                i.accepted_qualification.as_ref().unwrap().revision,
            ),
            4 => request(
                &f.session,
                Some(&f.project),
                None,
                Command::ForgetHandyHosting {
                    upload_request_id: i.hosting_request_id.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ),
            _ => request(
                &f.session,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ),
        };
        f.engine.handle(command).result.unwrap();
        assert_eq!(
            f.engine.handy_preparations.lock().unwrap().len(),
            0,
            "event {event}"
        );
    }
}
#[test]
fn handy_preparation_matching_effect_history_preserves_uncertainty_and_retires_an_older_clear_snapshot(
) {
    let f = fixture();
    let r = playback_preparation_inputs(&f);
    let saved = playback_preparation_report(playback_preparation_call(&f, &r).unwrap());
    let record = crate::device_effect_journal::EffectRecord {
        boot: saved.correlation.inspection.engine_epoch.clone(),
        device_session: saved.correlation.inspection.device_session.clone(),
        sequence: 1,
        admission: saved.admission.clone(),
        controller: None,
        qualification_record: saved
            .correlation
            .inspection
            .accepted_qualification
            .as_ref()
            .unwrap()
            .dossier_id
            .clone(),
        stop: false,
        request_sha256: "a".repeat(64),
    };
    let raw = serde_json::to_string(&record).unwrap();
    let id = format!("{:x}", Sha256::digest(raw.as_bytes()));
    let db = Connection::open(
        f.engine
            .config
            .state_dir
            .join("device-effects/journal.sqlite3"),
    )
    .unwrap();
    db.execute(
        "INSERT INTO effects VALUES(?1,?2,?3,'unknown')",
        params![id, record.admission.configuration.device.as_str(), raw],
    )
    .unwrap();
    drop(db);
    let value = playback_preparation_report(
        f.engine
            .handle(preparation_status_request(&f, &r.request_id))
            .result
            .unwrap(),
    );
    assert_eq!(value.state, HandyPlaybackPreparationState::Retired);
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
    let mut next = r;
    next.request_id = RequestId::new("uncertain-preparation").unwrap();
    let value = playback_preparation_report(playback_preparation_call(&f, &next).unwrap());
    assert!(value.physical_stop_pending);
    assert!(!value.armed && !value.playback_enabled);
}
#[test]
fn handy_preparation_checks_consent_current_selection_and_duplicate_device_ownership() {
    let f = fixture();
    let r = playback_preparation_inputs(&f);
    let mut denied = r.clone();
    if let Command::PrepareHandyPlayback {
        allow_online_inspection,
        ..
    } = &mut denied.command
    {
        *allow_online_inspection = false;
    }
    assert!(playback_preparation_call(&f, &denied).is_err());
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
    playback_preparation_call(&f, &r).unwrap();
    let mut other = r;
    other.request_id = RequestId::new("duplicate-device-preparation").unwrap();
    assert_eq!(
        playback_preparation_call(&f, &other).unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 1);
}
