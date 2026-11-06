// Included in authority::tests. All IO stops before synchronization; no sockets
// are opened. Durable qualification remains Pending/Synthetic with the existing
// explicit TEMP test permit, which is independent of the IO selection seam.
#[cfg(target_os = "linux")]
mod hsp_public_preparation_io_tests {
    use super::*;
    use handy_hsp_preparation::HspPreparationIo;
    use std::sync::atomic::AtomicU64;
    use std::time::{Duration, Instant};

    struct RefusingIo {
        origin: Option<String>,
        now_calls: AtomicU64,
        inspections: AtomicU64,
        hold: AtomicBool,
        entered: AtomicBool,
        release: AtomicBool,
    }
    impl RefusingIo {
        fn new(origin: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                origin: origin.map(str::to_owned),
                now_calls: AtomicU64::new(0),
                inspections: AtomicU64::new(0),
                hold: AtomicBool::new(false),
                entered: AtomicBool::new(false),
                release: AtomicBool::new(false),
            })
        }
    }
    impl HspPreparationIo for RefusingIo {
        fn now(&self) -> PResult<u64> {
            self.now_calls.fetch_add(1, Ordering::AcqRel);
            crate::handy_clock::boottime_ns()
        }
        fn inspect(
            &self,
            _: Arc<crate::handy_credentials::Credentials>,
            deadline: Instant,
            clock: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_online::DeviceTimingRead> {
            self.inspections.fetch_add(1, Ordering::AcqRel);
            self.entered.store(true, Ordering::Release);
            while self.hold.load(Ordering::Acquire) && !self.release.load(Ordering::Acquire) {
                clock()?;
                if Instant::now() >= deadline {
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(1));
            }
            Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "selected test observer refused before synchronization",
            ))
        }
        fn synchronize(
            &self,
            _: Arc<crate::handy_credentials::Credentials>,
            _: Instant,
            _: &mut dyn FnMut() -> PResult<u64>,
        ) -> crate::device_effect_journal::ClockSyncCompletion {
            panic!("refusing test IO must not synchronize")
        }
        fn server_sample(
            &self,
            _: Instant,
            _: &mut dyn FnMut() -> PResult<u64>,
        ) -> PResult<crate::handy_server_clock::Sample> {
            panic!("refusing test IO must not manufacture server clock observations")
        }
        fn hsp_origin(&self) -> Option<String> {
            self.origin.clone()
        }
    }
    struct Selected {
        f: Fixture,
        connection: HandyConnectionSession,
        permit: HandyQualificationBinding,
    }
    impl Selected {
        fn new() -> Self {
            let f = fixture();
            let (path, input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Synthetic);
            let imported = qualification_saved(
                f.engine
                    .handle(qualification_import(
                        &f,
                        "public-io-synthetic-dossier",
                        &path,
                        &input,
                    ))
                    .result
                    .unwrap(),
            );
            assert_eq!(imported.state, HandyQualificationState::Pending);
            assert_eq!(
                imported.manifest.subject.origin,
                HandyEvidenceOrigin::Synthetic
            );
            let (_, connection) = hsp_qualification_connection(&f);
            let permit = handy_qualification::install_authored_hsp_device_acceptance_for_test(
                &f.engine.db.lock().unwrap(),
                &imported.dossier_id,
                crate::handy_clock::boottime_ns()
                    .unwrap()
                    .checked_add(30_000_000_000)
                    .unwrap(),
            )
            .unwrap();
            Self {
                f,
                connection,
                permit,
            }
        }
        fn prepare(&self) -> Request {
            request(
                &self.f.session,
                Some(&self.f.project),
                Some(0),
                Command::PrepareHandyHsp {
                    engine_epoch: self.connection.engine_epoch.clone(),
                    device_session: self.connection.device_session.clone(),
                    qualification: self.permit.clone(),
                    timeout_ms: 5_000,
                    allow_online_synchronization: true,
                    owner_proof: self.f.engine.handy_owner_token.clone(),
                },
            )
        }
        fn install(&self, io: Arc<dyn HspPreparationIo>) -> PResult<()> {
            self.f.engine.install_handy_hsp_preparation_io_for_test(
                &self.f.session,
                &self.connection.engine_epoch,
                &self.connection.device_session,
                io,
            )
        }
        fn status(&self, original: &RequestId) -> HandyHspPreparationStatus {
            let ResponseBody::HandyHspPreparation(report) = self
                .f
                .engine
                .handle(request(
                    &self.f.session,
                    Some(&self.f.project),
                    None,
                    Command::HandyHspPreparationStatus {
                        preparation_request_id: original.clone(),
                        owner_proof: self.f.engine.handy_owner_token.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("preparation status");
            };
            *report
        }
        fn wait_failed(&self, original: &RequestId) {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                let report = self.status(original);
                if let Some(native) = report
                    .native
                    .as_ref()
                    .filter(|native| !native.worker_active && native.error.is_some())
                {
                    assert!(native
                        .error
                        .as_ref()
                        .unwrap()
                        .message
                        .contains("selected test observer refused"));
                    assert!(
                        report.history.is_none(),
                        "refusal cannot mint synchronization history"
                    );
                    return;
                }
                assert!(
                    Instant::now() < until,
                    "selected observer did not settle: {report:?}"
                );
                std::thread::park_timeout(Duration::from_millis(1));
            }
        }
    }
    #[test]
    fn public_hsp_preparation_selected_io_reaches_original_admission_without_clock_fabrication() {
        let x = Selected::new();
        let io = RefusingIo::new(Some("http://127.0.0.1:1"));
        x.install(io.clone()).unwrap();
        let original = x.prepare();
        x.f.engine.handle(original.clone()).result.unwrap();
        x.wait_failed(&original.request_id);
        assert_eq!(io.inspections.load(Ordering::Acquire), 1);
        x.f.engine.handle(original.clone()).result.unwrap();
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::CancelHandyHspPreparation {
                    preparation_request_id: original.request_id,
                    owner_proof: x.f.engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(io.inspections.load(Ordering::Acquire), 1);
        let ResponseBody::HandyQualification(report) =
            qualification_status(&x.f, &x.permit.dossier_id).unwrap()
        else {
            panic!("original qualification status");
        };
        assert_eq!(report.state, HandyQualificationState::Pending);
        x.f.engine.finish_shutdown().unwrap();
    }
    #[test]
    fn public_hsp_preparation_override_scope_mismatch_never_falls_through_to_native_io() {
        let x = Selected::new();
        let io = RefusingIo::new(Some("http://127.0.0.1:1"));
        x.install(io.clone()).unwrap();
        let stranger = pair(&x.f.engine, "scoped observer foreign actor");
        x.f.engine
            .handle(request(
                &x.f.session,
                None,
                None,
                Command::Grant {
                    session: stranger.clone(),
                    project_id: x.f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Export],
                },
            ))
            .result
            .unwrap();
        for case in 0..3 {
            let mut r = x.prepare();
            let Command::PrepareHandyHsp {
                engine_epoch,
                device_session,
                ..
            } = &mut r.command
            else {
                unreachable!();
            };
            match case {
                0 => *engine_epoch = DeviceSessionId::new("foreign-epoch").unwrap(),
                1 => *device_session = DeviceSessionId::new("foreign-device-session").unwrap(),
                _ => {
                    r.session = Some(stranger.clone());
                    r.auth_token = TOKENS.with(|t| t.borrow().get(stranger.as_str()).cloned());
                }
            }
            assert_eq!(
                x.f.engine.handle(r).result.unwrap_err().code,
                ErrorCode::DependencyMismatch
            );
        }
        assert_eq!(io.now_calls.load(Ordering::Acquire), 0);
        assert_eq!(io.inspections.load(Ordering::Acquire), 0);
        x.f.engine.finish_shutdown().unwrap();
    }
    #[test]
    fn public_hsp_preparation_io_is_per_engine_and_install_is_once_before_admission() {
        let x = Selected::new();
        let y = Selected::new();
        let io = RefusingIo::new(Some("http://127.0.0.1:1"));
        x.install(io.clone()).unwrap();
        assert_eq!(
            x.install(io.clone()).unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let uninstalled =
            y.f.engine
                .public_hsp_preparation_io_for_test(&y.prepare(), &y.f.session)
                .unwrap();
        assert!(
            uninstalled.hsp_origin().is_none(),
            "another Engine inherited fixture IO"
        );
        assert_eq!(
            y.install(io.clone()).unwrap_err().code,
            ErrorCode::RequestConflict
        );
        assert_eq!(io.now_calls.load(Ordering::Acquire), 0);
        let original = x.prepare();
        x.f.engine.handle(original.clone()).result.unwrap();
        x.wait_failed(&original.request_id);
        assert_eq!(io.inspections.load(Ordering::Acquire), 1);
        x.f.engine.finish_shutdown().unwrap();
        y.f.engine.finish_shutdown().unwrap();
    }
    #[test]
    fn public_hsp_preparation_install_refuses_existing_original_entry_and_nonloopback_origin() {
        let x = Selected::new();
        for origin in [
            None,
            Some("https://127.0.0.1:1"),
            Some("http://192.0.2.1:1"),
            Some("http://localhost:1"),
        ] {
            assert!(x.install(RefusingIo::new(origin)).is_err());
        }
        let io = RefusingIo::new(Some("http://127.0.0.1:1"));
        io.hold.store(true, Ordering::Release);
        let original = x.prepare();
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(&original).unwrap())
        );
        x.f.engine
            .handy_hsp_preparation_with_io(&original, &x.f.session, &fingerprint, io.clone())
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !io.entered.load(Ordering::Acquire) {
            assert!(Instant::now() < until, "original inspection not entered");
            std::thread::park_timeout(Duration::from_millis(1));
        }
        assert!(
            x.install(io.clone()).is_err(),
            "active original entry was replaced"
        );
        io.release.store(true, Ordering::Release);
        x.wait_failed(&original.request_id);
        assert_eq!(
            x.install(io.clone()).unwrap_err().code,
            ErrorCode::RequestConflict
        );
        x.f.engine.finish_shutdown().unwrap();
    }
}
