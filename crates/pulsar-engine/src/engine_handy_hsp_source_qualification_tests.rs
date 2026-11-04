// Source dossiers remain Synthetic/Pending in positive native-pin tests. The
// TEMP simulated decision is cfg(test) only and never represents measurement.
#[cfg(target_os = "linux")]
mod hsp_source_qualification_tests {
    use super::handy_hsp_source_qualification::{self as source, AcceptedHspSource};
    use super::*;
    struct SourceFixture {
        f: Fixture,
        media: BrowserMediaBinding,
        input: HandyHspSourceQualificationInput,
        import: Request,
    }
    fn current_revision(f: &Fixture) -> u64 {
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT revision FROM projects WHERE id=?1",
                [f.project.as_str()],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn profile() -> DeclaredInferenceProfile {
        let identity = |c: char| {
            let sha256 = c.to_string().repeat(64);
            ArtifactIdentity {
                artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                sha256,
                byte_len: 64,
            }
        };
        DeclaredInferenceProfile {
            schema_version: 1,
            model: identity('d'),
            runtime: identity('e'),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 6,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::TemporalScriptPosition,
                },
                resize: InferenceResize::NearestStretch,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0 / 255.0; 6],
                bias: vec![0.; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        }
    }
    fn fixture_source() -> SourceFixture {
        fixture_source_with_finite(false)
    }
    fn fixture_source_with_finite(finite: bool) -> SourceFixture {
        let f = fixture();
        // Both dossiers remain Synthetic/Pending. Only an expiring TEMP permit
        // enables native tests; no actual device/network or review is involved.
        let (path, mut hardware) = if finite {
            finite_hsp_qualification_input(&f)
        } else {
            hsp_qualification_input(&f, HandyEvidenceOrigin::Synthetic)
        };
        hardware.subject.profile.maximum_command_age_ns = 1_000_000_000;
        hardware.subject.profile.maximum_clock_uncertainty_ns = 100_000_000;
        hardware
            .subject
            .hsp
            .as_mut()
            .unwrap()
            .policy
            .max_sample_age_ns = 1_000_000_000;
        let imported = qualification_saved(
            f.engine
                .handle(qualification_import(
                    &f,
                    "source-hardware-import",
                    &path,
                    &hardware,
                ))
                .result
                .unwrap(),
        );
        let device_qualification =
            handy_qualification::install_authored_hsp_device_acceptance_for_test(
                &f.engine.db.lock().unwrap(),
                &imported.dossier_id,
                crate::handy_clock::boottime_ns()
                    .unwrap()
                    .checked_add(60_000_000_000)
                    .unwrap(),
            )
            .unwrap();
        let media_path = f.dir.path().join("authored-source.bin");
        fs::write(
            &media_path,
            b"AUTHORED source bytes; no actual video/browser correspondence claim",
        )
        .unwrap();
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path: media_path },
            ))
            .result
            .unwrap()
        else {
            panic!("source");
        };
        let before = current_revision(&f);
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(before),
                Command::SetSourcePlacement {
                    spec: SourcePlacementSpec {
                        source_version,
                        source_start: SourceTimestamp::new(0, 1).unwrap(),
                        source_end: SourceTimestamp::new(1, 1).unwrap(),
                        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                        project_anchor: ProjectTime::ZERO,
                        rate_numerator: 1,
                        rate_denominator: 1,
                    },
                    replace: None,
                },
            ))
            .result
            .unwrap();
        let revision = current_revision(&f);
        let ResponseBody::BrowserMediaCatalog(catalog) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(revision),
                Command::BrowserMediaCatalog,
            ))
            .result
            .unwrap()
        else {
            panic!("catalog");
        };
        let prepare = request(
            &f.session,
            Some(&f.project),
            Some(revision),
            Command::PrepareBrowserMedia {
                engine_epoch: catalog.engine_epoch,
                selection: catalog.entries[0].selection.clone(),
                clock: DeclaredSourceClock {
                    presentation_anchor: SourceTimestamp::new(5, 1).unwrap(),
                    elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
                },
                lease_ms: 60_000,
                allow_declared_clock: true,
            },
        );
        f.engine.handle(prepare.clone()).result.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let media = loop {
            let ResponseBody::BrowserMediaPreparation(report) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: prepare.request_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("media");
            };
            if report.state == BrowserMediaPreparationState::Ready {
                break report.binding.unwrap();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "authored media preparation did not finish"
            );
            std::thread::park_timeout(std::time::Duration::from_millis(2));
        };
        let mut evidence=HandyHspSourceEvidenceKind::ALL.into_iter().map(|kind| {
            let bytes=format!("AUTHORED SYNTHETIC {kind:?}; no measured analysis/display lead, physical timing or useful model claim");let path=f.dir.path().join(format!("source-{kind:?}.bin"));fs::write(&path,bytes.as_bytes()).unwrap();
            HandyHspSourceQualificationInputEvidence {kind,path,sha256:format!("{:x}",Sha256::digest(bytes.as_bytes())),byte_len:bytes.len() as u64}
        }).collect::<Vec<_>>();
        if finite {
            let bytes =
                b"AUTHORED finite input closure fixture; no physical or browser evidence claim";
            let path = f.dir.path().join("source-finite-input.bin");
            fs::write(&path, bytes).unwrap();
            evidence.push(HandyHspSourceQualificationInputEvidence {
                kind: HandyHspSourceEvidenceKind::FiniteInputClosure,
                path,
                sha256: format!("{:x}", Sha256::digest(bytes)),
                byte_len: bytes.len() as u64,
            });
        }
        let subject = HandyHspSourceSubject {
            cpu_runtime_image: None,
            schema_version: 1,
            origin: HandyEvidenceOrigin::Synthetic,
            context: imported.manifest.subject.context.clone(),
            device_qualification,
            browser_environment: ArtifactIdentity {
                artifact_id: ArtifactId::new(evidence[0].sha256.clone()).unwrap(),
                sha256: evidence[0].sha256.clone(),
                byte_len: evidence[0].byte_len,
            },
            project_id: f.project.clone(),
            revision: RevisionId::new(revision),
            selection: media.selection.clone(),
            clock: media.clock,
            inference_profile: profile(),
            source_time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
            recipe: HandyHspSourceRecipe::RvfcRgb64x64NearestStretchV1,
            age_basis: LiveCaptureAgeBasis::ProducerDeclared,
            age_observation: HandyHspSourceAgeObservation::HttpReceiptMonotonic,
            lookahead: BrowserAnalysisLookaheadPolicy {
                target_lead_us: 500_000,
                minimum_lead_us: 100_000,
                maximum_lead_us: 1_000_000,
            },
            maximum_analysis_age_ns: 500_000_000,
            maximum_capture_to_engine_receipt_ns: 100_000_000,
            finite_drain_policy_sha256: hardware.subject.hsp_finite_drain_policy_sha256().unwrap(),
            timing: HandyBrowserStartTiming {
                clock: BrowserClockAssumptions {
                    timestamp_error_ns: 1000,
                    relative_rate_error_ppm: 20,
                    maximum_age_ns: 1_000_000_000,
                    maximum_interval_ns: 100_000_000,
                },
                reference: HandyBrowserStartReference::ExpectedDisplay,
                display_early_ns: 2_000_000,
                display_late_ns: 3_000_000,
                source_correspondence_error_ns: 1_000_000,
                discrete_media_phase_error_ns: 1_000_000,
                media_rate_error_ppm: 20,
                physical_execution_error_ns: 1_000_000,
                maximum_start_age_ns: 500_000_000,
                maximum_alignment_error_ns: 50_000_000,
            },
        };
        let input = HandyHspSourceQualificationInput { subject, evidence };
        input.validate().unwrap();
        let import = import_request(&f, &input, "source-import");
        SourceFixture {
            f,
            media,
            input,
            import,
        }
    }
    fn import_request(f: &Fixture, input: &HandyHspSourceQualificationInput, id: &str) -> Request {
        let path = f.dir.path().join(format!("{id}.json"));
        let bytes = serde_json::to_vec(input).unwrap();
        fs::write(&path, &bytes).unwrap();
        let mut r = request(
            &f.session,
            Some(&f.project),
            Some(input.subject.revision.value()),
            Command::ImportHandyHspSourceQualification {
                path,
                manifest_sha256: format!("{:x}", Sha256::digest(bytes)),
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        r.request_id = RequestId::new(id).unwrap();
        r
    }
    fn imported(x: &SourceFixture) -> HandyHspSourceQualificationReport {
        let ResponseBody::HandyHspSourceQualificationSaved(receipt) =
            x.f.engine.handle(x.import.clone()).result.unwrap()
        else {
            panic!("source receipt");
        };
        receipt.validate().unwrap();
        receipt.report
    }
    fn capture(
        x: &SourceFixture,
        binding: &HandyHspSourceQualificationBinding,
        expected: &HandyHspSourceSubject,
    ) -> PResult<AcceptedHspSource> {
        let request = request(
            &x.f.session,
            Some(&x.f.project),
            Some(x.input.subject.revision.value()),
            Command::GetSnapshot,
        );
        let db = x.f.engine.db.lock().unwrap();
        x.f.engine
            .capture_handy_hsp_source_locked(&db, &request, binding, &x.media, expected)
    }
    fn simulated(
        x: &SourceFixture,
    ) -> (
        HandyHspSourceQualificationReport,
        HandyHspSourceQualificationBinding,
        AcceptedHspSource,
    ) {
        let report = imported(x);
        let binding = source::install_authored_hsp_source_acceptance_for_test(
            &x.f.engine.db.lock().unwrap(),
            &report.dossier_id,
        )
        .unwrap();
        let pin = capture(x, &binding, &x.input.subject).unwrap();
        (report, binding, pin)
    }
    #[test]
    fn hsp_source_import_and_replay_stay_pending_synthetic_and_cannot_be_physically_accepted() {
        let x = fixture_source();
        let first = imported(&x);
        let again = imported(&x);
        assert_eq!(first.dossier_id, again.dossier_id);
        assert_eq!(first.revision, 1);
        assert_eq!(first.state, HandyQualificationState::Pending);
        assert_eq!(
            first.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        let accept = request(
            &x.f.session,
            Some(&x.f.project),
            Some(x.input.subject.revision.value()),
            Command::AcceptHandyHspSourceQualification {
                dossier_id: first.dossier_id.clone(),
                expected_record_revision: 1,
                operator: "authored test operator; no real acceptance".into(),
                acknowledge_measured_evidence: true,
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        assert_eq!(
            x.f.engine.handle(accept).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let binding = HandyHspSourceQualificationBinding {
            dossier_id: first.dossier_id,
            revision: 2,
        };
        assert!(capture(&x, &binding, &x.input.subject).is_err());
        let db = x.f.engine.db.lock().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM handy_hsp_source_qualifications",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert!(!db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='handy_browser_start_qualifications')",[],|r|r.get::<_,bool>(0)).unwrap());
    }
    #[test]
    fn hsp_source_native_pin_uses_exact_profile_and_declared_age_bounds_under_held_db() {
        let x = fixture_source();
        let (report, binding, pin) = simulated(&x);
        assert!(pin.current());
        assert_eq!(pin.binding(), &binding);
        assert_eq!(pin.subject().origin, HandyEvidenceOrigin::Synthetic);
        let db = x.f.engine.db.lock().unwrap();
        source::revalidate_accepted_hsp_source(&db, &x.f.engine.pool, &pin).unwrap();
        let hint = source::accepted_hsp_source_subject(&db, &binding).unwrap();
        assert!(hint.exact_eq(&x.input.subject));
        let raw: String = db
            .query_row(
                "SELECT report FROM handy_hsp_source_qualifications WHERE id=?1",
                [report.dossier_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        let retained: HandyHspSourceQualificationReport = decode(&raw).unwrap();
        assert_eq!(retained.state, HandyQualificationState::Pending);
        assert_eq!(
            retained.manifest.subject.origin,
            HandyEvidenceOrigin::Synthetic
        );
        drop(db);
        let mut wrong = x.input.subject.clone();
        wrong.inference_profile.cpu_threads = 2;
        assert!(capture(&x, &binding, &wrong).is_err());
        let mut wrong = x.input.subject.clone();
        wrong.maximum_capture_to_engine_receipt_ns += 1;
        assert!(capture(&x, &binding, &wrong).is_err());
        assert!(pin.current());
    }
    #[test]
    fn hsp_source_declared_lead_and_total_age_must_fit_accepted_device_contract() {
        let x = fixture_source();
        for case in 0..3 {
            let mut input = x.input.clone();
            match case {
                0 => input.subject.lookahead.minimum_lead_us = 99_999,
                1 => input.subject.lookahead.maximum_lead_us = 1_000_001,
                _ => input.subject.maximum_capture_to_engine_receipt_ns = 600_000_000,
            };
            input.validate().unwrap();
            let req = import_request(&x.f, &input, &format!("invalid-envelope-{case}"));
            assert!(x.f.engine.handle(req).result.is_err());
        }
    }
    #[test]
    fn hsp_source_withdrawal_retires_native_pin_and_preserves_original_evidence() {
        let x = fixture_source();
        let (report, binding, pin) = simulated(&x);
        let withdrawal = request(
            &x.f.session,
            Some(&x.f.project),
            None,
            Command::WithdrawHandyHspSourceQualification {
                dossier_id: report.dossier_id.clone(),
                expected_record_revision: 1,
                reason: "withdraw authored applicability".into(),
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        );
        let ResponseBody::HandyHspSourceQualificationSaved(receipt) =
            x.f.engine.handle(withdrawal.clone()).result.unwrap()
        else {
            panic!("withdrawal");
        };
        assert_eq!(receipt.report.state, HandyQualificationState::Withdrawn);
        assert!(!pin.current());
        assert!(x.f.engine.handle(withdrawal).result.is_ok());
        assert!(capture(&x, &binding, &x.input.subject).is_err());
        assert_eq!(
            x.f.engine
                .db
                .lock()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM handy_hsp_source_qualification_evidence WHERE dossier=?1",
                    [report.dossier_id.as_str()],
                    |r| r.get::<_, u64>(0)
                )
                .unwrap(),
            4
        );
    }
    #[test]
    fn hsp_source_evidence_corruption_permanently_retires_original_pin() {
        let x = fixture_source();
        let (report, _, pin) = simulated(&x);
        let db = x.f.engine.db.lock().unwrap();
        db.execute("UPDATE handy_hsp_source_qualification_evidence SET data=CAST('changed' AS BLOB) WHERE dossier=?1 AND kind=?2",params![report.dossier_id.as_str(),encode(&HandyHspSourceEvidenceKind::SourceCorrespondence).unwrap()]).unwrap();
        assert!(source::revalidate_accepted_hsp_source(&db, &x.f.engine.pool, &pin).is_err());
        assert!(!pin.current());
        assert!(source::validate(&db).is_err());
    }
    #[test]
    fn hsp_source_project_mutation_and_shutdown_retire_actual_source_lifetimes() {
        let x = fixture_source();
        let (_, _, pin) = simulated(&x);
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                Some(x.input.subject.revision.value()),
                Command::RemoveSourcePlacement {
                    placement_id: x.media.selection.placement.placement_id.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!pin.current());
        let x = fixture_source();
        let (_, _, pin) = simulated(&x);
        let config = Engine::retire_for_test(x.f.engine);
        assert!(!pin.current());
        let reopened = Engine::open(config).unwrap();
        let db = reopened.db.lock().unwrap();
        assert!(!db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_temp_master WHERE name='pulsar_test_hsp_source_acceptance')",[],|r|r.get::<_,bool>(0)).unwrap());
    }
    #[test]
    fn finite_source_policy_requires_matching_device_and_explicit_native_selection() {
        let x = fixture_source_with_finite(true);
        let (_, _, pin) = simulated(&x);
        let registration = handy_registration_catalog(&x.f).registrations.remove(0);
        let selected = x
            .input
            .subject
            .finite_drain_policy_sha256
            .as_deref()
            .unwrap();
        let db = x.f.engine.db.lock().unwrap();
        let resolved = source::accepted_hsp_finite_drain_policy(
            &db,
            &x.f.engine.pool,
            &pin,
            &x.input.subject.device_qualification,
            &registration,
            Some(selected),
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.policy_sha256, selected);
        assert_eq!(resolved.policy.stop_budget_ns, 3_000_000);
        assert!(source::accepted_hsp_finite_drain_policy(
            &db,
            &x.f.engine.pool,
            &pin,
            &x.input.subject.device_qualification,
            &registration,
            None,
        )
        .unwrap()
        .is_none());
        assert!(source::accepted_hsp_finite_drain_policy(
            &db,
            &x.f.engine.pool,
            &pin,
            &x.input.subject.device_qualification,
            &registration,
            Some(&"a".repeat(64)),
        )
        .is_err());
        let mut other_device = x.input.subject.device_qualification.clone();
        other_device.revision += 1;
        assert!(source::accepted_hsp_finite_drain_policy(
            &db,
            &x.f.engine.pool,
            &pin,
            &other_device,
            &registration,
            Some(selected),
        )
        .is_err());
        source::validate(&db).unwrap();
        drop(db);
        let mut mismatched = x.input.clone();
        mismatched.subject.finite_drain_policy_sha256 = Some("a".repeat(64));
        let req = import_request(&x.f, &mismatched, "source-policy-mismatch");
        assert_eq!(
            x.f.engine.handle(req).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        assert!(pin.current());
    }
    #[test]
    fn finite_source_and_device_withdrawal_or_device_permit_expiry_prevent_drain() {
        for case in 0..3 {
            let x = fixture_source_with_finite(true);
            let (report, _, pin) = simulated(&x);
            let registration = handy_registration_catalog(&x.f).registrations.remove(0);
            if case == 0 {
                x.f.engine
                    .handle(request(
                        &x.f.session,
                        Some(&x.f.project),
                        None,
                        Command::WithdrawHandyHspSourceQualification {
                            dossier_id: report.dossier_id,
                            expected_record_revision: 1,
                            reason: "withdraw authored closure scope".into(),
                            owner_proof: x.f.engine.handy_owner_token.clone(),
                        },
                    ))
                    .result
                    .unwrap();
            } else if case == 1 {
                x.f.engine
                    .handle(qualification_withdraw(
                        &x.f,
                        &x.input.subject.device_qualification.dossier_id,
                        1,
                    ))
                    .result
                    .unwrap();
            } else {
                x.f.engine
                    .db
                    .lock()
                    .unwrap()
                    .execute(
                        "UPDATE temp.pulsar_test_hsp_device_acceptance SET expires_at_ns=0",
                        [],
                    )
                    .unwrap();
            }
            assert!(source::accepted_hsp_finite_drain_policy(
                &x.f.engine.db.lock().unwrap(),
                &x.f.engine.pool,
                &pin,
                &x.input.subject.device_qualification,
                &registration,
                x.input.subject.finite_drain_policy_sha256.as_deref(),
            )
            .is_err());
            assert!(!pin.current());
        }
    }
    #[test]
    fn legacy_source_cannot_select_finite_policy_or_import_finite_source_against_old_device() {
        let x = fixture_source();
        let (_, _, pin) = simulated(&x);
        let registration = handy_registration_catalog(&x.f).registrations.remove(0);
        assert!(source::accepted_hsp_finite_drain_policy(
            &x.f.engine.db.lock().unwrap(),
            &x.f.engine.pool,
            &pin,
            &x.input.subject.device_qualification,
            &registration,
            None,
        )
        .unwrap()
        .is_none());
        assert!(source::accepted_hsp_finite_drain_policy(
            &x.f.engine.db.lock().unwrap(),
            &x.f.engine.pool,
            &pin,
            &x.input.subject.device_qualification,
            &registration,
            Some(&"f".repeat(64)),
        )
        .is_err());
        let mut input = x.input.clone();
        input.subject.finite_drain_policy_sha256 = Some("f".repeat(64));
        let mut evidence = input.evidence.last().unwrap().clone();
        evidence.kind = HandyHspSourceEvidenceKind::FiniteInputClosure;
        input.evidence.push(evidence);
        input.validate().unwrap();
        let req = import_request(&x.f, &input, "source-new-policy-old-device");
        assert_eq!(
            x.f.engine.handle(req).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
    }
    #[test]
    fn hsp_source_successor_keeps_current_acceptance_when_original_media_retires_but_withdrawal_still_retires_both(
    ) {
        let x = fixture_source();
        let (report, binding, old) = simulated(&x);
        let prepare = request(
            &x.f.session,
            Some(&x.f.project),
            Some(x.media.revision.value()),
            Command::PrepareBrowserMediaSuccessor {
                predecessor: x.media.clone(),
                lease_ms: 60_000,
            },
        );
        x.f.engine.handle(prepare.clone()).result.unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let next_media = loop {
            let ResponseBody::BrowserMediaPreparation(value) =
                x.f.engine
                    .handle(request(
                        &x.f.session,
                        Some(&x.f.project),
                        None,
                        Command::BrowserMediaPreparationStatus {
                            original_request_id: prepare.request_id.clone(),
                        },
                    ))
                    .result
                    .unwrap()
            else {
                panic!("media")
            };
            if value.state == BrowserMediaPreparationState::Ready {
                assert!(value.current);
                break value.binding.unwrap();
            }
            assert_eq!(value.state, BrowserMediaPreparationState::Pending);
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        let db = x.f.engine.db.lock().unwrap();
        let media =
            x.f.engine
                .capture_browser_media_successor_locked(&db, &prepare, &x.media, &next_media)
                .unwrap();
        let mut changed = x.input.subject.clone();
        changed.maximum_capture_to_engine_receipt_ns += 1;
        assert!(x
            .f
            .engine
            .capture_handy_hsp_source_successor_locked(&db, &prepare, &old, &media, &changed)
            .is_err());
        let next =
            x.f.engine
                .capture_handy_hsp_source_successor_locked(
                    &db,
                    &prepare,
                    &old,
                    &media,
                    &x.input.subject,
                )
                .unwrap();
        assert_eq!(next.binding(), &binding);
        assert!(next.subject().exact_eq(old.subject()));
        media.commit().unwrap();
        drop(db);
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::ReleaseBrowserMedia {
                    original_request_id: x.media.preparation_request_id.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!old.current());
        assert!(next.current());
        let db = x.f.engine.db.lock().unwrap();
        assert!(source::revalidate_accepted_hsp_source(&db, &x.f.engine.pool, &old).is_err());
        assert!(
            next.current(),
            "capability-local retirement must not poison shared accepted subject"
        );
        source::revalidate_accepted_hsp_source(&db, &x.f.engine.pool, &next).unwrap();
        drop(db);
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::WithdrawHandyHspSourceQualification {
                    dossier_id: report.dossier_id,
                    expected_record_revision: 1,
                    reason: "withdraw authored continuity fixture".into(),
                    owner_proof: x.f.engine.handy_owner_token.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!old.current());
        assert!(!next.current());
        assert!(source::revalidate_accepted_hsp_source(
            &x.f.engine.db.lock().unwrap(),
            &x.f.engine.pool,
            &next
        )
        .is_err());
    }
}
