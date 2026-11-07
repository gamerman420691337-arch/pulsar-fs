fn cancel_setup_request(f: &Fixture, original: &RequestId, record_id: &str, id: &str) -> Request {
    let mut r = request(
        &f.session,
        Some(&f.project),
        None,
        Command::CancelHandySetup {
            setup_request_id: original.clone(),
            record_id: ArtifactId::new(record_id).unwrap(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
fn cancellation_report(body: ResponseBody) -> HandySetupCancellation {
    let ResponseBody::HandySetupCancellation(value) = body else {
        panic!("cancellation")
    };
    value.validate().unwrap();
    *value
}
#[test]
fn handy_setup_cancel_releases_only_unclaimed_exact_history_and_replay_is_inert() {
    let f = fixture();
    let record = setup_status_fixture_record(&f);
    let original = record.request_id.clone();
    let route = record.route_sha256.clone();
    let prepared = f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .prepare_setup(record)
        .unwrap();
    let request = cancel_setup_request(&f, &original, prepared.id(), "cancel-original");
    let receipt = cancellation_report(f.engine.handle(request.clone()).result.unwrap());
    assert!(receipt.settled);
    assert_eq!(
        receipt.status.history.as_ref().unwrap().state,
        HandySetupState::Cancelled
    );
    assert!(!receipt.status.loaded && !receipt.status.qualified);
    assert!(f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .claim_setup(&prepared)
        .is_err());
    assert!(!f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .setup_route_reserved(&route)
        .unwrap());
    let repeated = cancellation_report(f.engine.handle(request).result.unwrap());
    assert_eq!(
        serde_json::to_string(&receipt).unwrap(),
        serde_json::to_string(&repeated).unwrap()
    );
}
#[test]
fn handy_setup_cancel_cannot_clear_claimed_acknowledged_or_unknown_attempts() {
    use crate::device_effect_journal::SetupCompletion;
    for outcome in [
        None,
        Some(SetupCompletion::Acknowledged(
            crate::handy_wire::fixture_setup_acknowledgment(),
        )),
        Some(SetupCompletion::Unknown),
    ] {
        let f = fixture();
        let record = setup_status_fixture_record(&f);
        let original = record.request_id.clone();
        let route = record.route_sha256.clone();
        let prepared = f
            .engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .prepare_setup(record)
            .unwrap();
        let claim = f
            .engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .claim_setup(&prepared)
            .unwrap();
        if let Some(value) = outcome {
            f.engine
                ._device_effect_journal
                .lock()
                .unwrap()
                .complete_setup(&claim, value)
                .unwrap();
        }
        let result = f
            .engine
            .handle(cancel_setup_request(
                &f,
                &original,
                prepared.id(),
                "refused-cancel",
            ))
            .result;
        assert_eq!(result.unwrap_err().code, ErrorCode::Forbidden);
        assert!(f
            .engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .setup_route_reserved(&route)
            .unwrap());
        assert_eq!(
            f.engine
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM requests WHERE request='refused-cancel'",
                    [],
                    |r| r.get::<_, u64>(0)
                )
                .unwrap(),
            0
        );
    }
}
#[test]
fn handy_setup_cancel_reservation_failure_preserves_preparation_and_completion_retry_cannot_touch_replacement(
) {
    let f = fixture();
    let record = setup_status_fixture_record(&f);
    let original = record.request_id.clone();
    let prepared = f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .prepare_setup(record.clone())
        .unwrap();
    let request = cancel_setup_request(&f, &original, prepared.id(), "cancel-failure");
    f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER cancel_reservation_failure BEFORE INSERT ON requests WHEN NEW.request='cancel-failure' BEGIN SELECT RAISE(ABORT,'fixture reserve failure'); END;").unwrap();
    assert!(f.engine.handle(request.clone()).result.is_err());
    assert_eq!(
        f.engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .setup_history(prepared.id())
            .unwrap()
            .state,
        crate::device_effect_journal::SetupJournalState::Prepared
    );
    f.engine.db.lock().unwrap().execute_batch("DROP TRIGGER cancel_reservation_failure; CREATE TRIGGER cancel_ack_failure BEFORE UPDATE ON requests WHEN NEW.request='cancel-failure' BEGIN SELECT RAISE(ABORT,'fixture ack failure'); END;").unwrap();
    assert_eq!(
        f.engine.handle(request.clone()).result.unwrap_err().code,
        ErrorCode::UnknownOutcome
    );
    assert_eq!(
        f.engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .setup_history(prepared.id())
            .unwrap()
            .state,
        crate::device_effect_journal::SetupJournalState::Cancelled
    );
    let mut replacement = record;
    replacement.request_id = RequestId::new("replacement").unwrap();
    let replacement = f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .prepare_setup(replacement)
        .unwrap();
    let mut changed = request.clone();
    if let Command::CancelHandySetup { record_id, .. } = &mut changed.command {
        *record_id = ArtifactId::new(replacement.id()).unwrap();
    }
    assert!(f.engine.handle(changed).result.is_err());
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER cancel_ack_failure")
        .unwrap();
    assert!(cancellation_report(f.engine.handle(request).result.unwrap()).settled);
    assert_eq!(
        f.engine
            ._device_effect_journal
            .lock()
            .unwrap()
            .setup_history(replacement.id())
            .unwrap()
            .state,
        crate::device_effect_journal::SetupJournalState::Prepared
    );
}
#[test]
fn handy_setup_cancel_checks_actor_record_pin_owner_and_current_read_grant() {
    let f = fixture();
    let record = setup_status_fixture_record(&f);
    let original = record.request_id.clone();
    let prepared = f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .prepare_setup(record)
        .unwrap();
    let wrong = cancel_setup_request(&f, &original, &"a".repeat(64), "wrong-pin");
    assert_eq!(
        f.engine.handle(wrong).result.unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
    let other = pair(&f.engine, "other cancel actor");
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO grants VALUES(?1,?2,?3)",
            params![other.as_str(), f.project.as_str(), scope_name(Scope::Read)],
        )
        .unwrap();
    let other_request = request(
        &other,
        Some(&f.project),
        None,
        Command::CancelHandySetup {
            setup_request_id: original.clone(),
            record_id: ArtifactId::new(prepared.id()).unwrap(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    assert_eq!(
        f.engine.handle(other_request).result.unwrap_err().code,
        ErrorCode::NotFound
    );
    let mut wrong = cancel_setup_request(&f, &original, prepared.id(), "wrong-owner");
    if let Command::CancelHandySetup { owner_proof, .. } = &mut wrong.command {
        *owner_proof = "wrong".into();
    }
    assert_eq!(
        f.engine.handle(wrong).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::Read)
            ],
        )
        .unwrap();
    assert_eq!(
        f.engine
            .handle(cancel_setup_request(
                &f,
                &original,
                prepared.id(),
                "no-read"
            ))
            .result
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
}
