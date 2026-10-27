fn device_timing_inputs(f: &Fixture) -> (Request, HandyControllerLease) {
    let (acquire, _) = controller_inputs(f, 1000);
    let lease = controller_report(controller_call(f, &acquire, 1_000_000_000).unwrap()).lease;
    let mut r = request(
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
    r.request_id = RequestId::new("inspect-device-timing").unwrap();
    (r, lease)
}
fn device_timing_read_fixture(
    clock: &mut dyn FnMut() -> PResult<u64>,
) -> PResult<crate::handy_online::DeviceTimingRead> {
    let mut sample = || -> PResult<HandyDeviceTimingSample> {
        Ok(HandyDeviceTimingSample {
            info: HandyDeviceTimeInfo {
                device_uptime_ms: 100,
                reported_clock_offset_ms: -125,
                last_sync_round_trip_ms: 10,
            },
            user_offset: HandyUserTimeOffset { offset_ms: -50 },
            read_started_boottime_ns: clock()?,
            read_finished_boottime_ns: clock()?,
        })
    };
    Ok(crate::handy_online::DeviceTimingRead {
        before: program_device_sample().0,
        first: sample()?,
        second: sample()?,
        after: program_device_sample().0,
    })
}
fn device_timing_report(body: ResponseBody) -> HandyDeviceTimingInspection {
    let ResponseBody::HandyDeviceTiming(v) = body else {
        panic!("timing inspection")
    };
    v.validate().unwrap();
    *v
}
#[test]
fn handy_device_timing_is_fresh_read_only_diagnostic_and_never_admits_synchronization() {
    let f = fixture();
    let (r, lease) = device_timing_inputs(&f);
    let count = || {
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM requests", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
    };
    let before = count();
    for tick in [1_000_000_000, 1_000_000_001] {
        let result = device_timing_report(
            f.engine
                .handy_device_timing_with(
                    &r,
                    &f.session,
                    |_, _, clock| {
                        assert!(f.engine.db.try_lock().is_ok());
                        assert!(f.engine.handy_preparations.try_lock().is_ok());
                        assert!(f.engine._device_effect_journal.try_lock().is_ok());
                        let mut read = device_timing_read_fixture(clock)?;
                        read.second.info.reported_clock_offset_ms = 126;
                        read.second.user_offset.offset_ms = 50;
                        Ok(read)
                    },
                    || Ok(tick),
                )
                .unwrap(),
        );
        assert!(!result.synchronization_verified && !result.playback_enabled);
        assert_eq!(result.first.info.reported_clock_offset_ms, -125);
        assert_eq!(result.second.user_offset.offset_ms, 50);
        assert_eq!(result.inspection_started_boottime_ns, tick);
        assert_eq!(
            result.controller.deadline_boottime_ns,
            lease.deadline_boottime_ns
        );
        for case in 0..4 {
            let mut bad = result.clone();
            match case {
                0 => bad.synchronization_verified = true,
                1 => bad.playback_enabled = true,
                2 => bad.second.info.device_uptime_ms = 99,
                _ => bad.inspection_finished_boottime_ns = lease.deadline_boottime_ns,
            };
            assert!(bad.validate().is_err());
        }
    }
    assert_eq!(count(), before);
}
#[test]
fn handy_device_timing_selection_and_consent_refuse_before_remote_reads() {
    let f = fixture();
    let (r, _) = device_timing_inputs(&f);
    for case in 0..6 {
        let mut bad = r.clone();
        let Command::InspectHandyDeviceTiming {
            owner_proof,
            expected_generation,
            allow_online_inspection,
            controller_id,
            ..
        } = &mut bad.command
        else {
            unreachable!()
        };
        match case {
            0 => *owner_proof = "wrong".into(),
            1 => *expected_generation = 1,
            2 => *allow_online_inspection = false,
            3 => *controller_id = ClientId::new("other").unwrap(),
            4 => bad.expected_revision = Some(RevisionId::new(2)),
            _ => bad.project = Some(ProjectId::new("other").unwrap()),
        }
        assert!(f
            .engine
            .handy_device_timing_with(
                &bad,
                &f.session,
                |_, _, _| panic!("no remote read"),
                || Ok(1_000_000_000)
            )
            .is_err());
    }
}
#[test]
fn handy_device_timing_remote_changes_and_failure_retire_only_the_selected_controller() {
    for case in 0..6 {
        let f = fixture();
        let (r, lease) = device_timing_inputs(&f);
        let result = f.engine.handy_device_timing_with(
            &r,
            &f.session,
            |_, _, clock| {
                let mut read = device_timing_read_fixture(clock)?;
                match case {
                    0 => read.after.identity.session_id = "changed".into(),
                    1 => read.before.operative_settings.stroke.min += 1.0,
                    2 => read.second.info.device_uptime_ms = 99,
                    3 => {
                        return Err(ProtocolError::new(
                            ErrorCode::Unavailable,
                            "fixture disconnect",
                        ))
                    }
                    4 => {
                        controller_call(
                            &f,
                            &controller_status_request(&f, &lease),
                            lease.deadline_boottime_ns,
                        )
                        .unwrap();
                    }
                    _ => {
                        controller_call(
                            &f,
                            &controller_renew_request(&f, &lease, "renew-during-timing"),
                            1_000_000_000,
                        )
                        .unwrap();
                        return Err(ProtocolError::new(
                            ErrorCode::Unavailable,
                            "late fixture error",
                        ));
                    }
                };
                Ok(read)
            },
            || Ok(1_000_000_000),
        );
        assert!(result.is_err());
        if case == 5 {
            let current = controller_report(
                controller_call(&f, &controller_status_request(&f, &lease), 1_000_000_000).unwrap(),
            );
            assert_eq!(current.lease.generation, 1);
            assert!(current.lease.armed);
        } else {
            assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
        }
    }
}
#[test]
fn handy_device_timing_revoked_qualification_and_project_changes_during_io_refuse_results() {
    for case in 0..3 {
        let f = fixture();
        let (r, lease) = device_timing_inputs(&f);
        let prep = playback_preparation_report(
            f.engine
                .handle(preparation_status_request(
                    &f,
                    &RequestId::new("prepare-program").unwrap(),
                ))
                .result
                .unwrap(),
        );
        let binding = prep.correlation.inspection.accepted_qualification.unwrap();
        let result = f.engine.handy_device_timing_with(
            &r,
            &f.session,
            |_, _, clock| {
                let read = device_timing_read_fixture(clock)?;
                if case == 0 {
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            Some(1),
                            commit_program(&f, 1, program(0.3)),
                        ))
                        .result
                        .unwrap();
                } else if case == 1 {
                    f.engine
                        .handle(qualification_withdraw(
                            &f,
                            &binding.dossier_id,
                            binding.revision,
                        ))
                        .result
                        .unwrap();
                } else {
                    f.engine
                        .handle(request(
                            &f.session,
                            Some(&f.project),
                            None,
                            Command::ReleaseHandyController {
                                acquisition_request_id: lease.acquisition_request_id.clone(),
                                controller_id: lease.controller_id.clone(),
                                owner_proof: f.engine.handy_owner_token.clone(),
                            },
                        ))
                        .result
                        .unwrap();
                }
                Ok(read)
            },
            || Ok(1_000_000_000),
        );
        assert!(result.is_err());
    }
}

#[test]
fn handy_device_timing_monitor_loss_and_restart_refuse_before_acquisition() {
    let f = fixture();
    let (r, _) = device_timing_inputs(&f);
    assert!(f.engine.handle(r.clone()).result.is_err());
    assert_eq!(f.engine.handy_preparations.lock().unwrap().len(), 0);
    let Fixture { engine, dir, .. } = f;
    let config = Engine::retire_for_test(engine);
    let engine = Engine::open(config).unwrap();
    assert!(engine.handle(r).result.is_err());
    assert_eq!(engine.handy_preparations.lock().unwrap().len(), 0);
    drop(engine);
    drop(dir);
}
