fn hosting_prepared(f: &Fixture) -> HandyStrokeScript {
    handy_script_commit(f);
    handy_script_receipt(
        f.engine
            .handle(handy_script_request(
                f,
                "prepared",
                f.dir.path().join("prepared.csv"),
                1,
            ))
            .result
            .unwrap(),
    )
}
fn hosting_request(f: &Fixture, p: &HandyStrokeScript, id: &str) -> Request {
    let mut r = request(
        &f.session,
        Some(&f.project),
        Some(1),
        Command::HostHandyStrokeScript {
            preparation_request_id: p.request_id.clone(),
            script_sha256: p.identity.sha256.clone(),
            engine_epoch: DeviceSessionId::new(f.engine.engine_epoch.clone()).unwrap(),
            allow_temporary_public_hosting: true,
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    );
    r.request_id = RequestId::new(id).unwrap();
    r
}
fn hosting_call<F>(f: &Fixture, r: &Request, host: F) -> PResult<ResponseBody>
where
    F: FnOnce(
        &[u8],
    ) -> Result<
        crate::handy_hosting_wire::VerifiedHosted,
        crate::handy_hosting_wire::UnknownHostingOutcome,
    >,
{
    let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(r).unwrap()));
    f.engine
        .handy_hosting_with(r, &f.session, &fingerprint, host)
}
fn hosting_report(body: ResponseBody) -> HandyHostingReport {
    let ResponseBody::HandyHosting(r) = body else {
        panic!("hosting report")
    };
    r.validate().unwrap();
    *r
}
fn hosting_status(f: &Fixture, id: &RequestId) -> HandyHostingReport {
    hosting_report(
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::HandyHostingStatus {
                    upload_request_id: id.clone(),
                    owner_proof: f.engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap(),
    )
}
fn hosting_forget(f: &Fixture, id: &RequestId) -> Request {
    request(
        &f.session,
        Some(&f.project),
        None,
        Command::ForgetHandyHosting {
            upload_request_id: id.clone(),
            owner_proof: f.engine.handy_owner_token.clone(),
        },
    )
}
#[test]
fn handy_hosting_reserves_before_disclosure_and_replay_never_uploads_again() {
    let f = fixture();
    let p = hosting_prepared(&f);
    let r = hosting_request(&f, &p, "upload");
    let report = hosting_report(
        hosting_call(&f, &r, |bytes| {
            let active = hosting_status(&f, &r.request_id);
            assert_eq!(active.outcome, HandyHostingOutcome::Unknown);
            assert!(active.upload_in_flight && !active.link_available);
            assert!(
                hosting_report(hosting_call(&f, &r, |_| panic!("duplicate upload")).unwrap())
                    .upload_in_flight
            );
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), p.identity.sha256);
            fs::write(&p.path, b"replacement after capture").unwrap();
            Ok(crate::handy_hosting_wire::fixture_verified(bytes))
        })
        .unwrap(),
    );
    assert_eq!(report.outcome, HandyHostingOutcome::Verified);
    assert!(report.link_available && !report.upload_in_flight);
    assert!(hosting_report(hosting_call(&f, &r, |_| panic!("replay")).unwrap()).link_available);
    let raw: String = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT response FROM requests WHERE request='upload'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !raw.contains("https://")
            && !raw.contains("/download/")
            && !raw.contains(&f.engine.handy_owner_token)
    );
    let forget = hosting_forget(&f, &r.request_id);
    let retired = hosting_report(f.engine.handle(forget.clone()).result.unwrap());
    assert!(retired.retired && !retired.link_available);
    assert!(hosting_report(f.engine.handle(forget).result.unwrap()).retired);
    assert!(!hosting_report(hosting_call(&f, &r, |_| panic!()).unwrap()).link_available);
}
#[test]
fn handy_hosting_denies_changed_bytes_revision_owner_and_consent_before_broker() {
    let f = fixture();
    let p = hosting_prepared(&f);
    let r = hosting_request(&f, &p, "denied");
    let mut denied = r.clone();
    if let Command::HostHandyStrokeScript {
        allow_temporary_public_hosting,
        ..
    } = &mut denied.command
    {
        *allow_temporary_public_hosting = false;
    }
    assert_eq!(
        hosting_call(&f, &denied, |_| panic!()).unwrap_err().code,
        ErrorCode::Forbidden
    );
    denied = r.clone();
    if let Command::HostHandyStrokeScript { owner_proof, .. } = &mut denied.command {
        *owner_proof = "f".repeat(64);
    }
    assert_eq!(
        hosting_call(&f, &denied, |_| panic!()).unwrap_err().code,
        ErrorCode::Forbidden
    );
    denied = r.clone();
    denied.expected_revision = Some(RevisionId::new(0));
    assert_eq!(
        hosting_call(&f, &denied, |_| panic!()).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    fs::write(&p.path, b"changed").unwrap();
    assert_eq!(
        hosting_call(&f, &r, |_| panic!()).unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM requests WHERE request='denied'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        0
    );
}
#[test]
fn handy_hosting_forget_during_io_cannot_claim_cancellation_or_restore_link() {
    let f = fixture();
    let p = hosting_prepared(&f);
    let r = hosting_request(&f, &p, "upload");
    let report = hosting_report(
        hosting_call(&f, &r, |bytes| {
            let forget = hosting_forget(&f, &r.request_id);
            let forgotten = hosting_report(f.engine.handle(forget.clone()).result.unwrap());
            assert!(forgotten.retired && forgotten.upload_in_flight && !forgotten.link_available);
            assert!(hosting_report(f.engine.handle(forget).result.unwrap()).upload_in_flight);
            Ok(crate::handy_hosting_wire::fixture_verified(bytes))
        })
        .unwrap(),
    );
    assert_eq!(report.outcome, HandyHostingOutcome::Verified);
    assert!(report.retired && !report.link_available && !report.upload_in_flight);
}
#[test]
fn handy_hosting_lost_metadata_ack_and_panic_preserve_uncertainty_without_reupload() {
    let f = fixture();
    let p = hosting_prepared(&f);
    let r = hosting_request(&f, &p, "write-failed");
    assert!(hosting_call(&f,&r,|bytes| {
        f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER fixture_host_ack BEFORE UPDATE ON requests WHEN NEW.request='write-failed' BEGIN SELECT RAISE(ABORT,'fixture completion failure'); END;").unwrap();Ok(crate::handy_hosting_wire::fixture_verified(bytes))
    }).is_err());
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fixture_host_ack")
        .unwrap();
    let report =
        hosting_report(hosting_call(&f, &r, |_| panic!("no resend after ack failure")).unwrap());
    assert_eq!(report.outcome, HandyHostingOutcome::Unknown);
    assert!(!report.link_available && !report.upload_in_flight);
    let panic_request = hosting_request(&f, &p, "panic");
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hosting_call(
            &f,
            &panic_request,
            |_| panic!("fixture broker panic")
        )))
        .is_err()
    );
    assert!(!hosting_status(&f, &panic_request.request_id).upload_in_flight);
}
#[test]
fn handy_hosting_revocation_records_observation_but_retires_usable_link() {
    let f = fixture();
    let p = hosting_prepared(&f);
    let r = hosting_request(&f, &p, "revoke");
    assert_eq!(
        hosting_call(&f, &r, |bytes| {
            f.engine
                .db
                .lock()
                .unwrap()
                .execute(
                    "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
                    params![
                        f.session.as_str(),
                        f.project.as_str(),
                        scope_name(Scope::Export)
                    ],
                )
                .unwrap();
            Ok(crate::handy_hosting_wire::fixture_verified(bytes))
        })
        .unwrap_err()
        .code,
        ErrorCode::Forbidden
    );
    let raw: String = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT response FROM requests WHERE request='revoke'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let report = hosting_report(decode(&raw).unwrap());
    assert!(report.retired && !report.link_available);
    assert_eq!(report.outcome, HandyHostingOutcome::Verified);
}
#[test]
fn handy_hosting_restart_keeps_history_but_never_restores_url_or_delayed_consent() {
    let f = fixture();
    let p = hosting_prepared(&f);
    let r = hosting_request(&f, &p, "upload");
    assert!(
        hosting_report(
            hosting_call(&f, &r, |bytes| Ok(
                crate::handy_hosting_wire::fixture_verified(bytes)
            ))
            .unwrap()
        )
        .link_available
    );
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
    let report = hosting_report(hosting_call(&f, &r, |_| panic!("restart resend")).unwrap());
    assert_eq!(report.outcome, HandyHostingOutcome::Verified);
    assert!(!report.link_available && !report.upload_in_flight);
    let mut delayed = r;
    delayed.request_id = RequestId::new("never-dispatched").unwrap();
    assert_eq!(
        hosting_call(&f, &delayed, |_| panic!()).unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
}
