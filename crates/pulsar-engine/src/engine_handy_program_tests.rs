fn program_correlation_inputs(f: &Fixture) -> (Request, HandyQualificationReport) {
    let (setup, qualification) = setup_send_request(f, "original-program-setup");
    let status = setup_status_report(
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
    );
    let h = status.history.unwrap();
    let r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::InspectHandyProgram {
            setup_request_id: setup.request_id,
            record_id: h.record_id,
            qualification: h.qualification,
            engine_epoch: h.engine_epoch,
            device_session: h.device_session,
            hosting_request_id: h.hosting_request_id,
            allow_online_inspection: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    (r, qualification)
}
fn program_device_sample() -> (HandySetupDeviceObservation, HandyHspBuffer) {
    let mut device = setup_sample();
    device.remote_hssp.as_mut().unwrap().stream_id = 8;
    (device, crate::handy_wire::fixture_setup_acknowledgment())
}
#[test]
fn handy_program_correlation_accepts_streaming_buffers_and_repeats_fresh_reads_without_writes() {
    let f = fixture();
    let (r, _) = program_correlation_inputs(&f);
    let before: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .unwrap();
    for _ in 0..2 {
        let mut calls = 0;
        let ResponseBody::HandyProgramCorrelation(value) = f
            .engine
            .inspect_handy_program_with(
                &r,
                &f.session,
                |h, _| Ok(h.downloaded.clone()),
                |_, _| {
                    assert!(f.engine.db.try_lock().is_ok());
                    assert!(f.engine._device_effect_journal.try_lock().is_ok());
                    let (device, mut buffer) = program_device_sample();
                    buffer.points = calls;
                    buffer.max_points = 1.0;
                    buffer.first_point_time = 0;
                    buffer.last_point_time = 0;
                    buffer.tail_point_stream_index = 100;
                    calls += 1;
                    Ok((device, buffer))
                },
            )
            .unwrap()
        else {
            panic!("correlation")
        };
        value.validate().unwrap();
        assert_eq!(calls, 2);
        assert_eq!(
            value.expected_samples,
            HandyScriptSampleShape {
                points: 2,
                first_time_ms: 0,
                last_time_ms: 1000
            }
        );
        assert_eq!(value.first_buffer.points, 0);
        assert_eq!(value.second_buffer.points, 1);
        assert!(value.expected_samples.points > value.second_buffer.max_points as u64);
        assert!(!value.content_verified && !value.playback_enabled);
        assert!(!encode(&value).unwrap().contains("synthetic-route-secret"));
    }
    let after: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
}
#[test]
fn handy_program_correlation_refuses_changed_selection_and_authority_before_remote_reads() {
    let f = fixture();
    let (original, _) = program_correlation_inputs(&f);
    for index in 0..8 {
        let mut r = original.clone();
        let Command::InspectHandyProgram {
            record_id,
            engine_epoch,
            setup_request_id,
            device_session,
            qualification,
            allow_online_inspection,
            owner_proof,
            ..
        } = &mut r.command
        else {
            unreachable!()
        };
        match index {
            0 => *record_id = ArtifactId::new("a".repeat(64)).unwrap(),
            1 => *engine_epoch = DeviceSessionId::new("old-boot").unwrap(),
            2 => *setup_request_id = RequestId::new("missing").unwrap(),
            3 => *device_session = DeviceSessionId::new("wrong-session").unwrap(),
            4 => qualification.revision += 1,
            5 => *allow_online_inspection = false,
            6 => *owner_proof = "wrong".into(),
            _ => r.expected_revision = Some(RevisionId::new(2)),
        };
        assert!(f
            .engine
            .inspect_handy_program_with(
                &r,
                &f.session,
                |_, _| panic!("no hosted read"),
                |_, _| panic!("no device read")
            )
            .is_err());
    }
}
#[test]
fn handy_program_correlation_refuses_changed_stream_mode_or_uninitialized_buffer() {
    let f = fixture();
    let (r, _) = program_correlation_inputs(&f);
    for variant in 0..4 {
        assert!(f
            .engine
            .inspect_handy_program_with(
                &r,
                &f.session,
                |h, _| Ok(h.downloaded.clone()),
                |_, _| {
                    let (mut device, mut buffer) = program_device_sample();
                    match variant {
                        0 => {
                            buffer.stream_id += 1;
                            device.remote_hssp.as_mut().unwrap().stream_id += 1;
                        }
                        1 => device.mode.mode_session_id += 1,
                        2 => {
                            buffer.play_state = 0;
                            device.remote_hssp.as_mut().unwrap().play_state =
                                HandyRemotePlayState::NotInitialized;
                        }
                        _ => buffer.current_time += 1,
                    };
                    Ok((device, buffer))
                }
            )
            .is_err());
    }
}
#[test]
fn handy_program_correlation_rechecks_revocation_and_session_lifetime_after_remote_reads() {
    for revoked in [true, false] {
        let f = fixture();
        let (r, accepted) = program_correlation_inputs(&f);
        let Command::InspectHandyProgram { device_session, .. } = &r.command else {
            unreachable!()
        };
        let (connection, _) = f
            .engine
            .handy_sessions
            .lock()
            .unwrap()
            .capture(&f.session, device_session)
            .unwrap();
        let result = f.engine.inspect_handy_program_with(
            &r,
            &f.session,
            |h, _| {
                if revoked {
                    f.engine
                        .handle(qualification_withdraw(
                            &f,
                            &accepted.dossier_id,
                            accepted.revision,
                        ))
                        .result
                        .unwrap();
                } else {
                    f.engine
                        .handy_sessions
                        .lock()
                        .unwrap()
                        .invalidate(&connection.registration.device_id);
                }
                Ok(h.downloaded.clone())
            },
            |_, _| Ok(program_device_sample()),
        );
        assert!(result.is_err());
    }
}
