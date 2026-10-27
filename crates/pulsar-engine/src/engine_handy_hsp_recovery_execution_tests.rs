// Included in the Engine localhost test module. Every qualification/recovery
// observation below is authored test data, not acceptance of a physical device.

struct AdditionalStream {
    peer: Peer,
    session: HspSessionHandle,
    authority: Arc<SyntheticAuthority>,
    record: HspStreamRecord,
    binding: HspBinding,
}
impl AdditionalStream {
    fn new(rig: &Rig) -> Self {
        let peer = Peer::new(false);
        peer.state.lock().unwrap().native_session = "independent-native-session".into();
        let binding = HspBinding::new(
            HspStreamIdentity {
                generation: 2,
                stream_id: 42,
                live: LiveIdentity {
                    epoch: AttemptId::new("independent-hsp-live").unwrap(),
                    project: rig.f.project.clone(),
                    revision: RevisionId::new(1),
                    source: SourceVersionId::new("independent-hsp-source").unwrap(),
                },
            },
            ClientId::new("independent-hsp-owner").unwrap(),
            DeviceSessionId::new("independent-hsp-device").unwrap(),
            ArtifactId::new("independent-hsp-clock").unwrap(),
        );
        let mut record = rig.record.clone();
        record.request_id = RequestId::new("independent-hsp-request").unwrap();
        record.registration.device_id = DeviceId::new("independent-hsp-device").unwrap();
        record.device.session_id = "independent-native-session".into();
        record.binding = HspBindingRecord::capture(&binding);
        let route = HandyRouteIdentity::from_snapshot(
            &rig.f.engine.handy_owner_token,
            "independent-fixture-connection",
        )
        .unwrap();
        record.route_sha256 = route.route_sha256().into();
        record.route_namespace_sha256 = route.namespace_sha256().into();
        let mut transport = HandyHspTransport::from_snapshot(
            HspTransportBinding {
                binding: binding.clone(),
                registration: record.registration.clone(),
                device: DeviceIdentity {
                    firmware: record.device.firmware.clone(),
                    model_number: record.device.model_number,
                    model_name: record.device.model_name.clone(),
                    model_variant: record.device.model_variant,
                    session_id: record.device.session_id.clone(),
                    firmware_status: record.device.firmware_status,
                },
                mode: HandyDeviceMode {
                    mode: 4,
                    mode_session_id: 7,
                },
                setup_previous: HspSetupPreviousState::new(9, HspPlayState::Stopped).unwrap(),
            },
            Arc::new(Credentials {
                api: "fixture-api".into(),
                connection: "independent-fixture-connection".into(),
            }),
            true,
            &rig.f.engine.handy_owner_token,
        )
        .unwrap();
        transport.set_test_origin(peer.origin.clone());
        let anchor = crate::handy_clock::boottime_ns().unwrap();
        let authority = Arc::new(SyntheticAuthority {
            finite_stop_budget: AtomicU64::new(0),
            binding: binding.clone(),
            anchor,
            lease_until: AtomicU64::new(anchor + 60_000_000_000),
            retired: AtomicBool::new(false),
            pre_lock_pause: Mutex::new(None),
            claim_pause: Mutex::new(None),
            claim_observations: Mutex::new(Vec::new()),
        });
        let admission = HspAdmission::new(
            binding.clone(),
            policy(),
            transport,
            record.clone(),
            authority.clone(),
        )
        .unwrap();
        let session = rig.execution.try_admit(admission).unwrap();
        Self {
            peer,
            session,
            authority,
            record,
            binding,
        }
    }
    fn wait(
        &self,
        rig: &Rig,
        reason: &str,
        predicate: impl Fn(&HspSessionSnapshot) -> bool,
    ) -> HspSessionSnapshot {
        let deadline = Instant::now() + WAIT;
        let mut last = None;
        loop {
            match rig.execution.snapshot_for_session(
                &rig.f.session,
                &rig.f.project,
                &self.record.request_id,
            ) {
                Ok(snapshot) => {
                    if predicate(&snapshot) {
                        return snapshot;
                    }
                    last = Some(snapshot);
                }
                Err(HspExecutionError::Busy) => {}
                Err(error) => panic!("{reason}: {error:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "{reason}: last={last:?}; requests={:?}",
                self.peer.puts()
            );
            thread::park_timeout(Duration::from_millis(2));
        }
    }
    fn ready(&self, rig: &Rig) {
        self.wait(
            rig,
            "independent stream must finish its own Setup probe",
            |s| s.post_setup_bound && !s.native_active && !s.metadata_pending,
        );
    }
    fn offer(&self, sequence: u64) {
        let deadline = Instant::now() + WAIT;
        loop {
            let now = crate::handy_clock::boottime_ns().unwrap();
            let sample = HspSample {
                point: HspPoint::new((sequence as u32 - 1) * 100, 50).unwrap(),
                source_sequence: sequence,
                captured_at_ns: now,
                accepted_at_ns: now,
                continuous_from_previous: true,
            };
            match self.session.offer(&self.binding.identity().live, sample) {
                Ok(HspOfferOutcome::Accepted) => return,
                Err(HspExecutionError::Busy) if Instant::now() < deadline => {
                    thread::park_timeout(Duration::from_millis(2))
                }
                result => panic!("independent sample {sequence} offer: {result:?}"),
            }
        }
    }
}

fn capture_rig_recovery(rig: &Rig) -> crate::handy_hsp_execution::HspRecoveryTargets {
    rig.execution
        .capture_recovery_targets(
            &rig.record.registration.device_id,
            &rig.record.route_sha256,
            &rig.record.route_namespace_sha256,
        )
        .unwrap()
}

#[test]
fn engine_hsp_targeted_drain_retains_original_worker_and_leaves_unrelated_stream_live() {
    let rig = Rig::new(false);
    rig.ready();
    let independent = AdditionalStream::new(&rig);
    independent.ready(&rig);
    let targets = capture_rig_recovery(&rig);
    assert_eq!(targets.stream_ids(), [rig.record.id().unwrap()]);
    rig.peer.hold_next("/hsp/add");
    rig.offer(1);
    rig.offer(2);
    rig.peer.wait_held();
    let cancel = AtomicBool::new(false);
    let proof = thread::scope(|scope| {
        let (completed, result) = std::sync::mpsc::sync_channel(1);
        let handle = &rig.execution;
        let cancelled = &cancel;
        let worker = scope.spawn(move || {
            let outcome = handle.drain_recovery(targets, Instant::now() + WAIT, cancelled);
            let _ = completed.send(outcome);
        });
        rig.wait(
            "targeted drain must retire only the retained original",
            |s| s.retired && s.native_active,
        );
        assert!(rig.session.request_play().is_err());
        assert!(matches!(
            result.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        independent.offer(1);
        independent.offer(2);
        let unaffected = independent.wait(
            &rig,
            "unrelated stream must append during targeted drain",
            |s| s.total_added == 2 && !s.native_active && !s.metadata_pending,
        );
        assert!(!unaffected.retired);
        assert!(!independent.authority.retired.load(Ordering::Acquire));
        assert_eq!(independent.peer.puts(), ["/hsp/setup", "/hsp/add"]);
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add"]);
        rig.peer.release();
        let proof = result.recv_timeout(WAIT).unwrap().unwrap();
        worker.join().unwrap();
        proof
    });
    assert_eq!(proof.stream_ids(), [rig.record.id().unwrap()]);
    let original_membership = rig
        .f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .hsp_stream_membership(&rig.record.id().unwrap())
        .unwrap();
    assert_eq!(proof.memberships(), &[original_membership]);
    assert_original_stop_history(
        &rig,
        &[
            (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
            (HspEffectRecordKind::Append, HspJournalState::Unknown),
        ],
    );
    assert!(matches!(
        rig.execution.try_admit(rig.duplicate_admission()),
        Err(HspExecutionError::InvalidAdmission)
    ));
    independent.offer(3);
    independent.wait(
        &rig,
        "unrelated stream must still accept fresh source after drain",
        |s| s.total_added == 3 && !s.native_active && !s.metadata_pending,
    );
    independent.session.cancel();
    independent.wait(
        &rig,
        "unrelated stream stops only on its own cancellation",
        |s| {
            s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::StopAcknowledged
                && !s.native_active
                && !s.metadata_pending
        },
    );
    assert_eq!(
        independent.peer.puts(),
        ["/hsp/setup", "/hsp/add", "/hsp/add", "/hsp/stop"]
    );
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
    // Draining alone is not a committed resolution and does not release the old
    // route or recreate dispatch authority from historical membership.
    assert_eq!(capture_rig_recovery(&rig).stream_ids(), proof.stream_ids());
    rig.f.engine.finish_shutdown().unwrap();
    assert_eq!(Arc::strong_count(&rig.f.engine), 1);
}

#[test]
fn engine_hsp_cancelled_or_expired_targeted_drain_never_reopens_original_admission() {
    for cancelled in [true, false] {
        let rig = Rig::new(false);
        rig.ready();
        let targets = capture_rig_recovery(&rig);
        let cancel = AtomicBool::new(cancelled);
        let deadline = if cancelled {
            Instant::now() + WAIT
        } else {
            Instant::now()
        };
        assert!(rig
            .execution
            .drain_recovery(targets, deadline, &cancel)
            .is_err());
        rig.stopped();
        rig.wait("failed drain's original Stop metadata must join", |s| {
            !s.metadata_pending
        });
        assert!(rig.session.request_play().is_err());
        assert!(matches!(
            rig.execution.try_admit(rig.duplicate_admission()),
            Err(HspExecutionError::InvalidAdmission)
        ));
        assert_eq!(
            capture_rig_recovery(&rig).stream_ids(),
            [rig.record.id().unwrap()]
        );
        assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
        assert_original_stop_history(
            &rig,
            &[(HspEffectRecordKind::Setup, HspJournalState::Acknowledged)],
        );
        rig.f.engine.finish_shutdown().unwrap();
    }
}

fn assert_original_stop_history(
    rig: &Rig,
    ordinary_prefix: &[(HspEffectRecordKind, HspJournalState)],
) {
    use crate::handy_hsp_records::HspOutcomeRecord;
    let (effects, emergency) = {
        let journal = rig.f.engine._device_effect_journal.lock().unwrap();
        let stream = journal.hsp_stream(&rig.record.id().unwrap()).unwrap();
        let effects = stream
            .effect_ids
            .iter()
            .map(|id| journal.hsp_effect(id).unwrap())
            .collect::<Vec<_>>();
        (
            effects,
            journal.hsp_emergency_for_stream(&stream.id).unwrap(),
        )
    };
    let ordinary = effects
        .iter()
        .filter(|e| e.record.kind != HspEffectRecordKind::Stop)
        .map(|e| (e.record.kind, e.state))
        .collect::<Vec<_>>();
    assert_eq!(
        ordinary, ordinary_prefix,
        "Stop must preserve every original ordinary outcome"
    );
    let stops = effects
        .iter()
        .filter(|e| e.record.kind == HspEffectRecordKind::Stop)
        .collect::<Vec<_>>();
    assert!(stops.len() <= 1);
    let stop_record = match &emergency {
        Some(emergency) => {
            if let Some(unused) = stops.first() {
                assert_eq!(
                    unused.state,
                    HspJournalState::Cancelled,
                    "an unused late Stop preparation must not invent a claim"
                );
                assert!(unused.outcome.is_none() && unused.late_observation.is_none());
                assert_eq!(
                    unused.record, emergency.effect,
                    "late prepared and native Stop must be the same exact intent"
                );
            }
            assert_original_emergency_is_durable(rig);
            &emergency.effect
        }
        None => {
            assert_eq!(
                stops.len(),
                1,
                "one durable Stop or retrospective emergency observation is required"
            );
            let durable = stops[0];
            assert_eq!(durable.state, HspJournalState::Acknowledged);
            assert!(matches!(
                durable.outcome,
                Some(HspOutcomeRecord::Acknowledged(_))
            ));
            &durable.record
        }
    };
    let mut expected = effects
        .iter()
        .filter(|e| e.record.kind != HspEffectRecordKind::Stop)
        .map(|e| {
            (
                e.record.request.method.clone(),
                e.record.request.path.clone(),
                e.record.request.body.clone().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    expected.push((
        stop_record.request.method.clone(),
        stop_record.request.path.clone(),
        stop_record.request.body.clone().unwrap_or_default(),
    ));
    let actual = rig
        .peer
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|r| r.0 == "PUT")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        expected, actual,
        "each original sent intent, including exactly one Stop, must match the actual HTTP bytes"
    );
}

fn assert_original_emergency_is_durable(rig: &Rig) {
    use crate::handy_hsp_records::{HspEmergencyProvenance, HspOutcomeRecord};
    let journal = rig.f.engine._device_effect_journal.lock().unwrap();
    let emergency = journal
        .hsp_emergency_for_stream(&rig.record.id().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(emergency.stream_id, rig.record.id().unwrap());
    assert_eq!(emergency.effect.kind, HspEffectRecordKind::Stop);
    assert_eq!(emergency.effect.request.method, "PUT");
    assert_eq!(emergency.effect.request.path, "/hsp/stop");
    assert_eq!(
        emergency.provenance,
        HspEmergencyProvenance::NativeNoPrewrite
    );
    assert!(matches!(
        emergency.outcome,
        HspOutcomeRecord::Acknowledged(_)
    ));
    let membership = journal.hsp_stream_membership(&emergency.stream_id).unwrap();
    let retained = membership.emergency.unwrap();
    assert_eq!(retained.id, emergency.id().unwrap());
    assert_eq!(retained.effect_id, emergency.effect.id().unwrap());
    assert_eq!(retained.sequence, emergency.effect.sequence);
    let stopped = rig
        .peer
        .state
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter(|request| request.0 == "PUT" && request.1 == "/hsp/stop")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        stopped,
        [(
            emergency.effect.request.method.clone(),
            emergency.effect.request.path.clone(),
            emergency.effect.request.body.clone().unwrap_or_default()
        )],
        "retrospective record must identify the exact single observed Stop PUT"
    );
}

#[test]
fn engine_hsp_held_metadata_worker_allows_expiry_and_stop_then_persists_without_resending() {
    use crate::handy_hsp_execution::HspMetadataTaskKind;
    let rig = Rig::new(false);
    rig.ready();
    let hold = rig
        .session
        .hold_next_metadata_for_test(HspMetadataTaskKind::Complete);
    rig.offer(1);
    rig.offer(2);
    let task = hold.wait_entered(Instant::now() + WAIT).unwrap();
    assert_eq!(task.kind, HspMetadataTaskKind::Complete);
    assert_eq!(task.sequence, Some(2));
    assert!(!hold.joined());
    assert_eq!(
        rig.history(),
        [
            (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
            (HspEffectRecordKind::Append, HspJournalState::Claimed),
        ]
    );
    rig.authority.lease_until.store(
        crate::handy_clock::boottime_ns().unwrap(),
        Ordering::Release,
    );
    let stopped = rig.stopped();
    assert!(stopped.retired && stopped.emergency_observed && stopped.metadata_pending);
    assert!(
        !hold.joined(),
        "Stop must finish before the held metadata worker joins"
    );
    assert!(!rig.status().emergency_history_durable);
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
    hold.release();
    rig.wait(
        "joined late metadata must become durable without a transport replay",
        |s| !s.metadata_pending,
    );
    assert!(hold.joined());
    assert!(rig.status().emergency_history_durable);
    assert_original_emergency_is_durable(&rig);
    assert_eq!(
        rig.history(),
        [
            (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
            (HspEffectRecordKind::Append, HspJournalState::Acknowledged),
        ]
    );
    assert!(rig.session.request_play().is_err());
    rig.f.engine.finish_shutdown().unwrap();
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
    assert_eq!(Arc::strong_count(&rig.f.engine), 1);
}

#[test]
fn engine_hsp_late_stop_preparation_is_cancelled_before_emergency_history_without_resending() {
    use crate::handy_hsp_execution::HspMetadataTaskKind;
    let rig = Rig::new(false);
    rig.ready();
    let hold = rig
        .session
        .hold_next_metadata_for_test(HspMetadataTaskKind::Prepare);
    rig.session.cancel();
    let task = hold.wait_entered(Instant::now() + WAIT).unwrap();
    assert_eq!(task.kind, HspMetadataTaskKind::Prepare);
    assert_eq!(task.sequence, Some(2));
    let stopped = rig.stopped();
    assert!(stopped.retired && stopped.emergency_observed && stopped.metadata_pending);
    assert!(
        !hold.joined(),
        "native Stop must not wait for its journal preparation"
    );
    assert!(!rig.status().emergency_history_durable);
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
    let cancel = rig
        .session
        .hold_next_metadata_for_test(HspMetadataTaskKind::Cancel);
    hold.release();
    let cancel_task = cancel.wait_entered(Instant::now() + WAIT).unwrap();
    assert_eq!(cancel_task.kind, HspMetadataTaskKind::Cancel);
    assert_eq!(cancel_task.sequence, task.sequence);
    assert!(hold.joined());
    assert_eq!(
        rig.history(),
        [
            (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
            (HspEffectRecordKind::Stop, HspJournalState::Prepared),
        ]
    );
    let emergency = rig
        .session
        .hold_next_metadata_for_test(HspMetadataTaskKind::Emergency);
    cancel.release();
    let emergency_task = emergency.wait_entered(Instant::now() + WAIT).unwrap();
    assert_eq!(emergency_task.kind, HspMetadataTaskKind::Emergency);
    assert_eq!(emergency_task.sequence, task.sequence);
    assert!(cancel.joined());
    assert_eq!(
        rig.history(),
        [
            (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
            (HspEffectRecordKind::Stop, HspJournalState::Cancelled),
        ]
    );
    assert!(!rig.status().emergency_history_durable);
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
    emergency.release();
    rig.wait(
        "late Stop preparation must be cancelled and emergency history committed",
        |s| !s.metadata_pending && s.emergency_history_durable,
    );
    assert!(hold.joined() && cancel.joined() && emergency.joined());
    assert_eq!(
        rig.history(),
        [
            (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
            (HspEffectRecordKind::Stop, HspJournalState::Cancelled),
        ]
    );
    assert_original_emergency_is_durable(&rig);
    assert!(rig.session.request_play().is_err());
    rig.f.engine.finish_shutdown().unwrap();
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
    assert_eq!(Arc::strong_count(&rig.f.engine), 1);
}

fn linked_recovery_rig() -> (Rig, HandyQualificationReport) {
    use crate::handy_hsp_records::HspQualificationLinkRecord;
    let f = fixture();
    // The Physical enum exercises the acceptance branch with an explicitly
    // authored unit-test dossier and simulated operator request only.
    let (path, mut input) = hsp_qualification_input(&f, HandyEvidenceOrigin::Physical);
    let p = policy();
    input.subject.hsp.as_mut().unwrap().policy = HandyHspPolicy {
        max_buffered_points: p.max_buffered_points as u32,
        device_capacity_points: p.device_capacity_points as u32,
        max_chunk_points: p.max_chunk_points as u32,
        min_lead_ms: p.min_lead_ms,
        max_future_ms: p.max_future_ms,
        max_sample_age_ns: p.max_sample_age_ns,
        max_gap_ms: p.max_gap_ms,
    };
    let imported = qualification_saved(
        f.engine
            .handle(qualification_import(
                &f,
                "owner-recovery-fixture-import",
                &path,
                &input,
            ))
            .result
            .unwrap(),
    );
    let (registration, connection) = hsp_qualification_connection(&f);
    let accepted = qualification_saved(
        qualification_call(
            &f,
            &qualification_accept(&f, &imported.dossier_id, &connection),
            |_| Ok(hsp_qualification_sample()),
        )
        .unwrap(),
    );
    let qualification = HandyQualificationBinding {
        dossier_id: accepted.dossier_id.clone(),
        revision: accepted.revision,
    };
    let bundle = handy_qualification::accepted_hsp_bundle(
        &f.engine.db.lock().unwrap(),
        &qualification,
        &registration,
        &hsp_qualification_sample(),
    )
    .unwrap();
    assert_eq!(bundle.policy, p);
    let peer = Peer::new(false);
    peer.state.lock().unwrap().native_session = input.subject.identity.session_id.clone();
    let binding = HspBinding::new(
        HspStreamIdentity {
            generation: 1,
            stream_id: 42,
            live: LiveIdentity {
                epoch: AttemptId::new("linked-owner-hsp-live").unwrap(),
                project: f.project.clone(),
                revision: RevisionId::new(1),
                source: SourceVersionId::new("linked-owner-hsp-source").unwrap(),
            },
        },
        ClientId::new("linked-owner-hsp-client").unwrap(),
        connection.device_session.clone(),
        ArtifactId::new("linked-owner-hsp-clock").unwrap(),
    );
    let credentials = handy_session_credentials();
    let route =
        HandyRouteIdentity::from_snapshot(&f.engine.handy_owner_token, &credentials.connection)
            .unwrap();
    let record = HspStreamRecord {
        boot: DeviceSessionId::new(f.engine.engine_epoch.clone()).unwrap(),
        actor_session: f.session.clone(),
        request_id: RequestId::new("linked-owner-original").unwrap(),
        registration: registration.clone(),
        device: input.subject.identity.clone(),
        route_sha256: route.route_sha256().into(),
        route_namespace_sha256: route.namespace_sha256().into(),
        binding: HspBindingRecord::capture(&binding),
        qualification_sha256: accepted.dossier_id.to_string(),
        profile_sha256: bundle.profile_sha256.clone(),
    };
    let link = HspQualificationLinkRecord {
        stream_id: record.id().unwrap(),
        qualification,
        subject_sha256: bundle.subject_sha256,
        profile_sha256: bundle.profile_sha256,
        admission_sha256: format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&("authored-engine-owner-admission-fixture", &record,)).unwrap()
            )
        ),
    };
    let mut transport = HandyHspTransport::from_snapshot(
        HspTransportBinding {
            binding: binding.clone(),
            registration,
            device: DeviceIdentity {
                firmware: record.device.firmware.clone(),
                model_number: record.device.model_number,
                model_name: record.device.model_name.clone(),
                model_variant: record.device.model_variant,
                session_id: record.device.session_id.clone(),
                firmware_status: record.device.firmware_status,
            },
            mode: HandyDeviceMode {
                mode: 4,
                mode_session_id: 7,
            },
            setup_previous: HspSetupPreviousState::new(9, HspPlayState::Stopped).unwrap(),
        },
        credentials,
        true,
        &f.engine.handy_owner_token,
    )
    .unwrap();
    transport.set_test_origin(peer.origin.clone());
    let anchor = crate::handy_clock::boottime_ns().unwrap();
    let authority = Arc::new(SyntheticAuthority {
        finite_stop_budget: AtomicU64::new(0),
        binding: binding.clone(),
        anchor,
        lease_until: AtomicU64::new(anchor + 60_000_000_000),
        retired: AtomicBool::new(false),
        pre_lock_pause: Mutex::new(None),
        claim_pause: Mutex::new(None),
        claim_observations: Mutex::new(Vec::new()),
    });
    let admission = HspAdmission::new(
        binding.clone(),
        bundle.policy,
        transport,
        record.clone(),
        authority.clone(),
    )
    .unwrap()
    .with_qualification_link(link);
    let execution = f.engine.ensure_handy_hsp_execution().unwrap();
    let session = execution.try_admit(admission).unwrap();
    (
        Rig {
            f,
            peer,
            execution,
            session,
            authority,
            record,
            binding,
        },
        accepted,
    )
}

fn wait_for_native_recovery_tick(target: u64) {
    let deadline = Instant::now() + WAIT;
    loop {
        let now = crate::handy_clock::boottime_ns().unwrap();
        if now >= target {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "native test clock did not reach recovery sample instant"
        );
        thread::park_timeout(Duration::from_nanos((target - now).min(1_000_000)));
    }
}

fn commit_authored_hsp_resolution(
    rig: &Rig,
    qualification: &HandyQualificationReport,
    proof: &crate::handy_hsp_execution::HspDrainedProof,
) -> crate::device_effect_journal::HspResolvedReceipt {
    use crate::device_effect_journal::CustodyResolutionRecord;
    let mut journal = rig.f.engine._device_effect_journal.lock().unwrap();
    let snapshot = journal
        .custody_snapshot(
            &rig.record.registration.device_id,
            &rig.record.route_sha256,
            &rig.record.route_namespace_sha256,
        )
        .unwrap();
    assert_eq!(snapshot.hsp_streams, proof.memberships());
    let contract = qualification.manifest.subject.recovery.clone().unwrap();
    assert_eq!(contract.transport, HandyQualificationTransport::Hsp);
    let mut plan = HandyRecoveryPlan {
        plan_id: ArtifactId::new("a".repeat(64)).unwrap(),
        snapshot_id: ArtifactId::new(snapshot.id().unwrap()).unwrap(),
        project_id: rig.f.project.clone(),
        selection: HandyRecoverySelection {
            device_id: rig.record.registration.device_id.clone(),
            registration_revision: rig.record.registration.revision,
            engine_epoch: rig.record.boot.clone(),
            device_session: rig.binding.session().clone(),
            qualification: HandyQualificationBinding {
                dossier_id: qualification.dossier_id.clone(),
                revision: qualification.revision,
            },
        },
        registration: rig.record.registration.clone(),
        route_sha256: rig.record.route_sha256.clone(),
        route_namespace_sha256: rig.record.route_namespace_sha256.clone(),
        counts: snapshot.counts(),
        projects: snapshot.projects(),
        devices: snapshot.devices(),
        contract: contract.clone(),
        playback_enabled: false,
    };
    plan.plan_id = plan.identity().unwrap();
    let mut observation = hsp_qualification_sample();
    observation.identity = rig.record.device.clone();
    observation.mode = HandyDeviceMode {
        mode: 4,
        mode_session_id: 8,
    };
    let hsp = observation.remote_hsp.as_mut().unwrap();
    hsp.stream_id = 42;
    hsp.max_points = 32;
    // Authored historical samples for journal consistency, deliberately not a
    // claim that localhost observations qualified physical settling or stopping.
    let drained = crate::handy_clock::boottime_ns().unwrap();
    wait_for_native_recovery_tick(
        drained
            .checked_add(contract.prior_command_settle_ns)
            .unwrap(),
    );
    let first_started = crate::handy_clock::boottime_ns().unwrap();
    wait_for_native_recovery_tick(
        first_started
            .checked_add(contract.stopped_sample_separation_ns)
            .unwrap(),
    );
    let second_started = crate::handy_clock::boottime_ns().unwrap();
    let evidence = HandyRecoveryEvidence {
        drained_boottime_ns: drained,
        first: HandyRecoverySample {
            started_boottime_ns: first_started,
            finished_boottime_ns: first_started,
            observation: observation.clone(),
        },
        second: HandyRecoverySample {
            started_boottime_ns: second_started,
            finished_boottime_ns: second_started,
            observation,
        },
    };
    let resolution = CustodyResolutionRecord {
        actor_session: rig.f.session.clone(),
        request_id: RequestId::new("authored-owner-resolution").unwrap(),
        request_fingerprint: format!("{:x}", Sha256::digest(b"authored-owner-resolution")),
        plan,
        basis: snapshot.clone(),
        snapshot,
        evidence,
    };
    let id = journal.record_resolution(&resolution).unwrap();
    journal
        .hsp_resolution_receipt(&id, proof.memberships())
        .unwrap()
}

#[test]
fn engine_hsp_only_matching_committed_resolution_releases_exact_drained_owner() {
    let (rig, accepted) = linked_recovery_rig();
    rig.ready();
    let cancel = AtomicBool::new(false);
    let first_proof = rig
        .execution
        .drain_recovery(capture_rig_recovery(&rig), Instant::now() + WAIT, &cancel)
        .unwrap();
    assert_eq!(first_proof.stream_ids(), [rig.record.id().unwrap()]);
    assert!(rig.session.request_play().is_err());
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);

    let (other, other_accepted) = linked_recovery_rig();
    other.ready();
    let other_proof = other
        .execution
        .drain_recovery(capture_rig_recovery(&other), Instant::now() + WAIT, &cancel)
        .unwrap();
    let other_receipt = commit_authored_hsp_resolution(&other, &other_accepted, &other_proof);
    other
        .execution
        .finish_recovery(other_proof, &other_receipt, Instant::now() + WAIT, &cancel)
        .unwrap();
    assert!(capture_rig_recovery(&other).stream_ids().is_empty());
    assert!(rig
        .execution
        .finish_recovery(first_proof, &other_receipt, Instant::now() + WAIT, &cancel)
        .is_err());
    assert_eq!(
        capture_rig_recovery(&rig).stream_ids(),
        [rig.record.id().unwrap()]
    );
    assert!(rig.session.request_play().is_err());

    let superseded = rig
        .execution
        .drain_recovery(capture_rig_recovery(&rig), Instant::now() + WAIT, &cancel)
        .unwrap();
    let current = rig
        .execution
        .drain_recovery(capture_rig_recovery(&rig), Instant::now() + WAIT, &cancel)
        .unwrap();
    let receipt = commit_authored_hsp_resolution(&rig, &accepted, &current);
    // Durable resolution alone does not authorize an obsolete native drain
    // token to release a subsequently captured owner instance.
    assert!(rig
        .execution
        .finish_recovery(superseded, &receipt, Instant::now() + WAIT, &cancel)
        .is_err());
    let still_retained = rig
        .execution
        .snapshot_for_session(&rig.f.session, &rig.f.project, &rig.record.request_id)
        .unwrap();
    assert!(still_retained.retired && !still_retained.native_active);
    assert!(matches!(
        rig.execution.try_admit(rig.duplicate_admission()),
        Err(HspExecutionError::InvalidAdmission)
    ));
    rig.execution
        .finish_recovery(current, &receipt, Instant::now() + WAIT, &cancel)
        .unwrap();
    assert!(capture_rig_recovery(&rig).stream_ids().is_empty());
    let retained = rig
        .execution
        .snapshot_for_session(&rig.f.session, &rig.f.project, &rig.record.request_id)
        .unwrap();
    assert!(retained.retired && !retained.native_active && !retained.metadata_pending);
    assert!(rig.session.request_play().is_err());
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
    assert_eq!(other.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
    assert_original_stop_history(
        &rig,
        &[(HspEffectRecordKind::Setup, HspJournalState::Acknowledged)],
    );
    rig.f.engine.finish_shutdown().unwrap();
    other.f.engine.finish_shutdown().unwrap();
    assert_eq!(Arc::strong_count(&rig.f.engine), 1);
    assert_eq!(Arc::strong_count(&other.f.engine), 1);
}

#[test]
fn engine_hsp_shutdown_joins_held_metadata_after_independent_native_stop() {
    use crate::handy_hsp_execution::HspMetadataTaskKind;
    let rig = Rig::new(false);
    rig.ready();
    let hold = rig
        .session
        .hold_next_metadata_for_test(HspMetadataTaskKind::Complete);
    rig.offer(1);
    rig.offer(2);
    let task = hold.wait_entered(Instant::now() + WAIT).unwrap();
    assert_eq!(task.sequence, Some(2));
    rig.f.engine.request_shutdown();
    let stopped = rig.stopped();
    assert!(stopped.emergency_observed && stopped.metadata_pending && !hold.joined());
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
    thread::scope(|scope| {
        let (finished, done) = std::sync::mpsc::sync_channel(1);
        let engine = &rig.f.engine;
        let worker = scope.spawn(move || {
            let result = engine.finish_shutdown();
            let _ = finished.send(result);
        });
        assert!(matches!(
            done.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        assert!(!hold.joined());
        hold.release();
        done.recv_timeout(WAIT).unwrap().unwrap();
        worker.join().unwrap();
    });
    assert!(hold.joined());
    let snapshot = rig
        .execution
        .snapshot_for_session(&rig.f.session, &rig.f.project, &rig.record.request_id)
        .unwrap();
    assert!(snapshot.retired && !snapshot.native_active && !snapshot.metadata_pending);
    assert_original_emergency_is_durable(&rig);
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/add", "/hsp/stop"]);
    assert_eq!(Arc::strong_count(&rig.f.engine), 1);
}

fn wait_metadata_busy(
    session: &HspSessionHandle,
    reason: &str,
    predicate: impl Fn(&crate::handy_hsp_execution::HspMetadataBusyObservation) -> bool,
) -> crate::handy_hsp_execution::HspMetadataBusyObservation {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(observed) = session.metadata_busy_for_test() {
            if predicate(&observed) {
                return observed;
            }
        }
        assert!(
            Instant::now() < deadline,
            "{reason}: {:?}",
            session.metadata_busy_for_test()
        );
        thread::park_timeout(Duration::from_millis(2));
    }
}

#[test]
fn engine_hsp_short_shared_journal_mutex_contention_retries_exact_task_without_retirement() {
    use crate::handy_hsp_execution::HspMetadataTaskKind;
    let rig = Rig::new(false);
    rig.ready();
    let independent = AdditionalStream::new(&rig);
    independent.ready(&rig);
    let held = rig.f.engine._device_effect_journal.lock().unwrap();
    independent.offer(1);
    independent.offer(2);
    let first = wait_metadata_busy(
        &independent.session,
        "independent append must observe shared journal contention",
        |o| o.identity.kind == HspMetadataTaskKind::Prepare && o.identity.sequence == Some(2),
    );
    let repeated = wait_metadata_busy(
        &independent.session,
        "the same original metadata task must retry",
        |o| o.busy_count >= first.busy_count + 2,
    );
    assert_eq!(repeated.identity, first.identity);
    assert_eq!(repeated.first_attempt, first.first_attempt);
    assert!(
        repeated.first_attempt.elapsed() < Duration::from_secs(3),
        "short-contention fixture exceeded the unchanged native retry budget"
    );
    assert_eq!(independent.peer.puts(), ["/hsp/setup"]);
    let waiting = independent.wait(
        &rig,
        "retrying metadata must keep the unrelated source live",
        |s| s.accepted_samples == 2 && s.metadata_pending,
    );
    assert!(!waiting.retired);
    assert!(
        !rig.execution
            .snapshot_for_session(&rig.f.session, &rig.f.project, &rig.record.request_id)
            .unwrap()
            .retired
    );
    drop(held);
    independent.wait(
        &rig,
        "same original append must send once after mutex release",
        |s| s.total_added == 2 && !s.native_active && !s.metadata_pending,
    );
    let completed = independent.session.metadata_busy_for_test().unwrap();
    assert_eq!(completed.identity, first.identity);
    assert_eq!(completed.first_attempt, first.first_attempt);
    assert!(completed.busy_count >= repeated.busy_count);
    assert_eq!(independent.peer.puts(), ["/hsp/setup", "/hsp/add"]);
    rig.offer(1);
    rig.offer(2);
    rig.wait("other stream must also remain usable", |s| {
        s.total_added == 2 && !s.native_active && !s.metadata_pending
    });
    independent.session.cancel();
    independent.wait(
        &rig,
        "independent cancellation must join native and metadata work",
        |s| {
            s.runtime_state == crate::handy_hsp_runtime::HspRuntimeState::StopAcknowledged
                && !s.native_active
                && !s.metadata_pending
        },
    );
    rig.session.cancel();
    rig.stopped();
    rig.wait("other original Stop metadata must join", |s| {
        !s.metadata_pending
    });
    assert_original_stop_history(
        &rig,
        &[
            (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
            (HspEffectRecordKind::Append, HspJournalState::Acknowledged),
        ],
    );
    assert_eq!(
        independent.peer.puts(),
        ["/hsp/setup", "/hsp/add", "/hsp/stop"]
    );
    rig.f.engine.finish_shutdown().unwrap();
    assert_eq!(Arc::strong_count(&rig.f.engine), 1);
}

#[test]
fn engine_hsp_sustained_metadata_mutex_contention_expires_original_three_second_episode() {
    use crate::handy_hsp_execution::HspMetadataTaskKind;
    let rig = Rig::new(false);
    rig.ready();
    let held = rig.f.engine._device_effect_journal.lock().unwrap();
    rig.offer(1);
    rig.offer(2);
    let first = wait_metadata_busy(
        &rig.session,
        "append metadata must observe held mutex",
        |o| o.identity.kind == HspMetadataTaskKind::Prepare && o.identity.sequence == Some(2),
    );
    let repeated = wait_metadata_busy(
        &rig.session,
        "Busy retries must retain first-attempt identity",
        |o| o.busy_count >= first.busy_count + 2,
    );
    assert_eq!(repeated.identity, first.identity);
    assert_eq!(repeated.first_attempt, first.first_attempt);
    assert!(repeated.first_attempt.elapsed() < Duration::from_secs(3));
    let stopped = rig.stopped();
    assert!(
        first.first_attempt.elapsed() >= Duration::from_secs(3),
        "metadata retry must honor, rather than shorten, its fixed original budget"
    );
    assert!(stopped.retired && stopped.emergency_observed && stopped.metadata_pending);
    assert!(
        rig.authority.lease_until.load(Ordering::Acquire)
            > crate::handy_clock::boottime_ns().unwrap(),
        "the fixture must exercise metadata expiry, not source lease expiry"
    );
    assert_eq!(
        rig.peer.puts(),
        ["/hsp/setup", "/hsp/stop"],
        "an expired original prepare must never send an Append"
    );
    assert!(rig.session.request_play().is_err());
    drop(held);
    assert_recovered_history_stays_unsent(&rig);
    assert_eq!(rig.peer.puts(), ["/hsp/setup", "/hsp/stop"]);
    assert_eq!(Arc::strong_count(&rig.f.engine), 1);
}

fn authored_stopped_hsp_observation(
    peer: &Mutex<PeerState>,
    identity: &HandyIdentity,
) -> HandySetupDeviceObservation {
    let state = peer.lock().unwrap();
    assert!(
        !state.playing,
        "fixture inspection must observe the stopped original stream"
    );
    let mut sample = hsp_qualification_sample();
    sample.identity = identity.clone();
    sample.mode = HandyDeviceMode {
        mode: 4,
        mode_session_id: state.mode,
    };
    let remote = sample.remote_hsp.as_mut().unwrap();
    remote.play_state = HandyRemotePlayState::Stopped;
    remote.stream_id = state.stream;
    remote.current_time_ms = -10_000;
    remote.points = u32::try_from((state.tail + 1).max(0)).unwrap();
    remote.max_points = 32;
    remote.current_point = if state.tail < 0 { -1 } else { 0 };
    remote.playback_rate = 1.0;
    remote.looping = false;
    remote.pause_on_starving = false;
    remote.first_point_time_ms = u32::try_from(state.first_time).unwrap();
    remote.last_point_time_ms = u32::try_from(state.last_time).unwrap();
    remote.tail_point_stream_index = i32::try_from(state.tail).unwrap();
    remote.tail_point_stream_index_threshold = 1;
    sample.validate().unwrap();
    sample
}

fn original_unknown_append(rig: &Rig) -> (String, Vec<u8>) {
    let journal = rig.f.engine._device_effect_journal.lock().unwrap();
    let stream = journal.hsp_stream(&rig.record.id().unwrap()).unwrap();
    let effect = stream
        .effect_ids
        .iter()
        .map(|id| journal.hsp_effect(id).unwrap())
        .find(|effect| effect.record.kind == HspEffectRecordKind::Append)
        .unwrap();
    assert_eq!(effect.state, HspJournalState::Unknown);
    (
        effect.id,
        serde_json::to_vec(&(effect.record, effect.outcome, effect.late_observation)).unwrap(),
    )
}

#[test]
fn engine_hsp_public_reconciliation_samples_twice_preserves_unknown_and_replays_without_io() {
    let (rig, accepted) = linked_recovery_rig();
    rig.ready();
    rig.peer.hold_next("/hsp/add");
    rig.offer(1);
    rig.offer(2);
    rig.peer.wait_held();
    rig.authority.lease_until.store(
        crate::handy_clock::boottime_ns().unwrap(),
        Ordering::Release,
    );
    rig.wait(
        "original Append must retire while transport still owns its reply",
        |s| s.retired && s.native_active,
    );
    rig.peer.release();
    rig.stopped();
    rig.wait(
        "original unknown Append and exact Stop metadata must settle",
        |s| !s.metadata_pending,
    );
    assert_original_stop_history(
        &rig,
        &[
            (HspEffectRecordKind::Setup, HspJournalState::Acknowledged),
            (HspEffectRecordKind::Append, HspJournalState::Unknown),
        ],
    );
    let unknown_before = original_unknown_append(&rig);
    let selection = HandyRecoverySelection {
        device_id: rig.record.registration.device_id.clone(),
        registration_revision: rig.record.registration.revision,
        engine_epoch: rig.record.boot.clone(),
        device_session: rig.binding.session().clone(),
        qualification: HandyQualificationBinding {
            dossier_id: accepted.dossier_id,
            revision: accepted.revision,
        },
    };
    let planned = rig
        .f
        .engine
        .handle(request(
            &rig.f.session,
            Some(&rig.f.project),
            None,
            Command::PlanHandyReconciliation {
                selection,
                owner_proof: rig.f.engine.handy_owner_token.clone(),
            },
        ))
        .result
        .unwrap();
    let ResponseBody::HandyRecoveryPlan(plan) = planned else {
        panic!("expected HSP recovery plan");
    };
    assert_eq!(plan.contract.transport, HandyQualificationTransport::Hsp);
    let start = reconciliation_start(&rig.f, &plan, "public-hsp-owner-recovery");
    let state = rig.peer.state.clone();
    let identity = rig.record.device.clone();
    let inspections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = inspections.clone();
    // The injected inspector reports authored local-peer state and reviewed
    // fixture settings. It makes no physical-device or real acceptance claim.
    let pending = reconciliation_call(&rig.f, &start, move |_, _| {
        let sample = authored_stopped_hsp_observation(&state, &identity);
        observed.fetch_add(1, Ordering::AcqRel);
        Ok(sample)
    })
    .unwrap();
    assert_eq!(pending.state, HandyRecoveryState::Pending);
    let completed = reconciliation_wait(&rig.f, &start.request_id);
    assert_eq!(
        completed.state,
        HandyRecoveryState::Completed,
        "{:?}",
        completed.error
    );
    assert_eq!(inspections.load(Ordering::Acquire), 2);
    assert!(!completed.authority_restored && !completed.playback_enabled);
    assert_eq!(original_unknown_append(&rig), unknown_before);
    let resolution = completed.resolution.as_ref().unwrap();
    let membership = rig
        .f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .hsp_stream_membership(&rig.record.id().unwrap())
        .unwrap();
    let receipt = rig
        .f
        .engine
        ._device_effect_journal
        .lock()
        .unwrap()
        .hsp_resolution_receipt(resolution.record_id.as_str(), &[membership])
        .unwrap();
    let stream_id = rig.record.id().unwrap();
    assert_eq!(receipt.stream_ids(), [stream_id.as_str()]);
    assert!(capture_rig_recovery(&rig).stream_ids().is_empty());
    assert!(rig.session.request_play().is_err());
    let puts = rig.peer.puts();
    let replay = reconciliation_call(&rig.f, &start, |_, _| {
        panic!("completed HSP recovery replay must not inspect")
    })
    .unwrap();
    assert_eq!(encode(&replay).unwrap(), encode(&completed).unwrap());
    assert_eq!(inspections.load(Ordering::Acquire), 2);
    assert_eq!(rig.peer.puts(), puts);
    assert_eq!(original_unknown_append(&rig), unknown_before);
    rig.f.engine.finish_shutdown().unwrap();
    assert_eq!(Arc::strong_count(&rig.f.engine), 1);
}
