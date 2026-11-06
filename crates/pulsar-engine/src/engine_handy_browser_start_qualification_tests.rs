// Authored files, simulated operator decisions and injected hosting/device
// observations exercise conditional custody only; none is real qualification.
#[cfg(target_os = "linux")]
mod browser_start_qualification_tests {
    use super::handy_browser_start_qualification::{self as dossier, AcceptedBrowserStart};
    use super::*;

    struct BrowserFixture {
        f: Fixture,
        hardware: HandyQualificationReport,
        setup: Request,
        media: BrowserMediaBinding,
        preparation: Request,
        hosting: HandyHostingReport,
        script: HandyStrokeScript,
        input: HandyBrowserStartQualificationInput,
        import: Request,
    }
    fn fixture_browser(origin: HandyEvidenceOrigin) -> BrowserFixture {
        let f = fixture();
        let (mut setup, hardware) =
            setup_send_request_with(&f, "browser-unexecuted-setup", |input| {
                let s = &mut input.subject;
                s.schema_version = 6;
                s.profile.maximum_command_age_ns = 1_000_000_000;
                s.profile.maximum_clock_uncertainty_ns = 100_000_000;
                s.server_clock = Some(HandyServerClockQualification {
                    model: HandyServerClockModel::RequestBracketMonotonic,
                    timestamp_error_ns: 1_000,
                });
                s.device_clock = Some(HandyDeviceClockQualification {
                    model: HandyDeviceClockModel::SynchronousResponseEffectiveClock,
                    user_offset_ms: -50,
                    device_error_ns: 1_000_000,
                    maximum_sync_age_ns: 500_000_000,
                    maximum_sync_round_trip_ms: 100,
                    drift_ppm: 20,
                });
                s.recovery = Some(HandyRecoveryQualification {
                    transport: HandyQualificationTransport::Hssp,
                    model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
                    remote_state: HandyRecoveryRemoteState::Stopped,
                    prior_command_settle_ns: 1_000_000,
                    stopped_sample_separation_ns: 1_000_000,
                    maximum_sample_span_ns: 100_000_000,
                });
                s.continuous_timing = Some(HandyContinuousTimingQualification {
                    model: HandyContinuousTimingModel::HsspPlaytimeRequestBracket,
                    timestamp_error_ns: 1_000,
                    maximum_observation_age_ns: 1_000_000_000,
                    maximum_sample_span_ns: 100_000_000,
                    drift_ppm: 20,
                });
            });
        let path = f.dir.path().join("browser-source.bin");
        fs::write(
            &path,
            b"SYNTHETIC browser source; not video correspondence evidence",
        )
        .unwrap();
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path },
            ))
            .result
            .unwrap()
        else {
            panic!("source");
        };
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::SetSourcePlacement {
                    spec: SourcePlacementSpec {
                        source_version,
                        source_start: SourceTimestamp::new(0, 1).unwrap(),
                        source_end: SourceTimestamp::new(1, 1).unwrap(),
                        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                        project_anchor: ProjectTime::from_nanos(0),
                        rate_numerator: 1,
                        rate_denominator: 1,
                    },
                    replace: None,
                },
            ))
            .result
            .unwrap();
        let ResponseBody::BrowserMediaCatalog(catalog) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(2),
                Command::BrowserMediaCatalog,
            ))
            .result
            .unwrap()
        else {
            panic!("catalog");
        };
        let preparation = request(
            &f.session,
            Some(&f.project),
            Some(2),
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
        f.engine.handle(preparation.clone()).result.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let media = loop {
            let ResponseBody::BrowserMediaPreparation(value) = f
                .engine
                .handle(request(
                    &f.session,
                    Some(&f.project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: preparation.request_id.clone(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("prepared media");
            };
            if value.state == BrowserMediaPreparationState::Ready {
                assert!(value.current);
                break value.binding.unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        let script = handy_script_receipt(
            f.engine
                .handle(handy_script_request(
                    &f,
                    "browser-script",
                    f.dir.path().join("browser.csv"),
                    2,
                ))
                .result
                .unwrap(),
        );
        let mut host = hosting_request(&f, &script, "browser-host");
        host.expected_revision = Some(RevisionId::new(2));
        let hosting = hosting_report(
            hosting_call(&f, &host, |bytes| {
                Ok(crate::handy_hosting_wire::fixture_verified(bytes))
            })
            .unwrap(),
        );
        setup.expected_revision = Some(RevisionId::new(2));
        let Command::SetupHandyDevice {
            hosting_request_id,
            qualification,
            ..
        } = &mut setup.command
        else {
            panic!("unexecuted setup request");
        };
        *hosting_request_id = hosting.request_id.clone();
        *qualification = HandyQualificationBinding {
            dossier_id: hardware.dossier_id.clone(),
            revision: hardware.revision,
        };
        setup.validate().unwrap();
        let evidence: Vec<_> = HandyBrowserStartEvidenceKind::ALL.into_iter().map(|kind| {
            let bytes = format!("SYNTHETIC TEST FIXTURE {kind:?}; no measured browser applicability or physical acceptance");
            let path = f.dir.path().join(format!("browser-{kind:?}.bin"));
            fs::write(&path, bytes.as_bytes()).unwrap();
            HandyBrowserStartQualificationInputEvidence {
                kind, path, sha256: format!("{:x}", Sha256::digest(bytes.as_bytes())), byte_len: bytes.len() as u64,
            }
        }).collect();
        let input = HandyBrowserStartQualificationInput {
            subject: HandyBrowserStartSubject {
                schema_version: 1,
                origin,
                context: hardware.manifest.subject.context.clone(),
                device_qualification: HandyQualificationBinding {
                    dossier_id: hardware.dossier_id.clone(),
                    revision: hardware.revision,
                },
                browser_environment: ArtifactIdentity {
                    artifact_id: ArtifactId::new(evidence[0].sha256.clone()).unwrap(),
                    sha256: evidence[0].sha256.clone(),
                    byte_len: evidence[0].byte_len,
                },
                project_id: f.project.clone(),
                revision: RevisionId::new(2),
                selection: media.selection.clone(),
                clock: media.clock,
                master_sha256: script.master_sha256.clone(),
                script_actor_session: f.session.clone(),
                script_request_id: script.request_id.clone(),
                script: script.identity.clone(),
                timing: HandyBrowserStartTiming {
                    clock: BrowserClockAssumptions {
                        timestamp_error_ns: 1_000,
                        relative_rate_error_ppm: 20,
                        maximum_age_ns: 1_000_000_000,
                        maximum_interval_ns: 100_000_000,
                    },
                    reference: HandyBrowserStartReference::Callback,
                    display_early_ns: 2_000_000,
                    display_late_ns: 3_000_000,
                    source_correspondence_error_ns: 1_000_000,
                    discrete_media_phase_error_ns: 1_000_000,
                    media_rate_error_ppm: 20,
                    physical_execution_error_ns: 1_000_000,
                    maximum_start_age_ns: 500_000_000,
                    maximum_alignment_error_ns: 50_000_000,
                },
            },
            evidence,
        };
        input.validate().unwrap();
        let import = import_request(&f, "browser-import", &input);
        BrowserFixture {
            f,
            hardware,
            setup,
            media,
            preparation,
            hosting,
            script,
            input,
            import,
        }
    }
    /// Connected playback tests receive only ordinary receipts and the native
    /// Engine/source lifetime produced by existing public preparation paths.
    /// The Physical label and review are authored unit fixtures, not measurements.
    pub(super) fn browser_execution_fixture() -> (
        Fixture,
        BrowserMediaBinding,
        HandyBrowserStartQualificationReport,
        Request,
    ) {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let pending = imported(&x);
        let accepted = saved(x.f.engine.handle(accept(&x, &pending)).result.unwrap());
        (x.f, x.media, accepted, x.setup)
    }
    fn import_request(
        f: &Fixture,
        id: &str,
        input: &HandyBrowserStartQualificationInput,
    ) -> Request {
        let path = f.dir.path().join(format!("{id}.json"));
        let bytes = serde_json::to_vec(input).unwrap();
        fs::write(&path, &bytes).unwrap();
        let mut r = request(
            &f.session,
            Some(&f.project),
            Some(2),
            Command::ImportHandyBrowserStartQualification {
                path,
                manifest_sha256: format!("{:x}", Sha256::digest(&bytes)),
                owner_proof: f.engine.handy_owner_token.clone(),
            },
        );
        r.request_id = RequestId::new(id).unwrap();
        r
    }
    fn saved(body: ResponseBody) -> HandyBrowserStartQualificationReport {
        let ResponseBody::HandyBrowserStartQualificationSaved(receipt) = body else {
            panic!("browser dossier receipt");
        };
        receipt.validate().unwrap();
        receipt.report
    }
    fn imported(x: &BrowserFixture) -> HandyBrowserStartQualificationReport {
        saved(x.f.engine.handle(x.import.clone()).result.unwrap())
    }
    fn accept(x: &BrowserFixture, value: &HandyBrowserStartQualificationReport) -> Request {
        request(
            &x.f.session,
            Some(&x.f.project),
            Some(2),
            Command::AcceptHandyBrowserStartQualification {
                dossier_id: value.dossier_id.clone(),
                expected_record_revision: value.revision,
                operator: "simulated unit-test operator; no real acceptance".into(),
                acknowledge_measured_evidence: true,
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        )
    }
    fn withdraw(x: &BrowserFixture, value: &HandyBrowserStartQualificationReport) -> Request {
        request(
            &x.f.session,
            Some(&x.f.project),
            None,
            Command::WithdrawHandyBrowserStartQualification {
                dossier_id: value.dossier_id.clone(),
                expected_record_revision: value.revision,
                reason: "withdraw simulated unit-test applicability".into(),
                owner_proof: x.f.engine.handy_owner_token.clone(),
            },
        )
    }
    fn status(
        x: &BrowserFixture,
        value: &HandyBrowserStartQualificationReport,
    ) -> PResult<ResponseBody> {
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::HandyBrowserStartQualificationStatus {
                    dossier_id: value.dossier_id.clone(),
                    owner_proof: x.f.engine.handy_owner_token.clone(),
                },
            ))
            .result
    }
    fn capture(
        x: &BrowserFixture,
        value: &HandyBrowserStartQualificationReport,
    ) -> PResult<AcceptedBrowserStart> {
        let r = request(
            &x.f.session,
            Some(&x.f.project),
            Some(2),
            Command::GetSnapshot,
        );
        let db = x.f.engine.db.lock().unwrap();
        x.f.engine.capture_handy_browser_start_locked(
            &db,
            &r,
            &HandyBrowserStartQualificationBinding {
                dossier_id: value.dossier_id.clone(),
                revision: value.revision,
            },
            &x.media,
            &x.hosting,
            &x.input.subject.device_qualification,
        )
    }
    fn accepted(
        x: &BrowserFixture,
    ) -> (
        Request,
        HandyBrowserStartQualificationReport,
        AcceptedBrowserStart,
    ) {
        let pending = imported(x);
        assert!(capture(x, &pending).is_err());
        let command = accept(x, &pending);
        let value = saved(x.f.engine.handle(command.clone()).result.unwrap());
        let pin = capture(x, &value).unwrap();
        assert!(pin.current());
        assert_eq!(pin.script_quantization(), &x.script.quantization);
        assert_eq!(pin.subject(), &x.input.subject);
        (command, value, pin)
    }

    #[test]
    fn browser_dossier_import_retains_immutable_payloads_and_synthetic_stays_pending() {
        let x = fixture_browser(HandyEvidenceOrigin::Synthetic);
        let value = imported(&x);
        assert_eq!(value.state, HandyQualificationState::Pending);
        for item in &x.input.evidence {
            fs::remove_file(&item.path).unwrap();
        }
        let Command::ImportHandyBrowserStartQualification { path, .. } = &x.import.command else {
            panic!();
        };
        fs::remove_file(path).unwrap();
        assert!(status(&x, &value).is_ok());
        let replay = saved(x.f.engine.handle(x.import.clone()).result.unwrap());
        assert_eq!(
            serde_json::to_vec(&replay).unwrap(),
            serde_json::to_vec(&value).unwrap()
        );
        assert_eq!(
            x.f.engine
                .handle(accept(&x, &value))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert!(capture(&x, &value).is_err());
        dossier::validate(&x.f.engine.db.lock().unwrap()).unwrap();
    }
    #[test]
    fn browser_dossier_acceptance_withdrawal_and_reaccept_never_revive_old_pin() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (original, first, pin) = accepted(&x);
        let withdrawal = withdraw(&x, &first);
        let withdrawn = saved(x.f.engine.handle(withdrawal.clone()).result.unwrap());
        assert!(!pin.current());
        assert!(capture(&x, &first).is_err());
        assert_eq!(
            saved(x.f.engine.handle(original).result.unwrap()).revision,
            first.revision
        );
        assert!(!pin.current());
        let newer = saved(x.f.engine.handle(accept(&x, &withdrawn)).result.unwrap());
        let new_pin = capture(&x, &newer).unwrap();
        assert!(new_pin.current());
        assert!(!pin.current());
        // Both original replay and a fresh stale withdrawal are historical only.
        x.f.engine.handle(withdrawal).result.unwrap();
        assert_eq!(
            x.f.engine
                .handle(withdraw(&x, &first))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert!(new_pin.current());
        let db = x.f.engine.db.lock().unwrap();
        dossier::revalidate_accepted_browser_start(&db, &x.f.engine.pool, &new_pin).unwrap();
        assert!(dossier::revalidate_accepted_browser_start(&db, &x.f.engine.pool, &pin).is_err());
    }
    #[test]
    fn browser_dossier_changed_payload_refuses_use_but_allows_withdrawal() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (_, value, pin) = accepted(&x);
        {
            let db = x.f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE handy_browser_start_qualification_evidence SET data=x'00' WHERE dossier=?1",
                [value.dossier_id.as_str()],
            )
            .unwrap();
            assert!(
                dossier::revalidate_accepted_browser_start(&db, &x.f.engine.pool, &pin).is_err()
            );
            assert!(!pin.current());
            assert!(dossier::validate(&db).is_err());
        }
        assert!(status(&x, &value).is_err());
        assert!(capture(&x, &value).is_err());
        let withdrawn = saved(x.f.engine.handle(withdraw(&x, &value)).result.unwrap());
        assert_eq!(withdrawn.state, HandyQualificationState::Withdrawn);
        assert!(!pin.current());
    }
    #[test]
    fn browser_dossier_corrupt_decision_receipt_cannot_prevent_exact_native_retirement() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (original, value, pin) = accepted(&x);
        x.f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "UPDATE requests SET response='{}' WHERE session=?1 AND request=?2",
                params![x.f.session.as_str(), original.request_id.as_str()],
            )
            .unwrap();
        assert!(x.f.engine.handle(withdraw(&x, &value)).result.is_err());
        assert!(!pin.current());
        assert!(capture(&x, &value).is_err());
    }
    #[test]
    fn browser_dossier_failed_withdrawal_receipt_fences_existing_and_new_users() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (_, value, pin) = accepted(&x);
        let mut withdrawal = withdraw(&x, &value);
        withdrawal.request_id = RequestId::new("browser-failed-withdraw").unwrap();
        x.f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER refuse_browser_review BEFORE INSERT ON requests WHEN NEW.request='browser-failed-withdraw' BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;").unwrap();
        assert!(x.f.engine.handle(withdrawal).result.is_err());
        assert!(!pin.current());
        assert!(capture(&x, &value).is_err());
        let ResponseBody::HandyBrowserStartQualification(current) = status(&x, &value).unwrap()
        else {
            panic!();
        };
        assert_eq!(current.revision, value.revision);
        assert_eq!(current.state, HandyQualificationState::Accepted);
        x.f.engine
            .db
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER refuse_browser_review")
            .unwrap();
        let newer = saved(x.f.engine.handle(accept(&x, &current)).result.unwrap());
        assert!(capture(&x, &newer).unwrap().current());
        assert!(!pin.current());
    }
    #[test]
    fn browser_dossier_failed_acceptance_receipt_does_not_publish_a_decision() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let pending = imported(&x);
        let mut command = accept(&x, &pending);
        command.request_id = RequestId::new("browser-failed-accept").unwrap();
        x.f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER refuse_browser_review BEFORE INSERT ON requests WHEN NEW.request='browser-failed-accept' BEGIN SELECT RAISE(ABORT,'synthetic receipt failure'); END;").unwrap();
        assert!(x.f.engine.handle(command).result.is_err());
        let ResponseBody::HandyBrowserStartQualification(current) = status(&x, &pending).unwrap()
        else {
            panic!();
        };
        assert_eq!(current.revision, 1);
        assert_eq!(current.state, HandyQualificationState::Pending);
        assert!(capture(&x, &current).is_err());
    }
    #[test]
    fn browser_dossier_import_refuses_changed_inputs_foreign_scripts_and_excess_budget() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let mut changed = x.input.clone();
        changed.subject.timing.maximum_alignment_error_ns = x
            .hardware
            .manifest
            .subject
            .profile
            .maximum_clock_uncertainty_ns
            + 1;
        assert!(x
            .f
            .engine
            .handle(import_request(&x.f, "browser-excess-budget", &changed))
            .result
            .is_err());
        changed = x.input.clone();
        changed.subject.script_request_id = RequestId::new("nonexistent-script").unwrap();
        assert!(x
            .f
            .engine
            .handle(import_request(&x.f, "browser-foreign-script", &changed))
            .result
            .is_err());
        changed = x.input.clone();
        changed.subject.device_qualification.revision += 1;
        assert!(x
            .f
            .engine
            .handle(import_request(&x.f, "browser-stale-hardware", &changed))
            .result
            .is_err());
        changed = x.input.clone();
        changed.subject.context.platform.push_str(" changed");
        assert!(x
            .f
            .engine
            .handle(import_request(&x.f, "browser-changed-context", &changed))
            .result
            .is_err());
        fs::write(&x.input.evidence[1].path, b"changed external bytes").unwrap();
        assert!(x.f.engine.handle(x.import.clone()).result.is_err());
        let db = x.f.engine.db.lock().unwrap();
        let count: u64 = db.query_row("SELECT count(*) FROM sqlite_master WHERE name='handy_browser_start_qualifications'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn browser_dossier_acceptance_requires_owner_consent_current_revision_and_script() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let pending = imported(&x);
        let mut no_owner = accept(&x, &pending);
        let Command::AcceptHandyBrowserStartQualification { owner_proof, .. } =
            &mut no_owner.command
        else {
            panic!();
        };
        *owner_proof = "0".repeat(64);
        assert!(x.f.engine.handle(no_owner).result.is_err());
        let mut no_consent = accept(&x, &pending);
        let Command::AcceptHandyBrowserStartQualification {
            acknowledge_measured_evidence,
            ..
        } = &mut no_consent.command
        else {
            panic!();
        };
        *acknowledge_measured_evidence = false;
        assert_eq!(
            x.f.engine.handle(no_consent).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let mut old = accept(&x, &pending);
        old.expected_revision = Some(RevisionId::new(1));
        assert!(x.f.engine.handle(old).result.is_err());
        fs::write(&x.script.path, b"changed prepared CSV").unwrap();
        assert!(x.f.engine.handle(accept(&x, &pending)).result.is_err());
    }
    #[test]
    fn browser_dossier_public_source_release_retires_native_pin_and_replay_stays_history() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (original, value, pin) = accepted(&x);
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                None,
                Command::ReleaseBrowserMedia {
                    original_request_id: x.preparation.request_id.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!pin.current());
        assert!(capture(&x, &value).is_err());
        x.f.engine.handle(original).result.unwrap();
        assert!(!pin.current());
    }
    #[test]
    fn browser_dossier_hardware_withdrawal_retires_native_pin_before_return() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (_, value, pin) = accepted(&x);
        x.f.engine
            .handle(qualification_withdraw(
                &x.f,
                &x.hardware.dossier_id,
                x.hardware.revision,
            ))
            .result
            .unwrap();
        assert!(!pin.current());
        assert!(capture(&x, &value).is_err());
    }
    #[test]
    fn browser_dossier_project_placement_and_grant_mutations_retire_native_pins() {
        for revoke in [false, true] {
            let x = fixture_browser(HandyEvidenceOrigin::Physical);
            let (_, _, pin) = accepted(&x);
            let change = if revoke {
                request(
                    &x.f.session,
                    None,
                    None,
                    Command::Revoke {
                        session: x.f.session.clone(),
                        project_id: x.f.project.clone(),
                    },
                )
            } else {
                request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(2),
                    Command::RemoveSourcePlacement {
                        placement_id: x.media.selection.placement.placement_id.clone(),
                    },
                )
            };
            x.f.engine.handle(change).result.unwrap();
            assert!(!pin.current());
        }
    }
    #[test]
    fn browser_dossier_layout_corruption_is_refused_and_shutdown_retires_pins() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (_, _, pin) = accepted(&x);
        x.f.engine
            .db
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE handy_browser_start_qualification_evidence")
            .unwrap();
        assert!(dossier::validate(&x.f.engine.db.lock().unwrap()).is_err());
        let config = Engine::retire_for_test(x.f.engine);
        assert!(!pin.current());
        assert!(Engine::open(config).is_err());
    }
    #[test]
    fn browser_dossier_changed_script_revalidation_permanently_retires_captured_pin() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (_, value, pin) = accepted(&x);
        let original = fs::read(&x.script.path).unwrap();
        fs::write(&x.script.path, b"changed script bytes").unwrap();
        {
            let db = x.f.engine.db.lock().unwrap();
            assert!(
                dossier::revalidate_accepted_browser_start(&db, &x.f.engine.pool, &pin).is_err()
            );
        }
        assert!(!pin.current());
        fs::write(&x.script.path, original).unwrap();
        {
            let db = x.f.engine.db.lock().unwrap();
            assert!(
                dossier::revalidate_accepted_browser_start(&db, &x.f.engine.pool, &pin).is_err()
            );
        }
        assert!(!pin.current());
        // A distinct deliberate capture may use restored current artifacts; it
        // cannot repair an already-retired user's original lifetime.
        assert!(capture(&x, &value).unwrap().current());
        assert!(!pin.current());
    }
    #[test]
    fn browser_dossier_restart_preserves_acceptance_history_without_native_authority() {
        let x = fixture_browser(HandyEvidenceOrigin::Physical);
        let (original, value, pin) = accepted(&x);
        let BrowserFixture {
            f:
                Fixture {
                    engine,
                    session,
                    project,
                    dir,
                },
            media,
            hosting,
            input,
            ..
        } = x;
        let config = Engine::retire_for_test(engine);
        assert!(!pin.current());
        let reopened = Engine::open(config).unwrap();
        let replay = saved(reopened.handle(original).result.unwrap());
        assert_eq!(encode(&replay).unwrap(), encode(&value).unwrap());
        let r = request(&session, Some(&project), Some(2), Command::GetSnapshot);
        {
            let db = reopened.db.lock().unwrap();
            assert!(reopened
                .capture_handy_browser_start_locked(
                    &db,
                    &r,
                    &HandyBrowserStartQualificationBinding {
                        dossier_id: value.dossier_id,
                        revision: value.revision
                    },
                    &media,
                    &hosting,
                    &input.subject.device_qualification
                )
                .is_err());
            assert!(dossier::revalidate_accepted_browser_start(&db, &reopened.pool, &pin).is_err());
        }
        Engine::retire_for_test(reopened);
        drop(dir);
    }
}
