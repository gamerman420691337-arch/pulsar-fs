// Synthetic callback clocks and authored bytes prove native custody, not display
// calibration, submitted frame provenance, model accuracy or device alignment.
#[cfg(target_os = "linux")]
mod browser_timing_tests {
    use super::*;

    struct TimingFixture {
        f: Fixture,
        media: BrowserMediaBinding,
        open: Request,
        base: u64,
    }
    fn fixture_timing() -> TimingFixture {
        let f = fixture();
        let path = f.dir.path().join("browser-timing-source.bin");
        fs::write(&path, b"authored browser timing bytes").unwrap();
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
            panic!("source")
        };
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::SetSourcePlacement {
                    spec: SourcePlacementSpec {
                        source_version,
                        source_start: SourceTimestamp::new(0, 1).unwrap(),
                        source_end: SourceTimestamp::new(10, 1).unwrap(),
                        source_anchor: SourceTimestamp::new(1, 3).unwrap(),
                        project_anchor: ProjectTime::from_nanos(-1),
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
                Some(1),
                Command::BrowserMediaCatalog,
            ))
            .result
            .unwrap()
        else {
            panic!("catalog")
        };
        let prepare = request(
            &f.session,
            Some(&f.project),
            Some(1),
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
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
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
                panic!("media preparation")
            };
            if report.state == BrowserMediaPreparationState::Ready {
                assert!(report.current);
                break report.binding.unwrap();
            }
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        let player = HandyPlayerIdentity {
            launch_id: AttemptId::new("timing-launch").unwrap(),
            page_id: AttemptId::new("timing-page").unwrap(),
            player_id: AttemptId::new("timing-player").unwrap(),
            source_id: AttemptId::new("timing-source").unwrap(),
            generation: 1,
            time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
        };
        let mut open = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::OpenBrowserTiming {
                player,
                browser_media: media.clone(),
                policy: BrowserTimingPolicy {
                    probe_timeout_ms: 100,
                    observation_timeout_ms: 500,
                },
            },
        );
        open.request_id = RequestId::new("open-browser-timing").unwrap();
        let base = f.engine.browser_timing_clock_for_test();
        TimingFixture {
            f,
            media,
            open,
            base,
        }
    }
    fn report(body: ResponseBody) -> BrowserTimingReport {
        let ResponseBody::BrowserTiming(report) = body else {
            panic!("timing report")
        };
        report.validate().unwrap();
        *report
    }
    fn probe(body: ResponseBody) -> BrowserTimingProbe {
        let ResponseBody::BrowserTimingProbe(probe) = body else {
            panic!("timing probe")
        };
        probe.validate().unwrap();
        *probe
    }
    fn opened(x: &TimingFixture) -> BrowserTimingReport {
        report(x.f.engine.handle(x.open.clone()).result.unwrap())
    }
    fn command(x: &TimingFixture, command: Command) -> Request {
        request(&x.f.session, Some(&x.f.project), None, command)
    }
    fn issue(x: &TimingFixture, binding: &BrowserTimingBinding, sequence: u64) -> Request {
        command(
            x,
            Command::IssueBrowserTimingProbe {
                binding_id: binding.binding_id.clone(),
                player: binding.player.clone(),
                sequence,
            },
        )
    }
    fn status(x: &TimingFixture, binding: &BrowserTimingBinding) -> Request {
        command(
            x,
            Command::BrowserTimingStatus {
                binding_id: binding.binding_id.clone(),
                player: binding.player.clone(),
            },
        )
    }
    fn release(x: &TimingFixture, binding: &BrowserTimingBinding) -> Request {
        command(
            x,
            Command::ReleaseBrowserTiming {
                binding_id: binding.binding_id.clone(),
                player: binding.player.clone(),
            },
        )
    }
    fn history(x: &TimingFixture) -> Request {
        command(
            x,
            Command::BrowserTimingOpenStatus {
                original_request_id: x.open.request_id.clone(),
            },
        )
    }
    fn frame(sequence: u64) -> BrowserTimingFrame {
        BrowserTimingFrame {
            sample: HandyPlayerSample {
                browser_tick_us: 20_000 + sequence * 10_000,
                media_time_us: 5_400_000 + sequence * 100_000,
                presented_frames: sequence,
                playing: true,
                seeking: false,
                ended: false,
                visible: true,
                playback_rate_milli: 1000,
            },
            callback_tick_us: 19_000 + sequence * 10_000,
            presentation_time_us: Some(18_000 + sequence * 10_000),
            expected_display_time_us: Some(25_000 + sequence * 10_000),
        }
    }
    fn observe(
        x: &TimingFixture,
        probe: &BrowserTimingProbe,
        frame: BrowserTimingFrame,
    ) -> Request {
        command(
            x,
            Command::ObserveBrowserTiming {
                binding_id: probe.binding_id.clone(),
                player: probe.player.clone(),
                sequence: probe.sequence,
                probe_id: probe.probe_id.clone(),
                frame,
            },
        )
    }
    fn at(x: &TimingFixture, offset: u64, r: Request) -> PResult<ResponseBody> {
        x.f.engine
            .set_browser_timing_clock_for_test(x.base + offset);
        x.f.engine.handle(r).result
    }
    fn first(
        x: &TimingFixture,
        binding: &BrowserTimingBinding,
    ) -> (BrowserTimingProbe, BrowserTimingReport) {
        let p = probe(at(x, 10_000_000, issue(x, binding, 1)).unwrap());
        let value = report(at(x, 20_000_000, observe(x, &p, frame(1))).unwrap());
        (p, value)
    }

    #[test]
    fn browser_timing_observation_keeps_whole_bracket_exact_phase_and_inert_open_history() {
        let x = fixture_timing();
        let initial = opened(&x);
        assert!(initial.current);
        assert!(!initial.qualified);
        assert!(initial.observation.is_none());
        let p = probe(at(&x, 10_000_000, issue(&x, &initial.binding, 1)).unwrap());
        let repeated = probe(at(&x, 15_000_000, issue(&x, &initial.binding, 1)).unwrap());
        assert_eq!(p, repeated);
        let observed = report(at(&x, 20_000_000, observe(&x, &p, frame(1))).unwrap());
        let o = observed.observation.as_ref().unwrap();
        assert_eq!(o.bracket.native_issue_ns, x.base + 10_000_000);
        assert_eq!(o.bracket.native_receive_ns, x.base + 20_000_000);
        assert_eq!(o.bracket.browser_sample_us, 30_000);
        assert_eq!(
            o.project_phase,
            ExactSeconds::new(499_999_997, 3_000_000_000).unwrap()
        );
        assert_eq!(o.expires_at_boottime_ns, x.base + 510_000_000);
        assert_eq!(o.frame.presentation_time_us, Some(28_000));
        assert_eq!(o.frame.expected_display_time_us, Some(35_000));
        let replay = report(at(&x, 30_000_000, x.open.clone()).unwrap());
        assert_eq!(replay, observed);
        let recovered = report(x.f.engine.handle(history(&x)).result.unwrap());
        assert_eq!(recovered, observed);
        let db = x.f.engine.db.lock().unwrap();
        let raw: String = db
            .query_row(
                "SELECT response FROM requests WHERE session=?1 AND request=?2",
                params![x.f.session.as_str(), x.open.request_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let stored = report(decode(&raw).unwrap());
        assert!(
            stored.observation.is_none(),
            "dynamic observations must stay boot-local"
        );
        drop(db);
        assert_eq!(x.f.engine.browser_timing_retention_for_test(), (1, 1));
        let retired = report(at(&x, 510_000_000, status(&x, &initial.binding)).unwrap());
        assert!(!retired.current);
        assert_eq!(retired.observation, observed.observation);
        assert_eq!(x.f.engine.browser_timing_retention_for_test(), (1, 0));
        assert!(!report(x.f.engine.handle(x.open.clone()).result.unwrap()).current);
    }
    #[test]
    fn browser_timing_old_nonce_sequence_and_player_messages_cannot_retire_successors() {
        let x = fixture_timing();
        let initial = opened(&x);
        let (old, _) = first(&x, &initial.binding);
        let p = probe(at(&x, 40_000_000, issue(&x, &initial.binding, 2)).unwrap());
        assert!(at(&x, 50_000_000, observe(&x, &old, frame(1))).is_err());
        let duplicate = probe(
            x.f.engine
                .handle(issue(&x, &initial.binding, 2))
                .result
                .unwrap(),
        );
        assert_eq!(duplicate, p);
        let latest = report(at(&x, 60_000_000, observe(&x, &p, frame(2))).unwrap());
        for case in 0..5 {
            let mut stale = observe(&x, &old, frame(1));
            match case {
                0 => {
                    if let Command::ObserveBrowserTiming {
                        sequence, probe_id, ..
                    } = &mut stale.command
                    {
                        *sequence = 3;
                        *probe_id = p.probe_id.clone();
                    }
                }
                1 => stale.auth_token = Some("wrong-session-authentication".into()),
                2 => stale.project = Some(ProjectId::new("foreign-project").unwrap()),
                3 => {
                    if let Command::ObserveBrowserTiming { player, .. } = &mut stale.command {
                        player.generation += 1;
                    }
                }
                _ => {
                    if let Command::ObserveBrowserTiming { binding_id, .. } = &mut stale.command {
                        *binding_id = ArtifactId::new("foreign-binding").unwrap();
                    }
                }
            }
            assert!(at(&x, 120_000_000, stale).is_err());
            assert_eq!(
                report(
                    x.f.engine
                        .handle(status(&x, &initial.binding))
                        .result
                        .unwrap()
                ),
                latest
            );
        }
        let p3 = probe(
            x.f.engine
                .handle(issue(&x, &initial.binding, 3))
                .result
                .unwrap(),
        );
        assert!(x.f.engine.handle(observe(&x, &p, frame(2))).result.is_err());
        assert_eq!(
            probe(
                x.f.engine
                    .handle(issue(&x, &initial.binding, 3))
                    .result
                    .unwrap()
            ),
            p3
        );
        assert!(
            !report(at(&x, 220_000_000, status(&x, &initial.binding)).unwrap()).current,
            "exact current probe expires independently"
        );
    }
    #[test]
    fn browser_timing_invalid_lifecycle_and_nonadvancing_frame_retire_only_exact_scope() {
        for case in 0..10 {
            let x = fixture_timing();
            let initial = opened(&x);
            first(&x, &initial.binding);
            let p = probe(at(&x, 30_000_000, issue(&x, &initial.binding, 2)).unwrap());
            let mut invalid = frame(2);
            match case {
                0 => invalid.sample.playing = false,
                1 => invalid.sample.visible = false,
                2 => invalid.sample.seeking = true,
                3 => invalid.sample.ended = true,
                4 => invalid.sample.playback_rate_milli = 1500,
                5 => invalid.sample.presented_frames = 0,
                6 => {
                    invalid.sample.browser_tick_us = 30_000;
                    invalid.callback_tick_us = 29_000;
                }
                7 => invalid.sample.presented_frames = 1,
                8 => invalid.sample.media_time_us = 5_500_000,
                _ => invalid.sample.media_time_us = 15_000_000,
            }
            invalid.validate().unwrap();
            assert!(
                !report(at(&x, 40_000_000, observe(&x, &p, invalid)).unwrap()).current,
                "case {case}"
            );
            assert_eq!(x.f.engine.browser_timing_retention_for_test().1, 0);
            assert!(x
                .f
                .engine
                .handle(issue(&x, &initial.binding, 2))
                .result
                .is_err());
        }
    }
    #[test]
    fn browser_timing_probe_expiry_does_not_extend_initial_liveness_or_revive_observation() {
        let x = fixture_timing();
        let initial = opened(&x);
        let p = probe(at(&x, 450_000_000, issue(&x, &initial.binding, 1)).unwrap());
        assert_eq!(p.expires_at_boottime_ns, x.base + 550_000_000);
        assert!(
            at(&x, 500_000_000, observe(&x, &p, frame(1))).is_err(),
            "initial liveness expires even while a probe remains structurally in range"
        );
        assert!(
            !report(
                x.f.engine
                    .handle(status(&x, &initial.binding))
                    .result
                    .unwrap()
            )
            .current
        );
        assert!(!report(x.f.engine.handle(history(&x)).result.unwrap()).current);
        assert!(!report(x.f.engine.handle(x.open.clone()).result.unwrap()).current);
    }
    #[test]
    fn browser_timing_native_clock_reversal_retires_and_refuses_new_authority() {
        let x = fixture_timing();
        let initial = opened(&x);
        first(&x, &initial.binding);
        let value = report(at(&x, 19_000_000, status(&x, &initial.binding)).unwrap());
        assert!(!value.current);
        assert_eq!(x.f.engine.browser_timing_retention_for_test().1, 0);
        let mut fresh = x.open.clone();
        fresh.request_id = RequestId::new("after-timing-clock-reversal").unwrap();
        assert!(x.f.engine.handle(fresh).result.is_err());
        assert!(!report(x.f.engine.handle(history(&x)).result.unwrap()).current);
    }
    #[test]
    fn browser_timing_hsp_claim_samples_under_guard_and_preserves_newer_snapshot() {
        let x = fixture_timing();
        let initial = opened(&x);
        let (_, first_report) = first(&x, &initial.binding);
        let pin =
            x.f.engine
                .browser_timing_scope(&status(&x, &initial.binding), &initial.binding.binding_id)
                .unwrap();
        let pre_lock_tick = x.base + 20_000_000;
        assert_eq!(
            first_report.observation.unwrap().bracket.native_receive_ns,
            pre_lock_tick
        );

        // Causal interleaving: an old caller evaluation exists, then a complete
        // native callback advances the registry before the final claim locks it.
        let next = probe(at(&x, 30_000_000, issue(&x, &initial.binding, 2)).unwrap());
        let observed = report(at(&x, 40_000_000, observe(&x, &next, frame(2))).unwrap());
        let observation = observed.observation.unwrap();
        assert!(pre_lock_tick < observation.bracket.native_receive_ns);
        assert!(pin.try_snapshot(pre_lock_tick).is_err());
        assert!(
            pin.current(x.base + 40_000_000),
            "a newer observation must not retire the native lifetime"
        );
        assert_eq!(pin.try_snapshot(x.base + 40_000_000).unwrap(), observation);

        x.f.engine
            .set_browser_timing_clock_for_test(x.base + 50_000_000);
        let sampled = AtomicBool::new(false);
        let claim = pin
            .try_claim(|| {
                assert!(
                    matches!(
                        x.f.engine.browser_timing.try_lock(),
                        Err(std::sync::TryLockError::WouldBlock)
                    ),
                    "clock sampled before registry lock"
                );
                sampled.store(true, Ordering::Release);
                Ok(x.f.engine.browser_timing_test_tick.load(Ordering::Acquire))
            })
            .unwrap();
        assert!(sampled.load(Ordering::Acquire));
        assert_eq!(claim.admitted_at_ns, x.base + 50_000_000);
        assert_eq!(
            claim.observation, observation,
            "under-lock evaluation must preserve original observation and expiry"
        );
        drop(claim);
        assert!(
            report(
                x.f.engine
                    .handle(status(&x, &initial.binding))
                    .result
                    .unwrap()
            )
            .current
        );

        // An actual regression sampled under the same guard still permanently
        // retires timing. No age, observation or deadline is replaced.
        assert!(pin
            .try_claim(|| {
                assert!(matches!(
                    x.f.engine.browser_timing.try_lock(),
                    Err(std::sync::TryLockError::WouldBlock)
                ));
                // Only this held-guard sample regresses. The background
                // sweeper's test clock stays at 50ms, so it cannot cause this.
                Ok(x.base + 49_000_000)
            })
            .is_err());
        assert!(!pin.current(x.base + 50_000_000));
        assert!(!report(x.f.engine.handle(history(&x)).result.unwrap()).current);
        let mut fresh = x.open.clone();
        fresh.request_id = RequestId::new("after-hsp-claim-clock-regression").unwrap();
        assert!(x.f.engine.handle(fresh).result.is_err());
    }

    fn native_pin(
        x: &TimingFixture,
        binding: &BrowserTimingBinding,
    ) -> crate::authority::browser_timing::BrowserTimingPin {
        x.f.engine
            .browser_timing_scope(&status(x, binding), &binding.binding_id)
            .unwrap()
    }
    fn native_claim_at<'a>(
        pin: &'a crate::authority::browser_timing::BrowserTimingPin,
        at: u64,
    ) -> std::result::Result<
        crate::authority::browser_timing::BrowserTimingClaim<'a>,
        crate::authority::browser_timing::BrowserTimingClaimFailure,
    > {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        let end = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match pin.try_claim_native(|| Ok(at)) {
                Err(BrowserTimingClaimFailure::Contended) if std::time::Instant::now() < end => {
                    std::thread::sleep(std::time::Duration::from_millis(1))
                }
                outcome => return outcome,
            }
        }
    }
    #[test]
    fn native_timing_registry_contention_does_not_sample_clock_retire_or_renew() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        let x = fixture_timing();
        let initial = opened(&x);
        let (_, observed) = first(&x, &initial.binding);
        let original = observed.observation.unwrap();
        let pin = native_pin(&x, &initial.binding);
        let sampled = AtomicBool::new(false);
        let registry = x.f.engine.browser_timing.lock().unwrap();
        assert!(matches!(
            pin.try_claim_native(|| {
                sampled.store(true, Ordering::Release);
                Ok(x.base + 20_000_000)
            }),
            Err(BrowserTimingClaimFailure::Contended)
        ));
        assert!(!sampled.load(Ordering::Acquire));
        assert!(pin.current(x.base + 20_000_000));
        let legacy = pin.try_claim(|| {
            sampled.store(true, Ordering::Release);
            Ok(x.base + 20_000_000)
        });
        assert!(matches!(legacy, Err(ref error) if error.code == ErrorCode::Unavailable));
        assert!(!sampled.load(Ordering::Acquire));
        drop(legacy);
        drop(registry);
        let claim = native_claim_at(&pin, x.base + 20_000_000).unwrap();
        assert_eq!(
            claim.observation_for(&pin, x.base + 20_000_000).unwrap(),
            &original
        );
        drop(claim);
        assert_eq!(pin.latest(x.base + 20_000_000).unwrap(), original);
    }
    #[test]
    fn native_timing_poison_is_refused_without_sampling_or_retry_custody() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        let x = fixture_timing();
        let initial = opened(&x);
        first(&x, &initial.binding);
        let pin = native_pin(&x, &initial.binding);
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _registry = x.f.engine.browser_timing.lock().unwrap();
            panic!("authored timing-registry poison");
        }));
        assert!(poisoned.is_err());
        let sampled = AtomicBool::new(false);
        assert!(matches!(
            pin.try_claim_native(|| {
                sampled.store(true, Ordering::Release);
                Ok(x.base + 20_000_000)
            }),
            Err(BrowserTimingClaimFailure::Refused(_))
        ));
        assert!(!sampled.load(Ordering::Acquire));
    }
    #[test]
    fn native_timing_missing_observation_expiry_wrong_subject_and_regression_are_refusals() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        for case in 0..4 {
            let x = fixture_timing();
            let initial = opened(&x);
            let observation = if case == 0 {
                None
            } else {
                first(&x, &initial.binding).1.observation
            };
            let pin = native_pin(&x, &initial.binding);
            let at = match case {
                0 => x.base + 10_000_000,
                1 => observation.as_ref().unwrap().expires_at_boottime_ns,
                2 => x.base + 30_000_000,
                _ => x.base + 19_000_000,
            };
            x.f.engine.set_browser_timing_clock_for_test(if case == 3 {
                x.base + 20_000_000
            } else {
                at
            });
            let candidate = if case == 2 {
                let mut binding = pin.binding().clone();
                binding.player.generation += 1;
                pin.with_binding_for_test(binding)
            } else {
                pin.clone()
            };
            assert!(
                matches!(
                    native_claim_at(&candidate, at),
                    Err(BrowserTimingClaimFailure::Refused(_))
                ),
                "case {case}"
            );
            if case == 2 {
                assert!(
                    pin.current(at),
                    "a wrong caller must not retire the real owner"
                );
                let valid = native_claim_at(&pin, at).unwrap();
                assert_eq!(
                    valid.observation_for(&pin, at).unwrap(),
                    observation.as_ref().unwrap()
                );
            }
            if case == 1 || case == 3 {
                assert!(!pin.current(at.max(x.base + 30_000_000)));
            }
        }
    }
    #[test]
    fn native_timing_snapshot_reads_while_final_claim_serialization_is_held() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        let x = fixture_timing();
        let initial = opened(&x);
        let (_, observed) = first(&x, &initial.binding);
        let original = observed.observation.unwrap();
        let pin = native_pin(&x, &initial.binding);
        let at = x.base + 30_000_000;
        x.f.engine.set_browser_timing_clock_for_test(at);
        let held = pin.hold_registry_for_test();
        let worker_pin = pin.clone();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let task = std::thread::spawn(move || {
            assert!(matches!(
                worker_pin.try_claim_native(|| panic!("contended claim sampled clock")),
                Err(BrowserTimingClaimFailure::Contended)
            ));
            let _ = send.send(worker_pin.try_snapshot_native(at));
        });
        let outcome = receive.recv_timeout(std::time::Duration::from_secs(2));
        drop(held);
        task.join().unwrap();
        assert_eq!(
            outcome
                .expect("snapshot waited for publication serialization")
                .unwrap(),
            original
        );
        let claim = native_claim_at(&pin, at).unwrap();
        let native = claim.observation_for(&pin, at).unwrap();
        assert!(
            !std::ptr::eq(native, &claim.observation),
            "claim must borrow the stored native record"
        );
        assert_eq!(native, &original);
        drop(claim);
        assert_eq!(pin.latest(at).unwrap(), original);
    }
    #[test]
    fn native_timing_claim_rejects_foreign_lifetimes_and_tampered_public_copies() {
        let x = fixture_timing();
        let initial = opened(&x);
        let (_, observed) = first(&x, &initial.binding);
        let original = observed.observation.unwrap();
        let pin = native_pin(&x, &initial.binding);
        let mut second_open = x.open.clone();
        second_open.request_id = RequestId::new("second-native-timing").unwrap();
        if let Command::OpenBrowserTiming { player, .. } = &mut second_open.command {
            player.generation += 1;
        }
        let second = report(x.f.engine.handle(second_open).result.unwrap());
        let other_lifetime = native_pin(&x, &second.binding);
        let forged_lifetime = other_lifetime.with_binding_for_test(initial.binding.clone());
        let y = fixture_timing();
        let other = opened(&y);
        first(&y, &other.binding);
        let other_registry = native_pin(&y, &other.binding);
        let forged_registry = other_registry.with_binding_for_test(initial.binding.clone());
        let at = x.base + 30_000_000;
        x.f.engine.set_browser_timing_clock_for_test(at);
        let mut claim = native_claim_at(&pin, at).unwrap();
        assert!(claim.observation_for(&other_lifetime, at).is_err());
        assert!(claim.observation_for(&forged_lifetime, at).is_err());
        assert!(claim.observation_for(&forged_registry, at).is_err());
        claim.admitted_at_ns += 1;
        assert!(claim.observation_for(&pin, at + 1).is_err());
        claim.admitted_at_ns = at;
        claim.observation.expires_at_boottime_ns += 1_000_000;
        assert!(claim.observation_for(&pin, at).is_err());
        assert!(claim
            .observation_for(&pin, original.expires_at_boottime_ns)
            .is_err());
        claim.observation = original.clone();
        claim.observation.project_phase = ExactSeconds::new(99, 1).unwrap();
        assert!(claim.observation_for(&pin, at).is_err());
        claim.observation = original.clone();
        assert!(claim.observation_for(&pin, at - 1).is_err());
        assert_eq!(
            claim
                .observation_for(&pin, original.expires_at_boottime_ns - 1)
                .unwrap(),
            &original
        );
        assert!(claim
            .observation_for(&pin, original.expires_at_boottime_ns)
            .is_err());
        drop(claim);
        assert_eq!(
            pin.latest(at).unwrap(),
            original,
            "copied fields did not alter native observation or expiry"
        );
    }

    #[test]
    fn browser_timing_open_does_not_wait_for_unrelated_live_registry() {
        let x = fixture_timing();
        let engine = x.f.engine.clone();
        let request = x.open.clone();
        let held = x.f.engine.live_captures.lock().unwrap();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let task = std::thread::spawn(move || {
            let _ = send.send(engine.handle(request));
        });
        let outcome = receive.recv_timeout(std::time::Duration::from_secs(1));
        drop(held);
        task.join().unwrap();
        let response = outcome.expect("timing open waited for unrelated live-capture sweep");
        assert!(matches!(response.result, Ok(ResponseBody::BrowserTiming(_))));
    }

    #[test]
    fn browser_timing_fast_exchange_status_and_release_do_not_wait_for_database() {
        let x = fixture_timing();
        let initial = opened(&x);
        x.f.engine
            .set_browser_timing_clock_for_test(x.base + 10_000_000);
        let issue = issue(&x, &initial.binding, 1);
        let status = status(&x, &initial.binding);
        let release = release(&x, &initial.binding);
        let engine = x.f.engine.clone();
        let base = x.base;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let db = x.f.engine.db.lock().unwrap();
        let task = std::thread::spawn(move || {
            let p = probe(engine.handle(issue.clone()).result.unwrap());
            engine.set_browser_timing_clock_for_test(base + 20_000_000);
            let mut observe = issue;
            observe.command = Command::ObserveBrowserTiming {
                binding_id: p.binding_id,
                player: p.player,
                sequence: p.sequence,
                probe_id: p.probe_id,
                frame: frame(1),
            };
            let accepted = report(engine.handle(observe).result.unwrap());
            assert!(accepted.current && accepted.observation.is_some());
            assert!(report(engine.handle(status).result.unwrap()).current);
            assert!(!report(engine.handle(release).result.unwrap()).current);
            tx.send(()).unwrap();
        });
        let outcome = rx.recv_timeout(std::time::Duration::from_secs(2));
        drop(db);
        task.join().unwrap();
        outcome.expect("browser timing fast route waited for DB/live sweep");
        assert_eq!(x.f.engine.browser_timing_retention_for_test().1, 0);
    }
    #[test]
    fn browser_timing_owned_sweeper_expires_and_drops_source_pin_with_database_held() {
        let x = fixture_timing();
        let initial = opened(&x);
        let db = x.f.engine.db.lock().unwrap();
        x.f.engine
            .set_browser_timing_clock_for_test(x.base + 500_000_000);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while x.f.engine.browser_timing_retention_for_test().1 != 0
            && std::time::Instant::now() < until
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let retained = x.f.engine.browser_timing_retention_for_test();
        drop(db);
        assert_eq!(
            retained,
            (1, 0),
            "owned expiry worker required client or DB progress"
        );
        assert!(
            !report(
                x.f.engine
                    .handle(status(&x, &initial.binding))
                    .result
                    .unwrap()
            )
            .current
        );
    }
    #[test]
    fn browser_timing_public_source_and_project_retirement_invalidate_before_reply() {
        for event in 0..3 {
            let x = fixture_timing();
            let initial = opened(&x);
            first(&x, &initial.binding);
            let action = match event {
                0 => command(
                    &x,
                    Command::ReleaseBrowserMedia {
                        original_request_id: x.media.preparation_request_id.clone(),
                    },
                ),
                1 => request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(1),
                    Command::RemoveSourcePlacement {
                        placement_id: x.media.selection.placement.placement_id.clone(),
                    },
                ),
                _ => request(
                    &x.f.session,
                    None,
                    None,
                    Command::Revoke {
                        session: x.f.session.clone(),
                        project_id: x.f.project.clone(),
                    },
                ),
            };
            x.f.engine.handle(action).result.unwrap();
            assert_eq!(
                x.f.engine.browser_timing_retention_for_test().1,
                0,
                "event {event}"
            );
            assert!(
                !report(
                    x.f.engine
                        .handle(status(&x, &initial.binding))
                        .result
                        .unwrap()
                )
                .current
            );
            assert!(x
                .f
                .engine
                .handle(issue(&x, &initial.binding, 2))
                .result
                .is_err());
        }
    }
    #[test]
    fn browser_timing_open_receipt_failure_and_capacity_never_replace_current_bindings() {
        let x = fixture_timing();
        x.f.engine.db.lock().unwrap().execute_batch("CREATE TRIGGER refuse_browser_timing BEFORE INSERT ON requests WHEN NEW.request='open-browser-timing' BEGIN SELECT RAISE(ABORT,'synthetic timing receipt failure'); END;").unwrap();
        assert!(x.f.engine.handle(x.open.clone()).result.is_err());
        assert_eq!(x.f.engine.browser_timing_retention_for_test(), (0, 0));
        x.f.engine
            .db
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER refuse_browser_timing")
            .unwrap();
        let original = opened(&x);
        let mut duplicate = x.open.clone();
        duplicate.request_id = RequestId::new("duplicate-player-open").unwrap();
        assert_eq!(
            x.f.engine.handle(duplicate).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let mut reports = vec![original.clone()];
        for n in 1..8 {
            let mut next = x.open.clone();
            next.request_id = RequestId::new(format!("timing-capacity-{n}")).unwrap();
            if let Command::OpenBrowserTiming { player, .. } = &mut next.command {
                player.player_id = AttemptId::new(format!("timing-player-{n}")).unwrap();
            }
            reports.push(report(x.f.engine.handle(next).result.unwrap()));
        }
        let mut ninth = x.open.clone();
        ninth.request_id = RequestId::new("ninth-timing-open").unwrap();
        if let Command::OpenBrowserTiming { player, .. } = &mut ninth.command {
            player.player_id = AttemptId::new("ninth-player").unwrap();
        }
        assert_eq!(
            x.f.engine.handle(ninth.clone()).result.unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        assert_eq!(x.f.engine.browser_timing_retention_for_test(), (8, 8));
        x.f.engine
            .handle(release(&x, &reports[0].binding))
            .result
            .unwrap();
        assert!(report(x.f.engine.handle(ninth).result.unwrap()).current);
        assert!(!report(x.f.engine.handle(x.open.clone()).result.unwrap()).current);
        for live in &reports[1..] {
            assert!(report(x.f.engine.handle(status(&x, &live.binding)).result.unwrap()).current);
        }
    }
    #[test]
    fn browser_timing_lost_open_history_and_restart_never_recreate_native_observation() {
        let x = fixture_timing();
        let original = opened(&x);
        first(&x, &original.binding);
        let query = history(&x);
        let replay = x.open.clone();
        let TimingFixture { f, .. } = x;
        let Fixture { engine, dir, .. } = f;
        let engine = Engine::open(Engine::retire_for_test(engine)).unwrap();
        for request in [query, replay] {
            let historical = report(engine.handle(request).result.unwrap());
            assert!(!historical.current);
            assert!(historical.observation.is_none());
            assert_eq!(historical.binding, original.binding);
        }
        assert_eq!(engine.browser_timing_retention_for_test(), (0, 0));
        let _ = Engine::retire_for_test(engine);
        drop(dir);
    }
    fn successor_media(x: &TimingFixture) -> (Request, BrowserMediaBinding) {
        let r = request(
            &x.f.session,
            Some(&x.f.project),
            Some(x.media.revision.value()),
            Command::PrepareBrowserMediaSuccessor {
                predecessor: x.media.clone(),
                lease_ms: 60_000,
            },
        );
        x.f.engine.handle(r.clone()).result.unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let ResponseBody::BrowserMediaPreparation(value) =
                x.f.engine
                    .handle(command(
                        x,
                        Command::BrowserMediaPreparationStatus {
                            original_request_id: r.request_id.clone(),
                        },
                    ))
                    .result
                    .unwrap()
            else {
                panic!("media report")
            };
            if value.state == BrowserMediaPreparationState::Ready {
                assert!(value.current);
                return (r, value.binding.unwrap());
            }
            assert_eq!(value.state, BrowserMediaPreparationState::Pending);
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    fn successor_timing_request(
        x: &TimingFixture,
        old: &BrowserTimingBinding,
        media: &BrowserMediaBinding,
    ) -> Request {
        request(
            &x.f.session,
            Some(&x.f.project),
            Some(media.revision.value()),
            Command::PrepareBrowserTimingSuccessor {
                predecessor_binding_id: old.binding_id.clone(),
                player: old.player.clone(),
                browser_media: media.clone(),
                policy: old.policy.clone(),
            },
        )
    }
    fn successor_claim_at<'a>(
        old: &'a crate::authority::browser_timing::BrowserTimingPin,
        new: &crate::authority::browser_timing::BrowserTimingPin,
        media: &crate::authority::browser_media::NativeBrowserMediaSuccessor,
        at: u64,
    ) -> Result<
        crate::authority::browser_timing::BrowserTimingSuccessorClaim<'a>,
        crate::authority::browser_timing::BrowserTimingClaimFailure,
    > {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            match old.try_claim_successor_native(new, media, || Ok(at)) {
                Err(crate::authority::browser_timing::BrowserTimingClaimFailure::Contended)
                    if std::time::Instant::now() < end =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(1))
                }
                value => return value,
            }
        }
    }
    #[test]
    fn browser_timing_successor_claim_preserves_original_observations_and_independent_expiry() {
        let x = fixture_timing();
        let old = opened(&x);
        let (_, old_observed) = first(&x, &old.binding);
        let original_observation = old_observed.observation.unwrap();
        let (media_request, media) = successor_media(&x);
        let open = successor_timing_request(&x, &old.binding, &media);
        let new = report(x.f.engine.handle(open.clone()).result.unwrap());
        let old_pin = native_pin(&x, &old.binding);
        let new_pin = native_pin(&x, &new.binding);
        let proof =
            x.f.engine
                .capture_browser_media_successor_locked(
                    &x.f.engine.db.lock().unwrap(),
                    &media_request,
                    &x.media,
                    &media,
                )
                .unwrap();
        assert!(
            successor_claim_at(&old_pin, &new_pin, &proof, x.base + 20_000_000).is_err(),
            "no fabricated child observation"
        );
        let p = probe(at(&x, 30_000_000, issue(&x, &new.binding, 1)).unwrap());
        let observed = report(at(&x, 40_000_000, observe(&x, &p, frame(2))).unwrap());
        let new_observation = observed.observation.unwrap();
        assert!(x
            .f
            .engine
            .handle(successor_timing_request(&x, &old.binding, &media))
            .result
            .is_err());
        let mut ordinary = x.open.clone();
        ordinary.request_id = RequestId::new("ordinary-cannot-replace-player").unwrap();
        assert!(x.f.engine.handle(ordinary).result.is_err());
        let at = x.base + 40_000_000;
        let held = x.f.engine.browser_timing.lock().unwrap();
        let sampled = AtomicBool::new(false);
        assert!(matches!(
            old_pin.try_claim_successor_native(&new_pin, &proof, || {
                sampled.store(true, Ordering::Release);
                Ok(at)
            }),
            Err(crate::authority::browser_timing::BrowserTimingClaimFailure::Contended)
        ));
        assert!(!sampled.load(Ordering::Acquire));
        drop(held);
        let mut claim = successor_claim_at(&old_pin, &new_pin, &proof, at).unwrap();
        let (a, b) = claim.observations_for(&old_pin, &new_pin, at).unwrap();
        assert_eq!(a, &original_observation);
        assert_eq!(b, &new_observation);
        assert!(claim.observations_for(&new_pin, &old_pin, at).is_err());
        let mut forged = new.binding.clone();
        forged.player.generation += 1;
        assert!(claim
            .observations_for(&old_pin, &new_pin.with_binding_for_test(forged), at)
            .is_err());
        assert!(claim
            .observations_for(
                &old_pin,
                &new_pin,
                original_observation.expires_at_boottime_ns
            )
            .is_err());
        proof.commit().unwrap();
        claim.commit(at).unwrap();
        assert!(claim.commit(at).is_err());
        drop(claim);
        assert_eq!(old_pin.latest(at).unwrap(), original_observation);
        let released = report(x.f.engine.handle(release(&x, &old.binding)).result.unwrap());
        assert!(!released.current);
        assert!(!old_pin.current(at));
        assert!(new_pin.current(at));
        // Child's original expiry remains exact, even though its parent retired.
        let expired = report(
            self::at(
                &x,
                new_observation.expires_at_boottime_ns - x.base,
                status(&x, &new.binding),
            )
            .unwrap(),
        );
        assert!(!expired.current);
        assert!(!new_pin.current(new_observation.expires_at_boottime_ns));
        let replay = report(x.f.engine.handle(open).result.unwrap());
        assert!(!replay.current);
        assert_eq!(replay.binding, new.binding);
    }
    #[test]
    fn browser_timing_successor_first_callback_cannot_restart_original_player_counters() {
        let x = fixture_timing();
        let old = opened(&x);
        first(&x, &old.binding);
        let (_, media) = successor_media(&x);
        let open = successor_timing_request(&x, &old.binding, &media);
        let mut changed = open.clone();
        if let Command::PrepareBrowserTimingSuccessor { policy, .. } = &mut changed.command {
            policy.observation_timeout_ms += 1;
        }
        assert!(x.f.engine.handle(changed).result.is_err());
        let new = report(x.f.engine.handle(open).result.unwrap());
        let p = probe(at(&x, 30_000_000, issue(&x, &new.binding, 1)).unwrap());
        assert!(!report(at(&x, 40_000_000, observe(&x, &p, frame(1))).unwrap()).current);
        assert!(report(x.f.engine.handle(status(&x, &old.binding)).result.unwrap()).current);
    }
    #[test]
    fn browser_timing_uncommitted_successor_retires_on_exact_parent_release() {
        let x = fixture_timing();
        let old = opened(&x);
        first(&x, &old.binding);
        let (media_request, media) = successor_media(&x);
        let new = report(
            x.f.engine
                .handle(successor_timing_request(&x, &old.binding, &media))
                .result
                .unwrap(),
        );
        let old_pin = native_pin(&x, &old.binding);
        let new_pin = native_pin(&x, &new.binding);
        let proof =
            x.f.engine
                .capture_browser_media_successor_locked(
                    &x.f.engine.db.lock().unwrap(),
                    &media_request,
                    &x.media,
                    &media,
                )
                .unwrap();
        let p = probe(at(&x, 30_000_000, issue(&x, &new.binding, 1)).unwrap());
        let original = report(at(&x, 40_000_000, observe(&x, &p, frame(2))).unwrap())
            .observation
            .unwrap();
        assert!(new_pin.current(x.base + 40_000_000));
        x.f.engine.handle(release(&x, &old.binding)).result.unwrap();
        assert!(!new_pin.current(x.base + 40_000_000));
        assert!(successor_claim_at(&old_pin, &new_pin, &proof, x.base + 40_000_000).is_err());
        let history = report(x.f.engine.handle(status(&x, &new.binding)).result.unwrap());
        assert!(!history.current);
        assert_eq!(history.observation, Some(original));
    }
    #[test]
    fn native_timing_snapshot_publication_keeps_original_tick_observation_and_deadline() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        let x = fixture_timing();
        let initial = opened(&x);
        let (_, observed) = first(&x, &initial.binding);
        let original = observed.observation.unwrap();
        let pin = native_pin(&x, &initial.binding);
        let now = x.base + 20_000_000;
        // The real publication guard is held. Reads neither acquire it nor
        // sample this deliberately different controlled clock.
        let registry = pin.hold_registry_for_test();
        x.f.engine
            .browser_timing_test_tick
            .store(u64::MAX, Ordering::Release);
        assert_eq!(pin.try_snapshot_native(now).unwrap(), original);
        assert_eq!(pin.try_snapshot(now).unwrap(), original);
        assert_eq!(pin.latest(now).unwrap(), original);
        assert!(pin.current(now));
        assert_eq!(
            x.f.engine.browser_timing_test_tick.load(Ordering::Acquire),
            u64::MAX
        );
        assert!(matches!(
            pin.try_snapshot_native(original.expires_at_boottime_ns),
            Err(BrowserTimingClaimFailure::Refused(_))
        ));
        assert!(matches!(
            pin.try_claim_native(|| panic!("contended claim sampled clock")),
            Err(BrowserTimingClaimFailure::Contended)
        ));
        x.f.engine
            .browser_timing_test_tick
            .store(now, Ordering::Release);
        drop(registry);
        assert_eq!(
            report(
                x.f.engine
                    .handle(status(&x, &initial.binding))
                    .result
                    .unwrap()
            )
            .observation,
            Some(original)
        );
    }
    #[test]
    fn native_timing_snapshot_registry_poison_never_grants_authority_or_retry() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        let x = fixture_timing();
        let initial = opened(&x);
        first(&x, &initial.binding);
        let pin = native_pin(&x, &initial.binding);
        let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _registry = pin.hold_registry_for_test();
            panic!("authored publication-registry poison");
        }));
        assert!(poisoned.is_err());
        assert!(matches!(
            pin.try_snapshot_native(x.base + 20_000_000),
            Err(BrowserTimingClaimFailure::Refused(_))
        ));
        assert!(!pin.current(x.base + 20_000_000));
    }
    #[test]
    fn native_timing_selected_read_precedes_sampling_and_keeps_original_bounds() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        let x = fixture_timing();
        let initial = opened(&x);
        let (_, observed) = first(&x, &initial.binding);
        let original = observed.observation.unwrap();
        let pin = native_pin(&x, &initial.binding);
        let claim = native_claim_at(&pin, x.base + 20_000_000).unwrap();
        let original_start = claim.observation_lifetime_for_test();
        drop(claim);
        let selected = pin.select_read().unwrap();
        let mut samples = 0;
        let mut sample = || {
            samples += 1;
            let p = probe(at(&x, 30_000_000, issue(&x, &initial.binding, 2)).unwrap());
            let next = report(at(&x, 40_000_000, observe(&x, &p, frame(2))).unwrap());
            assert!(next.current);
            x.base + 41_000_000
        };
        let sampled_at = sample();
        assert_eq!(samples, 1);
        assert_eq!(selected.evaluate_at(sampled_at).unwrap(), original);
        assert!(!original_start.load(Ordering::Acquire));
        assert_eq!(original.expires_at_boottime_ns, x.base + 510_000_000);
        let latest = pin.try_snapshot_native(sampled_at).unwrap();
        assert_eq!(latest.sequence, 2);
        assert_eq!(latest.expires_at_boottime_ns, x.base + 530_000_000);
        assert!(matches!(
            pin.try_snapshot_native(latest.bracket.native_receive_ns - 1),
            Err(BrowserTimingClaimFailure::Refused(_))
        ));
        assert!(matches!(
            selected.evaluate_at(original.bracket.native_receive_ns - 1),
            Err(BrowserTimingClaimFailure::Refused(_))
        ));
        assert!(matches!(
            selected.evaluate_at(original.expires_at_boottime_ns),
            Err(BrowserTimingClaimFailure::Refused(_))
        ));
        assert_eq!(
            pin.try_snapshot_native(original.expires_at_boottime_ns)
                .unwrap(),
            latest,
            "a newer publication must not extend the selected original's expiry"
        );
    }
    #[test]
    fn native_timing_selected_read_refuses_source_lifetime_poison_and_clock_retirement() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        for case in 0..4 {
            let x = fixture_timing();
            let initial = opened(&x);
            first(&x, &initial.binding);
            let pin = native_pin(&x, &initial.binding);
            let selected = pin.select_read().unwrap();
            let now = x.base + 20_000_000;
            assert!(selected.evaluate_at(now).is_ok());
            match case {
                0 => {
                    x.f.engine
                        .handle(command(
                            &x,
                            Command::ReleaseBrowserMedia {
                                original_request_id: x.media.preparation_request_id.clone(),
                            },
                        ))
                        .result
                        .unwrap();
                }
                1 => {
                    x.f.engine
                        .handle(release(&x, &initial.binding))
                        .result
                        .unwrap();
                }
                2 => {
                    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let _registry = pin.hold_registry_for_test();
                        panic!("authored selected-publication registry poison");
                    }));
                    assert!(poisoned.is_err());
                }
                3 => {
                    assert!(matches!(
                        native_claim_at(&pin, now - 1),
                        Err(BrowserTimingClaimFailure::Refused(_))
                    ));
                }
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    selected.evaluate_at(now),
                    Err(BrowserTimingClaimFailure::Refused(_))
                ),
                "selected publication retained authority after retirement case {case}"
            );
            assert!(!pin.current(now));
        }
    }
    #[test]
    fn native_timing_atomic_replacement_keeps_fresh_reads_and_retires_original_start_token() {
        let x = fixture_timing();
        let initial = opened(&x);
        let (_, observed) = first(&x, &initial.binding);
        let original = observed.observation.unwrap();
        let pin = native_pin(&x, &initial.binding);
        let claim = native_claim_at(&pin, x.base + 20_000_000).unwrap();
        let original_start = claim.observation_lifetime_for_test();
        assert!(original_start.load(Ordering::Acquire));
        drop(claim);
        let stop = AtomicBool::new(false);
        let read_at = x.base + 95_000_000;
        let reader_pin = pin.clone();
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        struct StopReader<'a>(&'a AtomicBool);
        impl Drop for StopReader<'_> {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let reads = std::thread::scope(|scope| {
            let stop = &stop;
            let _stop_on_unwind = StopReader(stop);
            let reader = scope.spawn(move || {
                let mut reads = 0u64;
                let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    assert!(
                        std::time::Instant::now() < end,
                        "publication reader fixture exceeded its bound"
                    );
                    let value = reader_pin
                        .try_snapshot_native(read_at)
                        .expect("publication produced false absence during valid replacement");
                    assert!((1..=33).contains(&value.sequence));
                    assert_eq!(
                        value.expires_at_boottime_ns,
                        value.bracket.native_issue_ns + 500_000_000
                    );
                    value.validate().unwrap();
                    reads += 1;
                    if reads == 1 {
                        started_tx.send(()).unwrap();
                    }
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    std::thread::yield_now();
                }
                reads
            });
            started_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            for sequence in 2..=33 {
                let issue_at = (sequence * 2 + 20) * 1_000_000;
                let p = probe(at(&x, issue_at, issue(&x, &initial.binding, sequence)).unwrap());
                let next =
                    report(at(&x, issue_at + 1_000_000, observe(&x, &p, frame(sequence))).unwrap());
                assert!(next.current);
            }
            stop.store(true, Ordering::Release);
            reader.join().unwrap()
        });
        assert!(reads > 0);
        assert!(
            !original_start.load(Ordering::Acquire),
            "replacement kept a captured original start alive"
        );
        assert_eq!(original.expires_at_boottime_ns, x.base + 510_000_000);
        let latest = pin.try_snapshot_native(read_at).unwrap();
        assert_eq!(latest.sequence, 33);
        assert!(
            pin.try_snapshot_native(latest.bracket.native_receive_ns - 1)
                .is_err(),
            "reader fell back to an older publication for a future-than-caller observation"
        );
        let current = native_claim_at(&pin, read_at).unwrap();
        assert_eq!(current.observation_for(&pin, read_at).unwrap(), &latest);
        assert!(!Arc::ptr_eq(
            &original_start,
            &current.observation_lifetime_for_test()
        ));
        assert!(current
            .observation_lifetime_for_test()
            .load(Ordering::Acquire));
    }
    #[test]
    fn native_timing_snapshot_missing_scope_expiry_retirement_and_regression_stay_refused() {
        use crate::authority::browser_timing::BrowserTimingClaimFailure;
        for case in 0..6 {
            let x = fixture_timing();
            let initial = opened(&x);
            let original = (case != 0).then(|| first(&x, &initial.binding).1.observation.unwrap());
            let pin = native_pin(&x, &initial.binding);
            let mut candidate = pin.clone();
            let now = match case {
                0 => x.base + 10_000_000,
                1 => x.base + 19_000_000,
                2 => original.as_ref().unwrap().expires_at_boottime_ns,
                _ => x.base + 20_000_000,
            };
            if case == 3 {
                let mut forged = pin.binding().clone();
                forged.player.generation += 1;
                candidate = pin.with_binding_for_test(forged);
            }
            if case == 4 {
                x.f.engine
                    .handle(command(
                        &x,
                        Command::ReleaseBrowserMedia {
                            original_request_id: x.media.preparation_request_id.clone(),
                        },
                    ))
                    .result
                    .unwrap();
            }
            if case == 5 {
                assert!(matches!(
                    native_claim_at(&pin, now - 1),
                    Err(BrowserTimingClaimFailure::Refused(_))
                ));
            }
            assert!(
                matches!(
                    candidate.try_snapshot_native(now),
                    Err(BrowserTimingClaimFailure::Refused(_))
                ),
                "case {case}"
            );
            if case == 1 || case == 3 {
                assert!(
                    pin.current(x.base + 20_000_000),
                    "an old caller tick or forged copy must not retire the actual subject"
                );
                assert_eq!(
                    pin.try_snapshot_native(x.base + 20_000_000).unwrap(),
                    original.unwrap()
                );
            }
            if case == 4 || case == 5 {
                assert!(
                    !pin.current(now),
                    "actual source retirement or registry regression remains terminal"
                );
            }
        }
    }
}
