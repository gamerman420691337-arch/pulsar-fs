fn qualification_context(f: &Fixture) -> HandyQualificationContext {
    let ResponseBody::HandyQualificationContext(c) = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::HandyQualificationContext {
                transport: HandyQualificationTransport::Hssp,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("context")
    };
    c.validate().unwrap();
    *c
}
fn qualification_input(
    f: &Fixture,
    origin: HandyEvidenceOrigin,
) -> (PathBuf, HandyQualificationInput) {
    let registration = handy_session_registered(f);
    let context = qualification_context(f);
    assert_ne!(
        context.control_image.sha256, f.engine.binary_identity.sha256,
        "control image must not be the analysis worker"
    );
    let profile = HandyHsspQualificationProfile {
        dynamics: DynamicsProfile {
            axes: vec![AxisDynamics {
                axis: Axis::Stroke,
                unit: "mm".into(),
                minimum: 0.0,
                maximum: 100.0,
                max_velocity: 1e6,
                max_acceleration: 1e6,
                max_jerk: 1e6,
            }],
            coupled: vec![],
            minimum_gain: 0.0,
        },
        minimum_segment_ms: 10,
        maximum_step_percent: 20,
        minimum_position_percent: 10,
        maximum_position_percent: 90,
        maximum_command_age_ns: 1_000_000,
        maximum_clock_uncertainty_ns: 1_000_000,
        drift_ppm: 10,
        stopping_bound_ns: Some(1_000_000),
    };
    let mut evidence = Vec::new();
    for kind in HandyEvidenceKind::ALL {
        let path = f.dir.path().join(format!("{kind:?}.bin"));
        let bytes = format!("SYNTHETIC TEST FIXTURE: {kind:?}; not physical evidence").into_bytes();
        fs::write(&path, &bytes).unwrap();
        evidence.push(HandyQualificationInputEvidence {
            kind,
            path,
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            byte_len: bytes.len() as u64,
        });
    }
    let input = HandyQualificationInput {
        subject: HandyQualificationSubject {
            schema_version: 2,
            operative_settings: Some(setup_sample().operative_settings),
            server_clock: None,
            device_clock: None,
            recovery: None,
            continuous_timing: None,
            hsp: None,
            origin,
            unit_label: "synthetic test unit; no physical qualification".into(),
            device_id: registration.device_id,
            registration_revision: registration.revision,
            registration_binding_sha256: registration.binding_sha256,
            identity: handy_session_observation().identity.unwrap(),
            context,
            profile,
        },
        evidence,
    };
    let path = f.dir.path().join("qualification.json");
    (path, input)
}
fn qualification_import(
    f: &Fixture,
    id: &str,
    path: &Path,
    input: &HandyQualificationInput,
) -> Request {
    let bytes = serde_json::to_vec(input).unwrap();
    fs::write(path, &bytes).unwrap();
    let mut r = request(
        &f.session,
        None,
        None,
        Command::ImportHandyQualification {
            manifest_path: path.into(),
            manifest_sha256: format!("{:x}", Sha256::digest(bytes)),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
fn qualification_saved(r: ResponseBody) -> HandyQualificationReport {
    let ResponseBody::HandyQualificationSaved(receipt) = r else {
        panic!("saved qualification")
    };
    receipt.validate().unwrap();
    receipt.report
}
fn qualification_status(f: &Fixture, id: &ArtifactId) -> PResult<ResponseBody> {
    f.engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::HandyQualificationStatus {
                dossier_id: id.clone(),
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        ))
        .result
}
fn qualification_withdraw(f: &Fixture, id: &ArtifactId, revision: u64) -> Request {
    request(
        &f.session,
        None,
        None,
        Command::WithdrawHandyQualification {
            dossier_id: id.clone(),
            expected_qualification_revision: revision,
            note: "withdraw synthetic test review".into(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
fn qualification_accept(
    f: &Fixture,
    id: &ArtifactId,
    connection: &HandyConnectionSession,
) -> Request {
    request(
        &f.session,
        None,
        None,
        Command::AcceptHandyQualification {
            dossier_id: id.clone(),
            expected_qualification_revision: 1,
            engine_epoch: connection.engine_epoch.clone(),
            device_session: connection.device_session.clone(),
            note: "simulated operator decision in a unit fixture; not actual acceptance".into(),
            acknowledge_physical_evidence: true,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
fn qualification_call<F>(f: &Fixture, r: &Request, inspect: F) -> PResult<ResponseBody>
where
    F: FnOnce(Arc<crate::handy_credentials::Credentials>) -> PResult<HandySetupDeviceObservation>,
{
    let hash = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine
        .handy_qualification_with(r, &f.session, &hash, inspect)
}
#[test]
fn handy_qualification_import_retains_bytes_and_history_without_acceptance() {
    let f = fixture();
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Synthetic);
    let r = qualification_import(&f, "import", &path, &input);
    let first = qualification_saved(f.engine.handle(r.clone()).result.unwrap());
    assert_eq!(first.state, HandyQualificationState::Pending);
    for e in &input.evidence {
        fs::remove_file(&e.path).unwrap();
    }
    fs::remove_file(path).unwrap();
    assert!(qualification_status(&f, &first.dossier_id).is_ok());
    assert_eq!(
        qualification_saved(f.engine.handle(r.clone()).result.unwrap()).revision,
        1
    );
    let withdrawn = qualification_saved(
        f.engine
            .handle(qualification_withdraw(&f, &first.dossier_id, 1))
            .result
            .unwrap(),
    );
    assert_eq!(withdrawn.state, HandyQualificationState::Withdrawn);
    assert_eq!(
        qualification_saved(f.engine.handle(r).result.unwrap()).state,
        HandyQualificationState::Pending
    );
    let ResponseBody::HandyQualification(current) =
        qualification_status(&f, &first.dossier_id).unwrap()
    else {
        panic!()
    };
    assert_eq!(current.state, HandyQualificationState::Withdrawn);
}
#[test]
fn handy_qualification_synthetic_missing_roles_and_changed_context_refuse() {
    let f = fixture();
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Synthetic);
    let mut invalid = input.clone();
    invalid.evidence.pop();
    assert!(f
        .engine
        .handle(qualification_import(&f, "missing", &path, &invalid))
        .result
        .is_err());
    invalid = input.clone();
    invalid.subject.context.control_image.sha256 = "f".repeat(64);
    invalid.subject.context.control_image.artifact_id = ArtifactId::new("f".repeat(64)).unwrap();
    assert_eq!(
        f.engine
            .handle(qualification_import(&f, "changed-driver", &path, &invalid))
            .result
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    let saved = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "import", &path, &input))
            .result
            .unwrap(),
    );
    let connection = HandyConnectionSession {
        engine_epoch: DeviceSessionId::new(f.engine.engine_epoch.clone()).unwrap(),
        device_session: DeviceSessionId::new("missing").unwrap(),
        open_request_id: RequestId::new("open").unwrap(),
        registration: handy_registration_catalog(&f).registrations.remove(0),
        state: HandyConnectionState::Observed,
        inspection: Some(handy_session_observation()),
    };
    let accept = qualification_accept(&f, &saved.dossier_id, &connection);
    assert_eq!(
        qualification_call(&f, &accept, |_| panic!("synthetic must not inspect"))
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}
#[test]
fn handy_qualification_simulated_review_and_withdrawal_are_atomic_and_replay_is_historical() {
    let f = fixture();
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
    let saved = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "import", &path, &input))
            .result
            .unwrap(),
    );
    let reg = handy_registration_catalog(&f).registrations.remove(0);
    let connection = handy_session_report(
        handy_session_call(&f, &handy_session_open_request(&f, "open", &reg), |_| {
            Ok(handy_session_observation())
        })
        .unwrap(),
    );
    let accept = qualification_accept(&f, &saved.dossier_id, &connection);
    let accepted =
        qualification_saved(qualification_call(&f, &accept, |_| Ok(setup_sample())).unwrap());
    assert_eq!(accepted.state, HandyQualificationState::Accepted);
    assert_eq!(accepted.revision, 2);
    let withdraw = qualification_withdraw(&f, &saved.dossier_id, 2);
    let withdrawn = qualification_saved(f.engine.handle(withdraw.clone()).result.unwrap());
    assert_eq!(withdrawn.revision, 3);
    assert_eq!(withdrawn.state, HandyQualificationState::Withdrawn);
    assert_eq!(
        qualification_saved(
            qualification_call(&f, &accept, |_| panic!("no replay inspection")).unwrap()
        )
        .revision,
        2
    );
    assert_eq!(
        qualification_saved(f.engine.handle(withdraw).result.unwrap()).revision,
        3
    );
    let ResponseBody::HandyQualification(current) =
        qualification_status(&f, &saved.dossier_id).unwrap()
    else {
        panic!()
    };
    assert_eq!(current.state, HandyQualificationState::Withdrawn);
}
#[test]
fn handy_qualification_late_acceptance_cannot_undo_withdrawal_or_registration_change() {
    for withdraw in [true, false] {
        let f = fixture();
        let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
        let saved = qualification_saved(
            f.engine
                .handle(qualification_import(&f, "import", &path, &input))
                .result
                .unwrap(),
        );
        let reg = handy_registration_catalog(&f).registrations.remove(0);
        let connection = handy_session_report(
            handy_session_call(&f, &handy_session_open_request(&f, "open", &reg), |_| {
                Ok(handy_session_observation())
            })
            .unwrap(),
        );
        let accept = qualification_accept(&f, &saved.dossier_id, &connection);
        assert!(qualification_call(&f, &accept, |_| {
            if withdraw {
                f.engine
                    .handle(qualification_withdraw(&f, &saved.dossier_id, 1))
                    .result
                    .unwrap();
            } else {
                f.engine
                    .handle(handy_registration_request(
                        &f,
                        "changed",
                        1,
                        "changed registration",
                    ))
                    .result
                    .unwrap();
            }
            Ok(setup_sample())
        })
        .is_err());
        let ResponseBody::HandyQualification(current) =
            qualification_status(&f, &saved.dossier_id).unwrap()
        else {
            panic!()
        };
        assert_ne!(current.state, HandyQualificationState::Accepted);
    }
}
#[test]
fn handy_qualification_corrupt_evidence_refuses_use_but_does_not_block_withdrawal() {
    let f = fixture();
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Synthetic);
    let saved = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "import", &path, &input))
            .result
            .unwrap(),
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE handy_qualification_evidence SET data=X'00' WHERE dossier=?1",
            [saved.dossier_id.as_str()],
        )
        .unwrap();
    assert!(qualification_status(&f, &saved.dossier_id).is_err());
    assert_eq!(
        qualification_saved(
            f.engine
                .handle(qualification_withdraw(&f, &saved.dossier_id, 1))
                .result
                .unwrap()
        )
        .state,
        HandyQualificationState::Withdrawn
    );
}
#[test]
fn handy_qualification_receipt_failure_rolls_back_import_and_decision_tampering_refuses() {
    let f = fixture();
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
    let original = qualification_import(&f, "import-failure", &path, &input);
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER fixture_qualification BEFORE INSERT ON requests WHEN NEW.request='import-failure' BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END;").unwrap();
    assert!(f.engine.handle(original.clone()).result.is_err());
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name LIKE 'handy_qualification%'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        0
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fixture_qualification")
        .unwrap();
    let saved = qualification_saved(f.engine.handle(original).result.unwrap());
    let withdrawn = qualification_saved(
        f.engine
            .handle(qualification_withdraw(&f, &saved.dossier_id, 1))
            .result
            .unwrap(),
    );
    let mut tampered = withdrawn;
    tampered.state = HandyQualificationState::Accepted;
    tampered.decision.as_mut().unwrap().state = HandyQualificationState::Accepted;
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE handy_qualifications SET report=?1 WHERE id=?2",
            params![encode(&tampered).unwrap(), saved.dossier_id.as_str()],
        )
        .unwrap();
    assert!(qualification_status(&f, &saved.dossier_id).is_err());
}
#[test]
fn handy_qualification_restart_preserves_review_and_unknown_schema_refuses_without_db_change() {
    let f = fixture();
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Synthetic);
    let saved = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "import", &path, &input))
            .result
            .unwrap(),
    );
    let withdraw = qualification_withdraw(&f, &saved.dossier_id, 1);
    f.engine.handle(withdraw.clone()).result.unwrap();
    let Fixture {
        engine,
        session,
        project,
        dir,
    } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    let f = Fixture {
        engine,
        session,
        project,
        dir,
    };
    assert_eq!(
        qualification_saved(f.engine.handle(withdraw).result.unwrap()).state,
        HandyQualificationState::Withdrawn
    );
    let Fixture { engine, dir, .. } = f;
    let config = Engine::retire_for_test(engine);
    let db_path = config.state_dir.join("projects.sqlite3");
    {
        let db = Connection::open(&db_path).unwrap();
        db.execute_batch(
            "PRAGMA ignore_check_constraints=ON; UPDATE handy_qualification_schema SET version=2;",
        )
        .unwrap();
    }
    let before = fs::read(&db_path).unwrap();
    assert!(Engine::open(config).is_err());
    assert_eq!(fs::read(db_path).unwrap(), before);
    drop(dir);
}

#[test]
fn handy_qualification_selected_inspection_checks_current_acceptance_and_late_withdrawal() {
    let f = fixture();
    let (mut inspection, connection, _) = setup_inputs(&f);
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
    let imported = qualification_saved(
        f.engine
            .handle(qualification_import(
                &f,
                "qualification-import",
                &path,
                &input,
            ))
            .result
            .unwrap(),
    );
    let accept = qualification_accept(&f, &imported.dossier_id, &connection);
    let accepted =
        qualification_saved(qualification_call(&f, &accept, |_| Ok(setup_sample())).unwrap());
    let binding = HandyQualificationBinding {
        dossier_id: accepted.dossier_id.clone(),
        revision: accepted.revision,
    };
    if let Command::InspectHandySetup { qualification, .. } = &mut inspection.command {
        *qualification = Some(binding.clone());
    }
    let ResponseBody::HandySetupInspection(report) = f
        .engine
        .inspect_handy_setup_with(
            &inspection,
            &f.session,
            |h, _| Ok(h.downloaded.clone()),
            |_, _| Ok(setup_sample()),
        )
        .unwrap()
    else {
        panic!("inspection")
    };
    assert_eq!(report.accepted_qualification, Some(binding));
    assert!(!report.qualified && !report.setup_performed);
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(
                &inspection,
                &f.session,
                |h, _| {
                    f.engine
                        .handle(qualification_withdraw(
                            &f,
                            &accepted.dossier_id,
                            accepted.revision,
                        ))
                        .result
                        .unwrap();
                    Ok(h.downloaded.clone())
                },
                |_, _| Ok(setup_sample())
            )
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        f.engine
            .inspect_handy_setup_with(
                &inspection,
                &f.session,
                |_, _| panic!("withdrawn evidence cannot read hosted bytes"),
                |_, _| panic!("withdrawn evidence cannot inspect device")
            )
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn handy_qualification_selected_script_outside_sample_limits_refuses_before_network() {
    for limit in 0..3 {
        let f = fixture();
        let (mut inspection, connection, _) = setup_inputs(&f);
        let (path, mut input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
        match limit {
            0 => input.subject.profile.minimum_position_percent = 21,
            1 => input.subject.profile.maximum_position_percent = 19,
            _ => input.subject.profile.minimum_segment_ms = 1001,
        }
        let imported = qualification_saved(
            f.engine
                .handle(qualification_import(&f, "envelope-import", &path, &input))
                .result
                .unwrap(),
        );
        let accepted = qualification_saved(
            qualification_call(
                &f,
                &qualification_accept(&f, &imported.dossier_id, &connection),
                |_| Ok(setup_sample()),
            )
            .unwrap(),
        );
        if let Command::InspectHandySetup { qualification, .. } = &mut inspection.command {
            *qualification = Some(HandyQualificationBinding {
                dossier_id: accepted.dossier_id,
                revision: accepted.revision,
            });
        }
        let error = f
            .engine
            .inspect_handy_setup_with(
                &inspection,
                &f.session,
                |_, _| panic!("out-of-envelope script cannot fetch hosted bytes"),
                |_, _| panic!("out-of-envelope script cannot inspect device"),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert!(error.message.contains("accepted sample"));
    }
}
#[test]
fn handy_qualification_hosted_source_identity_cannot_be_rebound_in_history() {
    let f = fixture();
    let (inspection, _, mut hosted) = setup_inputs(&f);
    hosted.script.sha256 = "a".repeat(64);
    hosted.script.artifact_id = ArtifactId::new(hosted.script.sha256.clone()).unwrap();
    hosted.link_available = false;
    hosted.upload_in_flight = false;
    hosted.validate().unwrap();
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
            params![
                encode(&ResponseBody::HandyHosting(Box::new(hosted.clone()))).unwrap(),
                f.session.as_str(),
                hosted.request_id.as_str()
            ],
        )
        .unwrap();
    let error = f
        .engine
        .inspect_handy_setup_with(
            &inspection,
            &f.session,
            |_, _| panic!("rebound script cannot fetch bytes"),
            |_, _| panic!("rebound script cannot inspect device"),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
}

#[test]
fn handy_qualification_acceptance_requires_reviewed_settings_and_fresh_exact_match() {
    for legacy in [true, false] {
        let f = fixture();
        let (_, connection, _) = setup_inputs(&f);
        let (path, mut input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
        if legacy {
            input.subject.schema_version = 1;
            input.subject.operative_settings = None;
        }
        let imported = qualification_saved(
            f.engine
                .handle(qualification_import(&f, "settings-import", &path, &input))
                .result
                .unwrap(),
        );
        let accept = qualification_accept(&f, &imported.dossier_id, &connection);
        if legacy {
            let error =
                qualification_call(&f, &accept, |_| panic!("legacy evidence must not inspect"))
                    .unwrap_err();
            assert_eq!(error.code, ErrorCode::Forbidden);
            assert!(error.message.contains("operative settings"));
        } else {
            let error = qualification_call(&f, &accept, |_| {
                let mut sample = setup_sample();
                sample.operative_settings.slider.overclock_enabled = true;
                Ok(sample)
            })
            .unwrap_err();
            assert_eq!(error.code, ErrorCode::DependencyMismatch);
            assert!(error.message.contains("operative settings"));
            let ResponseBody::HandyQualification(current) =
                qualification_status(&f, &imported.dossier_id).unwrap()
            else {
                panic!()
            };
            assert_eq!(current.state, HandyQualificationState::Pending);
            let accepted = qualification_saved(
                qualification_call(&f, &accept, |_| Ok(setup_sample())).unwrap(),
            );
            assert_eq!(accepted.state, HandyQualificationState::Accepted);
        }
    }
}
#[test]
fn handy_qualification_settings_mismatch_and_mid_inspection_change_refuse() {
    let f = fixture();
    let (mut request, connection, _) = setup_inputs(&f);
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
    let imported = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "settings-import", &path, &input))
            .result
            .unwrap(),
    );
    let accepted = qualification_saved(
        qualification_call(
            &f,
            &qualification_accept(&f, &imported.dossier_id, &connection),
            |_| Ok(setup_sample()),
        )
        .unwrap(),
    );
    if let Command::InspectHandySetup { qualification, .. } = &mut request.command {
        *qualification = Some(HandyQualificationBinding {
            dossier_id: accepted.dossier_id,
            revision: accepted.revision,
        });
    }
    let error = f
        .engine
        .inspect_handy_setup_with(
            &request,
            &f.session,
            |_, _| panic!("settings mismatch must not fetch hosted bytes"),
            |_, _| {
                let mut sample = setup_sample();
                sample.operative_settings.stroke.min = 0.1;
                Ok(sample)
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DependencyMismatch);
    let mut samples = 0;
    let error = f
        .engine
        .inspect_handy_setup_with(
            &request,
            &f.session,
            |hosted, _| Ok(hosted.downloaded.clone()),
            |_, _| {
                samples += 1;
                let mut sample = setup_sample();
                if samples == 2 {
                    sample.operative_settings.slider.x_inverse_motor = true;
                }
                Ok(sample)
            },
        )
        .unwrap_err();
    assert_eq!(samples, 2);
    assert_eq!(error.code, ErrorCode::DependencyMismatch);
    assert!(error.message.contains("operative settings"));
}

#[test]
fn handy_recovery_qualification_requires_new_contract_current_acceptance_and_exact_evidence() {
    for scenario in 0..5 {
        let f = fixture();
        let (path, mut input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
        if scenario != 0 {
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
                maximum_sample_span_ns: 1_000_000,
            });
        }
        let saved = qualification_saved(
            f.engine
                .handle(qualification_import(&f, "import-recovery", &path, &input))
                .result
                .unwrap(),
        );
        let registration = handy_registration_catalog(&f).registrations.remove(0);
        let connection = handy_session_report(
            handy_session_call(
                &f,
                &handy_session_open_request(&f, "open-recovery", &registration),
                |_| Ok(handy_session_observation()),
            )
            .unwrap(),
        );
        let accepted = qualification_saved(
            qualification_call(
                &f,
                &qualification_accept(&f, &saved.dossier_id, &connection),
                |_| Ok(setup_sample()),
            )
            .unwrap(),
        );
        let binding = HandyQualificationBinding {
            dossier_id: accepted.dossier_id.clone(),
            revision: accepted.revision,
        };
        match scenario {
            2 => {
                f.engine
                    .handle(qualification_withdraw(&f, &accepted.dossier_id, 2))
                    .result
                    .unwrap();
            }
            3 => {
                f.engine
                    .db
                    .lock()
                    .unwrap()
                    .execute(
                        "UPDATE handy_qualification_evidence SET data=?1 WHERE dossier=?2",
                        params![
                            b"corrupt test evidence".to_vec(),
                            accepted.dossier_id.as_str()
                        ],
                    )
                    .unwrap();
            }
            _ => {}
        }
        let mut selected_registration = registration;
        if scenario == 4 {
            selected_registration.binding_sha256 = "f".repeat(64);
        }
        let db = f.engine.db.lock().unwrap();
        let result =
            handy_qualification::accepted_recovery_record(&db, &binding, &selected_registration);
        let historical = handy_qualification::historical_recovery_subject(&db, &binding);
        if scenario == 3 {
            assert!(
                historical.is_err(),
                "corrupt history cannot identify the old physical subject"
            );
        } else {
            assert_eq!(historical.unwrap().identity, input.subject.identity);
        }
        if scenario == 1 {
            let verified = result.unwrap();
            assert_eq!(verified.dossier_id, binding.dossier_id);
            assert!(verified.manifest.subject.recovery.is_some());
        } else {
            assert!(result.is_err(), "scenario {scenario}");
        }
    }
}

fn hsp_qualification_input(
    f: &Fixture,
    origin: HandyEvidenceOrigin,
) -> (PathBuf, HandyQualificationInput) {
    let (path, mut input) = qualification_input(f, origin);
    let s = &mut input.subject;
    s.schema_version = 7;
    s.context =
        handy_qualification::context_for_transport(HandyQualificationTransport::Hsp).unwrap();
    s.server_clock = Some(HandyServerClockQualification {
        model: HandyServerClockModel::RequestBracketMonotonic,
        timestamp_error_ns: 1,
    });
    s.device_clock = Some(HandyDeviceClockQualification {
        model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
        user_offset_ms: 0,
        device_error_ns: 1,
        maximum_sync_age_ns: 1_000_000,
        maximum_sync_round_trip_ms: 1,
        drift_ppm: 1,
    });
    s.recovery = Some(HandyRecoveryQualification {
        transport: HandyQualificationTransport::Hsp,
        model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
        remote_state: HandyRecoveryRemoteState::Stopped,
        prior_command_settle_ns: 1_000_000,
        stopped_sample_separation_ns: 1_000_000,
        maximum_sample_span_ns: 1_000_000,
    });
    s.hsp = Some(HandyHspQualificationProfile {
        policy: HandyHspPolicy {
            max_buffered_points: 100,
            device_capacity_points: 100,
            max_chunk_points: 50,
            min_lead_ms: 100,
            max_future_ms: 1000,
            max_sample_age_ns: 1_000_000,
            max_gap_ms: 100,
        },
        playback_rate: 1,
        looping: false,
        pause_on_starving: false,
        append_flush: false,
        sole_controller: true,
        finite_drain: None,
    });
    input.validate().unwrap();
    (path, input)
}
fn hsp_qualification_sample() -> HandySetupDeviceObservation {
    let mut sample = serde_json::to_value(setup_sample()).unwrap();
    sample["mode"]["mode"] = serde_json::json!(4);
    sample["remote_hssp"] = serde_json::Value::Null;
    sample["remote_hsp"] = serde_json::json!({
        "play_state":"stopped", "stream_id":7, "current_time_ms":-1,
        "points":0, "max_points":100, "current_point":-1,
        "playback_rate":0.0, "looping":false, "pause_on_starving":false,
        "first_point_time_ms":0, "last_point_time_ms":0,
        "tail_point_stream_index":-1, "tail_point_stream_index_threshold":0,
    });
    let sample: HandySetupDeviceObservation = serde_json::from_value(sample).unwrap();
    sample.validate().unwrap();
    sample
}

fn finite_hsp_qualification_input(f: &Fixture) -> (PathBuf, HandyQualificationInput) {
    let (path, mut input) = hsp_qualification_input(f, HandyEvidenceOrigin::Synthetic);
    input.subject.hsp.as_mut().unwrap().finite_drain = Some(HandyHspFiniteDrainQualification {
        schema_version: 1,
        model: HandyHspFiniteDrainModel::EarliestPhaseTailThenOwnedStop,
        maximum_drain_duration_ns: 1_000_000_000,
        maximum_terminal_overrun_ns: 4_000_000,
        maximum_stop_dispatch_delay_ns: 2_000_000,
    });
    let bytes = b"AUTHORED finite-tail fixture; not physical qualification or acceptance";
    let evidence_path = f.dir.path().join("finite-tail.bin");
    fs::write(&evidence_path, bytes).unwrap();
    input.evidence.push(HandyQualificationInputEvidence {
        kind: HandyEvidenceKind::FiniteTailDrain,
        path: evidence_path,
        sha256: format!("{:x}", Sha256::digest(bytes)),
        byte_len: bytes.len() as u64,
    });
    input.validate().unwrap();
    (path, input)
}

#[test]
fn hsp_finite_device_resolution_requires_exact_policy_and_no_synthetic_physical_acceptance() {
    let f = fixture();
    let (path, input) = finite_hsp_qualification_input(&f);
    let imported = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "finite-device", &path, &input))
            .result
            .unwrap(),
    );
    let (registration, connection) = hsp_qualification_connection(&f);
    let hash = input
        .subject
        .hsp_finite_drain_policy_sha256()
        .unwrap()
        .unwrap();
    let binding = HandyQualificationBinding {
        dossier_id: imported.dossier_id.clone(),
        revision: 2,
    };
    assert!(handy_qualification::accepted_hsp_finite_drain_policy(
        &f.engine.db.lock().unwrap(),
        &binding,
        &registration,
        &hash
    )
    .is_err());
    let accept = qualification_accept(&f, &imported.dossier_id, &connection);
    assert_eq!(
        qualification_call(&f, &accept, |_| panic!(
            "synthetic evidence must never inspect device"
        ))
        .unwrap_err()
        .code,
        ErrorCode::Forbidden
    );
    let db = f.engine.db.lock().unwrap();
    let expires = crate::handy_clock::boottime_ns()
        .unwrap()
        .checked_add(60_000_000_000)
        .unwrap();
    let permit = handy_qualification::install_authored_hsp_device_acceptance_for_test(
        &db,
        &imported.dossier_id,
        expires,
    )
    .unwrap();
    assert_eq!(permit, binding);
    permit.validate().unwrap();
    let original_scope: (u64, u64, String) = db.query_row(
        "SELECT revision,binding_revision,subject_sha256 FROM temp.pulsar_test_hsp_device_acceptance WHERE dossier=?1",
        [imported.dossier_id.as_str()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).unwrap();
    assert_eq!(original_scope, (1, 2, input.subject.sha256().unwrap()));
    let durable_binding = HandyQualificationBinding {
        dossier_id: permit.dossier_id.clone(),
        revision: 1,
    };
    assert!(durable_binding.validate().is_err());
    assert!(handy_qualification::accepted_hsp_finite_drain_policy(
        &db,
        &durable_binding,
        &registration,
        &hash,
    )
    .is_err());
    let resolved =
        handy_qualification::accepted_hsp_finite_drain_policy(&db, &permit, &registration, &hash)
            .unwrap();
    assert_eq!(resolved.policy_sha256, hash);
    assert_eq!(resolved.policy.maximum_duration_ns, 1_000_000_000);
    assert_eq!(resolved.policy.stop_budget_ns, 3_000_000);
    assert_eq!(resolved.maximum_stop_dispatch_delay_ns, 2_000_000);
    assert_eq!(resolved.stopping_bound_ns, 1_000_000);
    assert!(handy_qualification::accepted_hsp_finite_drain_policy(
        &db,
        &permit,
        &registration,
        &"f".repeat(64)
    )
    .is_err());
    let mut wrong_revision = permit.clone();
    wrong_revision.revision += 1;
    assert!(handy_qualification::accepted_hsp_finite_drain_policy(
        &db,
        &wrong_revision,
        &registration,
        &hash,
    )
    .is_err());
    let mut wrong_registration = registration.clone();
    wrong_registration.binding_sha256 = "e".repeat(64);
    assert!(handy_qualification::accepted_hsp_finite_drain_policy(
        &db,
        &permit,
        &wrong_registration,
        &hash,
    )
    .is_err());
    let raw: String = db
        .query_row(
            "SELECT report FROM handy_qualifications WHERE id=?1",
            [imported.dossier_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    let retained: HandyQualificationReport = decode(&raw).unwrap();
    assert_eq!(retained.state, HandyQualificationState::Pending);
    assert_eq!(
        retained.manifest.subject.origin,
        HandyEvidenceOrigin::Synthetic
    );
    assert_eq!(retained.revision, 1);
    handy_qualification::validate(&db).unwrap();
    drop(db);
    // A test permit cannot grant the public physical-acceptance operation either.
    assert_eq!(
        qualification_call(&f, &accept, |_| panic!("no device inspection"))
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    let db = f.engine.db.lock().unwrap();
    db.execute(
        "UPDATE temp.pulsar_test_hsp_device_acceptance SET subject_sha256=?1",
        ["e".repeat(64)],
    )
    .unwrap();
    assert!(handy_qualification::accepted_hsp_finite_drain_policy(
        &db,
        &permit,
        &registration,
        &hash,
    )
    .is_err());
}

#[test]
fn hsp_device_test_permit_cannot_enable_legacy_drain_or_survive_expiry_withdrawal_restart() {
    for case in 0..4 {
        let f = fixture();
        let (path, input) = if case == 0 {
            hsp_qualification_input(&f, HandyEvidenceOrigin::Synthetic)
        } else {
            finite_hsp_qualification_input(&f)
        };
        let imported = qualification_saved(
            f.engine
                .handle(qualification_import(&f, "permit-scope", &path, &input))
                .result
                .unwrap(),
        );
        let (registration, _) = hsp_qualification_connection(&f);
        let hash = input
            .subject
            .hsp_finite_drain_policy_sha256()
            .unwrap()
            .unwrap_or_else(|| "f".repeat(64));
        let db = f.engine.db.lock().unwrap();
        assert!(
            handy_qualification::install_authored_hsp_device_acceptance_for_test(
                &db,
                &imported.dossier_id,
                0
            )
            .is_err()
        );
        let expires = crate::handy_clock::boottime_ns()
            .unwrap()
            .checked_add(60_000_000_000)
            .unwrap();
        let binding = handy_qualification::install_authored_hsp_device_acceptance_for_test(
            &db,
            &imported.dossier_id,
            expires,
        )
        .unwrap();
        if case == 0 {
            assert!(handy_qualification::accepted_hsp_finite_drain_policy(
                &db,
                &binding,
                &registration,
                &hash
            )
            .is_err());
            continue;
        }
        if case == 1 {
            db.execute(
                "UPDATE temp.pulsar_test_hsp_device_acceptance SET expires_at_ns=0",
                [],
            )
            .unwrap();
            assert!(handy_qualification::accepted_hsp_finite_drain_policy(
                &db,
                &binding,
                &registration,
                &hash
            )
            .is_err());
            continue;
        }
        drop(db);
        if case == 2 {
            f.engine
                .handle(qualification_withdraw(&f, &imported.dossier_id, 1))
                .result
                .unwrap();
            let db = f.engine.db.lock().unwrap();
            assert!(handy_qualification::accepted_hsp_finite_drain_policy(
                &db,
                &binding,
                &registration,
                &hash
            )
            .is_err());
            assert!(
                handy_qualification::install_authored_hsp_device_acceptance_for_test(
                    &db,
                    &imported.dossier_id,
                    expires
                )
                .is_err()
            );
        } else {
            let config = Engine::retire_for_test(f.engine);
            let reopened = Engine::open(config).unwrap();
            let db = reopened.db.lock().unwrap();
            assert!(!db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE name='pulsar_test_hsp_device_acceptance')", [], |r| r.get::<_,bool>(0)).unwrap());
            assert!(handy_qualification::accepted_hsp_finite_drain_policy(
                &db,
                &binding,
                &registration,
                &hash
            )
            .is_err());
        }
    }
}
fn hsp_qualification_connection(f: &Fixture) -> (HandyRegistration, HandyConnectionSession) {
    let registration = handy_registration_catalog(f).registrations.remove(0);
    let connection = handy_session_report(
        handy_session_call(
            f,
            &handy_session_open_request(f, "hsp-open", &registration),
            |_| Ok(handy_session_observation()),
        )
        .unwrap(),
    );
    (registration, connection)
}

#[test]
fn hsp_qualification_context_pins_transport_without_changing_loaded_image_or_platform() {
    let f = fixture();
    let old = qualification_context(&f);
    let ResponseBody::HandyQualificationContext(hsp) = f
        .engine
        .handle(request(
            &f.session,
            None,
            None,
            Command::HandyQualificationContext {
                transport: HandyQualificationTransport::Hsp,
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("HSP qualification context");
    };
    assert_eq!(hsp.control_image, old.control_image);
    assert_eq!(hsp.platform, old.platform);
    assert_eq!(hsp.transport.as_str(), HANDY_HSP_QUALIFICATION_TRANSPORT);
    assert_ne!(hsp.transport, old.transport);
}

#[test]
fn hsp_qualification_synthetic_and_wrong_or_absent_remote_mode_refuse_acceptance() {
    for case in 0..5 {
        let f = fixture();
        let origin = if case == 0 {
            HandyEvidenceOrigin::Synthetic
        } else {
            HandyEvidenceOrigin::Physical
        };
        let (path, input) = hsp_qualification_input(&f, origin);
        let imported = qualification_saved(
            f.engine
                .handle(qualification_import(&f, "hsp-import", &path, &input))
                .result
                .unwrap(),
        );
        let (_, connection) = hsp_qualification_connection(&f);
        let accept = qualification_accept(&f, &imported.dossier_id, &connection);
        let error = qualification_call(&f, &accept, |_| {
            assert_ne!(case, 0, "synthetic evidence must fail before inspection");
            let mut sample = hsp_qualification_sample();
            match case {
                1 => sample = setup_sample(),
                2 => sample.remote_hsp = None,
                3 => sample.identity.firmware.push('x'),
                _ => sample.operative_settings.slider.x_inverse_motor = true,
            }
            Ok(sample)
        })
        .unwrap_err();
        assert_eq!(
            error.code,
            if case == 0 {
                ErrorCode::Forbidden
            } else {
                ErrorCode::DependencyMismatch
            }
        );
        let ResponseBody::HandyQualification(current) =
            qualification_status(&f, &imported.dossier_id).unwrap()
        else {
            panic!();
        };
        assert_eq!(current.state, HandyQualificationState::Pending);
    }
}

#[test]
fn hsp_qualification_has_distinct_accepted_bundle_and_cannot_authorize_hosted_hssp() {
    let f = fixture();
    let (path, input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Physical);
    let imported = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "hsp-import", &path, &input))
            .result
            .unwrap(),
    );
    let (registration, connection) = hsp_qualification_connection(&f);
    // This uses authored unit fixtures and a simulated operator request only.
    let accepted = qualification_saved(
        qualification_call(
            &f,
            &qualification_accept(&f, &imported.dossier_id, &connection),
            |_| Ok(hsp_qualification_sample()),
        )
        .unwrap(),
    );
    let binding = HandyQualificationBinding {
        dossier_id: accepted.dossier_id.clone(),
        revision: accepted.revision,
    };
    let db = f.engine.db.lock().unwrap();
    let bundle = handy_qualification::accepted_hsp_bundle(
        &db,
        &binding,
        &registration,
        &hsp_qualification_sample(),
    )
    .unwrap();
    assert_eq!(bundle.report.dossier_id, binding.dossier_id);
    assert_eq!(
        bundle.policy,
        input
            .subject
            .hsp
            .as_ref()
            .unwrap()
            .policy
            .to_policy()
            .unwrap()
    );
    assert_eq!(bundle.subject_sha256, input.subject.sha256().unwrap());
    assert_eq!(
        bundle.profile_sha256,
        input.subject.hsp_profile_sha256().unwrap()
    );
    let hosted = crate::handy_hosting_wire::fixture_verified(b"0,20\n1000,30\n");
    assert_eq!(
        handy_qualification::accepted_record(
            &db,
            &binding,
            &registration,
            &input.subject.identity,
            &hosted
        )
        .unwrap_err()
        .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        handy_qualification::accepted_timing(
            &db,
            &binding,
            &registration,
            &setup_sample(),
            &hosted
        )
        .unwrap_err()
        .code,
        ErrorCode::Forbidden
    );
    assert_eq!(
        handy_qualification::accepted_recovery_record(&db, &binding, &registration)
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    for case in 0..4 {
        let mut observation = hsp_qualification_sample();
        match case {
            0 => observation = setup_sample(),
            1 => observation.remote_hsp = None,
            2 => observation.identity.firmware.push('x'),
            _ => observation.operative_settings.stroke.min = 0.1,
        }
        assert!(
            handy_qualification::accepted_hsp_bundle(&db, &binding, &registration, &observation)
                .is_err(),
            "case {case}"
        );
    }
}

#[test]
fn hsp_qualification_rechecks_current_decision_evidence_and_registration() {
    for case in 0..4 {
        let f = fixture();
        let (path, input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Physical);
        let imported = qualification_saved(
            f.engine
                .handle(qualification_import(&f, "hsp-import", &path, &input))
                .result
                .unwrap(),
        );
        let (mut registration, connection) = hsp_qualification_connection(&f);
        let accepted = qualification_saved(
            qualification_call(
                &f,
                &qualification_accept(&f, &imported.dossier_id, &connection),
                |_| Ok(hsp_qualification_sample()),
            )
            .unwrap(),
        );
        let mut binding = HandyQualificationBinding {
            dossier_id: accepted.dossier_id.clone(),
            revision: accepted.revision,
        };
        match case {
            0 | 1 => {
                let withdrawn = qualification_saved(
                    f.engine
                        .handle(qualification_withdraw(
                            &f,
                            &binding.dossier_id,
                            binding.revision,
                        ))
                        .result
                        .unwrap(),
                );
                if case == 1 {
                    binding.revision = withdrawn.revision;
                }
            }
            2 => {
                f.engine
                    .db
                    .lock()
                    .unwrap()
                    .execute(
                        "UPDATE handy_qualification_evidence SET data=X'00' WHERE dossier=?1",
                        [binding.dossier_id.as_str()],
                    )
                    .unwrap();
            }
            _ => registration.binding_sha256 = "f".repeat(64),
        }
        let db = f.engine.db.lock().unwrap();
        let error =
            handy_qualification::accepted_hsp_record(&db, &binding, &registration).unwrap_err();
        if case == 0 {
            assert_eq!(error.code, ErrorCode::RevisionConflict);
        }
        if case == 1 {
            assert_eq!(error.code, ErrorCode::Forbidden);
        }
    }
}

#[test]
fn hssp_qualification_cannot_be_reused_as_hsp_and_hsp_review_rechecks_after_inspection() {
    let f = fixture();
    let (path, input) = qualification_input(&f, HandyEvidenceOrigin::Physical);
    let imported = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "hssp-import", &path, &input))
            .result
            .unwrap(),
    );
    let (registration, connection) = hsp_qualification_connection(&f);
    let accepted = qualification_saved(
        qualification_call(
            &f,
            &qualification_accept(&f, &imported.dossier_id, &connection),
            |_| Ok(setup_sample()),
        )
        .unwrap(),
    );
    let binding = HandyQualificationBinding {
        dossier_id: accepted.dossier_id,
        revision: accepted.revision,
    };
    {
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            handy_qualification::accepted_hsp_record(&db, &binding, &registration)
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
    }
    let (path, hsp_input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Physical);
    let hsp_imported = qualification_saved(
        f.engine
            .handle(qualification_import(&f, "hsp-import", &path, &hsp_input))
            .result
            .unwrap(),
    );
    let error = qualification_call(
        &f,
        &qualification_accept(&f, &hsp_imported.dossier_id, &connection),
        |_| {
            f.engine
                .handle(qualification_withdraw(&f, &hsp_imported.dossier_id, 1))
                .result
                .unwrap();
            Ok(hsp_qualification_sample())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
}
