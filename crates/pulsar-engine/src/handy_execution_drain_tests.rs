use super::*;
use crate::playback_runtime::{tests::clock, QualifiedCommandTiming};
use pulsar_core::{DeviceQualification, QualificationAuthority};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
};

struct Fixture {
    dir: tempfile::TempDir,
    registry: Arc<Mutex<Registry>>,
    journal: Arc<Mutex<DeviceEffectJournal>>,
    db: Arc<Mutex<Connection>>,
}
struct Target {
    id: ArtifactId,
    runtime: Arc<Mutex<HandyPlaybackRuntime>>,
    pending: Arc<AtomicBool>,
    listener: TcpListener,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let journal =
            DeviceEffectJournal::open(dir.path(), DeviceSessionId::new("drain-boot").unwrap())
                .unwrap();
        Self {
            dir,
            registry: Arc::new(Mutex::new(Registry::default())),
            journal: Arc::new(Mutex::new(journal)),
            db: Arc::new(Mutex::new(Connection::open_in_memory().unwrap())),
        }
    }
    fn resources(&self) -> Resources {
        Resources {
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            registry: self.registry.clone(),
            journal: self.journal.clone(),
            db: self.db.clone(),
            pool: ResourcePool::new(64 * 1024 * 1024, 4),
            binding: self.journal.lock().unwrap().receipt_binding(),
        }
    }
    fn start(&self) -> ExecutionPump {
        ExecutionPump::start(
            self.registry.clone(),
            self.journal.clone(),
            self.db.clone(),
            ResourcePool::new(64 * 1024 * 1024, 4),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap()
    }
    fn resolve(&self, target: &Target) {
        let report = self.registry.lock().unwrap().entries[&target.id]
            .report
            .clone();
        let inspection = &report.correlation.inspection;
        let route = format!("{:x}", Sha256::digest(target.id.as_str().as_bytes()));
        let namespace = "a".repeat(64);
        let mut journal = self.journal.lock().unwrap();
        let snapshot = journal
            .custody_snapshot(&report.admission.configuration.device, &route, &namespace)
            .unwrap();
        // Authored historical fixtures only: this does not qualify a device or
        // invoke the Engine's physical-evidence adapter.
        let mut plan = HandyRecoveryPlan {
            plan_id: ArtifactId::new("0".repeat(64)).unwrap(),
            snapshot_id: ArtifactId::new(snapshot.id().unwrap()).unwrap(),
            project_id: report.admission.project.clone(),
            selection: HandyRecoverySelection {
                device_id: report.admission.configuration.device.clone(),
                registration_revision: inspection.registration.revision,
                engine_epoch: DeviceSessionId::new("drain-boot").unwrap(),
                device_session: inspection.device_session.clone(),
                qualification: inspection.accepted_qualification.clone().unwrap(),
            },
            registration: inspection.registration.clone(),
            route_sha256: route,
            route_namespace_sha256: namespace,
            counts: snapshot.counts(),
            projects: snapshot.projects(),
            devices: snapshot.devices(),
            contract: HandyRecoveryQualification {
                transport: HandyQualificationTransport::Hssp,
                model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
                remote_state: HandyRecoveryRemoteState::Stopped,
                prior_command_settle_ns: 1_000_000_000,
                stopped_sample_separation_ns: 1_000_000_000,
                maximum_sample_span_ns: 1_000_000_000,
            },
            playback_enabled: false,
        };
        plan.plan_id = plan.identity().unwrap();
        let record = crate::device_effect_journal::CustodyResolutionRecord {
            actor_session: SessionId::new("synthetic-drain-owner").unwrap(),
            request_id: RequestId::new(format!("{}-resolve", target.id)).unwrap(),
            request_fingerprint: "b".repeat(64),
            plan,
            basis: snapshot.clone(),
            snapshot,
            evidence: HandyRecoveryEvidence {
                drained_boottime_ns: 1_000_000_000,
                first: HandyRecoverySample {
                    started_boottime_ns: 2_000_000_000,
                    finished_boottime_ns: 2_100_000_000,
                    observation: inspection.device.clone(),
                },
                second: HandyRecoverySample {
                    started_boottime_ns: 3_100_000_000,
                    finished_boottime_ns: 3_200_000_000,
                    observation: inspection.device.clone(),
                },
            },
        };
        journal.record_resolution(&record).unwrap();
    }
    fn target(&self, name: &str) -> Target {
        let id = ArtifactId::new(name).unwrap();
        let mut report: HandyPlaybackPreparation = serde_json::from_str(include_str!(
            "../../pulsar-protocol/tests/fixtures/handy-playback-preparation.json"
        ))
        .unwrap();
        let mut admission = report.admission.clone();
        admission.configuration.device = DeviceId::new(name).unwrap();
        // These local HTTP fixtures exercise custody, not hardware qualification.
        let qualification = DeviceQualification::new(
            admission.configuration.clone(),
            report
                .correlation
                .inspection
                .accepted_qualification
                .as_ref()
                .unwrap()
                .dossier_id
                .clone(),
            QualificationAuthority::LocalHumanAcceptance,
            Some(7_000_000),
        )
        .unwrap();
        let timing =
            QualifiedCommandTiming::new(qualification, 10_000_000, 1_000_000, 100).unwrap();
        let session = DeviceSessionId::new(format!("{name}-session")).unwrap();
        let inspection = &mut report.correlation.inspection;
        inspection.engine_epoch = DeviceSessionId::new("drain-boot").unwrap();
        inspection.device_session = session.clone();
        inspection.registration.device_id = admission.configuration.device.clone();
        let mut journal = self.journal.lock().unwrap();
        let setup = journal
            .prepare_setup(crate::device_effect_journal::SetupEffectRecord {
                boot: inspection.engine_epoch.clone(),
                device_session: session.clone(),
                actor_session: SessionId::new("synthetic-drain-owner").unwrap(),
                request_id: RequestId::new(format!("{name}-setup")).unwrap(),
                hosting_request_id: inspection.hosting_request_id.clone(),
                project_id: admission.project.clone(),
                revision: admission.revision,
                registration: inspection.registration.clone(),
                qualification: inspection.accepted_qualification.clone().unwrap(),
                master_sha256: inspection.master_sha256.clone(),
                script: inspection.script.clone(),
                hosted: inspection.hosted.clone(),
                route_sha256: format!("{:x}", Sha256::digest(name.as_bytes())),
                route_namespace_sha256: Some("a".repeat(64)),
                acknowledgment_required: true,
                preflight_mode: Some(inspection.device.mode.clone()),
            })
            .unwrap();
        let claimed = journal.claim_setup(&setup).unwrap();
        journal
            .complete_setup(
                &claimed,
                crate::device_effect_journal::SetupCompletion::Acknowledged(
                    report.correlation.acknowledgment.clone(),
                ),
            )
            .unwrap();
        drop(journal);
        let mut runtime = HandyPlaybackRuntime::new(
            admission.configuration.device.clone(),
            session.clone(),
            timing,
        )
        .unwrap();
        let controller = ClientId::new(format!("{name}-controller")).unwrap();
        runtime
            .arm(controller.clone(), admission.clone(), None)
            .unwrap();
        let mapping = clock();
        let start = runtime
            .prepare_start(&controller, mapping.clone(), 0)
            .unwrap();
        runtime
            .dispatch_start_durable(
                start,
                &admission,
                &controller,
                &mapping.domain,
                mapping.monotonic_ns,
                &mut self.journal.lock().unwrap(),
            )
            .unwrap();
        let stop = runtime.revoke_controller().unwrap().unwrap();
        let runtime = Arc::new(Mutex::new(runtime));
        let pending = Arc::new(AtomicBool::new(false));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let credentials = Arc::new(Credentials {
            api: "synthetic-drain-api".into(),
            connection: "synthetic-drain-connection".into(),
        });
        let mut transport = HandyTransport::from_snapshot(
            TransportBinding {
                admission: admission.clone(),
                session: session.clone(),
                stream_id: 8,
            },
            credentials.clone(),
        )
        .unwrap();
        transport.set_test_origin(format!("http://{}", listener.local_addr().unwrap()));
        report.preparation_id = id.clone();
        report.admission = admission;
        report.state = HandyPlaybackPreparationState::Retired;
        report.physical_stop_pending = true;
        let inspection = &report.correlation.inspection;
        let captured = handy_setup::Captured {
            connection: HandyConnectionSession {
                engine_epoch: inspection.engine_epoch.clone(),
                device_session: session,
                open_request_id: RequestId::new(format!("{name}-open")).unwrap(),
                registration: inspection.registration.clone(),
                state: HandyConnectionState::Retired,
                inspection: None,
            },
            credentials,
            hosting: HandyHostingReport {
                request_id: inspection.hosting_request_id.clone(),
                preparation_request_id: RequestId::new("synthetic-script").unwrap(),
                project_id: inspection.project_id.clone(),
                revision: inspection.revision,
                engine_epoch: inspection.engine_epoch.clone(),
                master_sha256: inspection.master_sha256.clone(),
                script: inspection.script.clone(),
                outcome: HandyHostingOutcome::Verified,
                downloaded: Some(inspection.hosted.clone()),
                retired: true,
                link_available: false,
                upload_in_flight: false,
            },
            hosted: crate::handy_hosting_wire::fixture_verified(b"0,0\n1000,100\n"),
        };
        self.registry.lock().unwrap().entries.insert(
            id.clone(),
            Entry {
                owner: SessionId::new("synthetic-drain-owner").unwrap(),
                report,
                captured,
                runtime: runtime.clone(),
                transport: Arc::new(transport),
                execution_pending: pending.clone(),
                stop_activity: Arc::new(Mutex::new(None)),
                controller: None,
                server_clock: None,
                playback_clock: None,
                continuous_timing: None,
                player_guard: None,
                continuous_attempt: 0,
                clock_attempt: 0,
                pending_controller_stop: Some(stop),
                play: None,
                pending_play: None,
                browser_owner: None,
            },
        );
        Target {
            id,
            runtime,
            pending,
            listener,
        }
    }
}
fn wait(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready() {
        assert!(Instant::now() < deadline, "synthetic drain fixture stalled");
        thread::sleep(Duration::from_millis(2));
    }
}
fn server(target: &Target) -> (mpsc::Receiver<()>, SyncSender<()>, JoinHandle<()>) {
    let listener = target.listener.try_clone().unwrap();
    let (entered, started) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let mut accepted = None;
        wait(|| match listener.accept() {
            Ok((stream, _)) => {
                accepted = Some(stream);
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(error) => panic!("synthetic drain accept: {error}"),
        });
        let mut stream = accepted.unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "PUT /hssp/stop HTTP/1.1");
        loop {
            line.clear();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
        }
        entered.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(5)).unwrap();
        let body = r#"{"result":{"play_state":2,"stream_id":8,"current_time":0,"playback_rate":1.0,"loop":false,"pause_on_starving":false}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    (started, release, worker)
}
fn request(ids: Vec<ArtifactId>) -> DrainRequest {
    DrainRequest {
        ids,
        stage: DrainStage::Quiesce,
        deadline: Instant::now() + Duration::from_secs(5),
        abandoned: Arc::new(AtomicBool::new(false)),
        reply: mpsc::sync_channel(1).0,
    }
}

#[test]
fn reconciliation_drain_collects_pending_retirement_before_declaring_empty_work() {
    let f = Fixture::new();
    let target = f.target("pending-target");
    let resources = f.resources();
    let mut work = HashMap::new();
    let drain = request(vec![target.id.clone()]);
    assert!(!finish_drain(&resources, &mut work, &drain).unwrap());
    assert!(work.contains_key(&target.id));
    assert!(target.pending.load(Ordering::Acquire));
    let registry = f.registry.lock().unwrap();
    assert!(registry.entries[&target.id]
        .pending_controller_stop
        .is_none());
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
    assert!(target.listener.accept().is_err());
}

#[test]
fn reconciliation_drain_waits_for_target_only_and_preserves_unrelated_inflight_stop() {
    let f = Fixture::new();
    let target = f.target("selected-target");
    let other = f.target("unselected-target");
    let (started, release, serving) = server(&target);
    let (other_started, other_release, other_serving) = server(&other);
    let mut pump = f.start();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    other_started.recv_timeout(Duration::from_secs(5)).unwrap();
    thread::scope(|scope| {
        let (done, result) = mpsc::sync_channel(1);
        let pump = &pump;
        let id = target.id.clone();
        scope.spawn(move || {
            done.send(pump.drain_for_reconciliation(
                vec![id],
                Instant::now() + Duration::from_secs(4),
                &AtomicBool::new(false),
            ))
            .unwrap();
        });
        assert!(result.recv_timeout(Duration::from_millis(50)).is_err());
        release.send(()).unwrap();
        result
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
    });
    serving.join().unwrap();
    assert!(!target.pending.load(Ordering::Acquire));
    assert!(other.pending.load(Ordering::Acquire));
    assert_eq!(
        other.runtime.lock().unwrap().session().state(),
        DeviceState::StopRequested
    );
    assert!(f.registry.lock().unwrap().entries.contains_key(&target.id));
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
    pump.finish_reconciliation(
        vec![target.id.clone()],
        Instant::now() + Duration::from_secs(1),
        &AtomicBool::new(false),
    )
    .unwrap();
    other_release.send(()).unwrap();
    other_serving.join().unwrap();
    wait(|| !other.pending.load(Ordering::Acquire));
    assert!(target.listener.accept().is_err());
    assert!(other.listener.accept().is_err());
    pump.shutdown().unwrap();
}

#[test]
fn reconciliation_handle_wait_releases_owner_slot_for_unrelated_controller_checks() {
    let f = Fixture::new();
    let target = f.target("handle-slot-target");
    let (started, release, serving) = server(&target);
    let slot = Mutex::new(Some(f.start()));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let handle = {
        let owner = slot.lock().unwrap();
        owner.as_ref().unwrap().reconciliation_handle().unwrap()
    };
    let registry_references = Arc::strong_count(&f.registry);
    let waiter = handle.clone();
    assert_eq!(Arc::strong_count(&f.registry), registry_references);
    thread::scope(|scope| {
        let (done, result) = mpsc::sync_channel(1);
        let id = target.id.clone();
        scope.spawn(move || {
            done.send(waiter.drain_for_reconciliation(
                vec![id],
                Instant::now() + Duration::from_secs(4),
                &AtomicBool::new(false),
            ))
            .unwrap();
        });
        assert!(result.recv_timeout(Duration::from_millis(50)).is_err());
        {
            let owner = slot
                .try_lock()
                .expect("target drain retained the pump owner slot");
            // This is the owner-slot access used by the unrelated controller
            // monitor/renewal check, while the selected stop is still blocked.
            assert!(owner.as_ref().unwrap().is_running());
            assert!(owner.as_ref().unwrap().reconciliation_handle().is_ok());
        }
        assert!(target.pending.load(Ordering::Acquire));
        release.send(()).unwrap();
        result
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
    });
    serving.join().unwrap();
    assert!(!target.pending.load(Ordering::Acquire));
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
    slot.lock().unwrap().as_mut().unwrap().shutdown().unwrap();
    assert!(handle
        .drain_for_reconciliation(
            vec![target.id],
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
        )
        .is_err());
}

#[test]
fn reconciliation_handle_outlives_owner_without_retaining_resources_or_accepting_commands() {
    let f = Fixture::new();
    let mut pump = f.start();
    let handle = pump.reconciliation_handle().unwrap();
    let cloned = handle.clone();
    pump.shutdown().unwrap();
    assert_eq!(
        pump.reconciliation_handle().err().unwrap().code,
        ErrorCode::Unavailable
    );
    drop(pump);
    assert_eq!(Arc::strong_count(&f.registry), 1);
    assert_eq!(Arc::strong_count(&f.journal), 1);
    assert_eq!(Arc::strong_count(&f.db), 1);
    let begin = Instant::now();
    for stage in [DrainStage::Quiesce, DrainStage::Finish] {
        let error = cloned
            .reconciliation_command(
                Vec::new(),
                stage,
                Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unavailable);
    }
    assert!(begin.elapsed() < Duration::from_secs(1));
}

#[test]
fn reconciliation_handle_wait_notices_shutdown_while_stop_custody_still_drains() {
    let f = Fixture::new();
    let target = f.target("handle-shutdown-target");
    let (started, release, serving) = server(&target);
    let mut pump = f.start();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let handle = pump.reconciliation_handle().unwrap();
    thread::scope(|scope| {
        let (done, result) = mpsc::sync_channel(1);
        let id = target.id.clone();
        scope.spawn(move || {
            done.send(handle.drain_for_reconciliation(
                vec![id],
                Instant::now() + Duration::from_secs(4),
                &AtomicBool::new(false),
            ))
            .unwrap();
        });
        assert!(result.recv_timeout(Duration::from_millis(50)).is_err());
        let (retired, retirement) = mpsc::sync_channel(1);
        scope.spawn(move || retired.send(pump.shutdown()).unwrap());
        assert_eq!(
            result
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        assert!(target.pending.load(Ordering::Acquire));
        assert!(retirement.try_recv().is_err());
        release.send(()).unwrap();
        retirement
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
    });
    serving.join().unwrap();
    assert!(!target.pending.load(Ordering::Acquire));
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
}

fn cancelled_request_keeps_inflight_stop_owned(finish: bool) {
    let f = Fixture::new();
    let target = f.target("cancel-target");
    let (started, release, serving) = server(&target);
    let mut pump = f.start();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let cancel = AtomicBool::new(false);
    thread::scope(|scope| {
        let (done, result) = mpsc::sync_channel(1);
        let pump = &pump;
        let cancel = &cancel;
        let id = target.id.clone();
        scope.spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(4);
            let outcome = if finish {
                pump.finish_reconciliation(vec![id], deadline, cancel)
            } else {
                pump.drain_for_reconciliation(vec![id], deadline, cancel)
            };
            done.send(outcome).unwrap();
        });
        assert!(result.recv_timeout(Duration::from_millis(50)).is_err());
        cancel.store(true, Ordering::Release);
        assert_eq!(
            result
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
    });
    assert!(target.pending.load(Ordering::Acquire));
    release.send(()).unwrap();
    serving.join().unwrap();
    wait(|| !target.pending.load(Ordering::Acquire));
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
    assert!(target.listener.accept().is_err());
    pump.shutdown().unwrap();
}

#[test]
fn reconciliation_drain_cancel_keeps_inflight_stop_owned_until_normal_completion() {
    cancelled_request_keeps_inflight_stop_owned(false);
}

#[test]
fn reconciliation_finish_abandonment_keeps_inflight_stop_owned_until_normal_completion() {
    cancelled_request_keeps_inflight_stop_owned(true);
}

#[test]
fn reconciliation_drain_timeout_preserves_emergency_metadata_then_retry_drains_without_resend() {
    let f = Fixture::new();
    let target = f.target("metadata-target");
    let writer = Connection::open(f.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (started, release, serving) = server(&target);
    let mut pump = f.start();
    // A held project mutex must not stall the independent stop owner or drain.
    let db = f.db.lock().unwrap();
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    serving.join().unwrap();
    wait(|| target.runtime.lock().unwrap().session().state() == DeviceState::StopUnconfirmed);
    let error = pump
        .drain_for_reconciliation(
            vec![target.id.clone()],
            Instant::now() + Duration::from_millis(100),
            &AtomicBool::new(false),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(target.pending.load(Ordering::Acquire));
    drop(db);
    writer.execute_batch("COMMIT").unwrap();
    pump.drain_for_reconciliation(
        vec![target.id.clone()],
        Instant::now() + Duration::from_secs(3),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!target.pending.load(Ordering::Acquire));
    let device = target.runtime.lock().unwrap().session().device().clone();
    let journal = f.journal.lock().unwrap();
    let history = journal.history(&device, 10).unwrap();
    assert_eq!(history.iter().filter(|h| h.record.stop).count(), 1);
    let stop = history.iter().find(|h| h.record.stop).unwrap();
    assert!(stop.outcome.is_some());
    assert!(journal.emergency_stop_recorded(&stop.id).unwrap());
    drop(journal);
    assert!(target.listener.accept().is_err());
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
    pump.shutdown().unwrap();
}

#[test]
fn reconciliation_drain_retains_rejected_completion_until_exact_durable_resolution() {
    let f = Fixture::new();
    let target = f.target("rejected-target");
    let resources = f.resources();
    let mut work = HashMap::new();
    collect_pending(&resources, &mut work);
    let w = work.get_mut(&target.id).unwrap();
    let effect = w.stop.as_ref().unwrap().effect.clone();
    let prepared = w
        .runtime
        .lock()
        .unwrap()
        .prepare_stop_durable(effect, &mut f.journal.lock().unwrap())
        .unwrap();
    w.completion = Some(DispatchCompletion {
        request: DispatchRequest::Stop(prepared),
        attempt: DispatchAttempt::Rejected(DispatchFailure::Cancelled),
    });
    w.stop.as_mut().unwrap().submitted = true;
    assert!(!w.step(&resources));
    let mut drain = request(vec![target.id.clone()]);
    assert!(finish_drain(&resources, &mut work, &drain).unwrap());
    assert_eq!(work[&target.id].rejected.len(), 1);
    assert!(target.pending.load(Ordering::Acquire));
    drain.stage = DrainStage::Finish;
    assert!(finish_drain(&resources, &mut work, &drain).is_err());
    assert_eq!(work[&target.id].rejected.len(), 1);
    drain.abandoned.store(true, Ordering::Release);
    assert_eq!(
        finish_drain(&resources, &mut work, &drain)
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
    assert!(work.contains_key(&target.id));
    assert!(target.pending.load(Ordering::Acquire));
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
    let device = target.runtime.lock().unwrap().session().device().clone();
    let before = f.journal.lock().unwrap().history(&device, 10).unwrap();
    let before = before
        .iter()
        .map(|h| {
            (
                h.id.clone(),
                serde_json::to_string(&h.record).unwrap(),
                h.outcome,
            )
        })
        .collect::<Vec<_>>();
    f.resolve(&target);
    drain.abandoned.store(false, Ordering::Release);
    assert!(finish_drain(&resources, &mut work, &drain).unwrap());
    assert!(!work.contains_key(&target.id));
    assert!(!target.pending.load(Ordering::Acquire));
    let after = f.journal.lock().unwrap().history(&device, 10).unwrap();
    let after = after
        .iter()
        .map(|h| {
            (
                h.id.clone(),
                serde_json::to_string(&h.record).unwrap(),
                h.outcome,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(before, after);
    assert!(f.registry.lock().unwrap().entries.contains_key(&target.id));
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
}

#[test]
fn reconciliation_drain_registers_rejected_emergency_intent_without_an_attempted_outcome() {
    let f = Fixture::new();
    let target = f.target("rejected-emergency-target");
    let other = f.target("unrelated-retirement-target");
    let resources = f.resources();
    let mut work = HashMap::new();
    collect_pending(&resources, &mut work);
    let w = work.get_mut(&target.id).unwrap();
    let effect = w.stop.as_ref().unwrap().effect.clone();
    let runtime = w.runtime.lock().unwrap();
    let intent = resources.binding.capture_intent(&runtime, &effect).unwrap();
    let id = intent.record_id().unwrap();
    let stop = runtime
        .prepare_emergency_stop(effect, intent.clone(), &resources.binding)
        .unwrap();
    drop(runtime);
    w.stop.as_mut().unwrap().intent = Some(intent);
    w.stop.as_mut().unwrap().submitted = true;
    w.completion = Some(DispatchCompletion {
        request: DispatchRequest::EmergencyStop(Box::new(stop)),
        attempt: DispatchAttempt::Rejected(DispatchFailure::Cancelled),
    });
    assert!(!w.step(&resources));
    let writer = Connection::open(f.dir.path().join("device-effects/journal.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let drain = request(vec![target.id.clone()]);
    assert!(!finish_drain(&resources, &mut work, &drain).unwrap());
    assert_eq!(work[&target.id].rejected.len(), 1);
    {
        let mut registry = f
            .registry
            .try_lock()
            .expect("contended reconciliation retained the native registry");
        controller::retire_entry(registry.entries.get_mut(&other.id).unwrap());
    }
    assert!(work.contains_key(&other.id));
    assert!(other.pending.load(Ordering::Acquire));
    writer.execute_batch("ROLLBACK").unwrap();
    assert!(finish_drain(&resources, &mut work, &drain).unwrap());
    let device = target.runtime.lock().unwrap().session().device().clone();
    let journal = f.journal.lock().unwrap();
    let history = journal.history(&device, 10).unwrap();
    let original = history.iter().find(|h| h.id == id).unwrap();
    assert!(original.record.stop);
    assert!(original.outcome.is_none());
    assert!(!journal.emergency_stop_recorded(&id).unwrap());
    assert_eq!(work[&target.id].rejected.len(), 1);
    assert!(target.pending.load(Ordering::Acquire));
    assert!(target
        .runtime
        .lock()
        .unwrap()
        .session()
        .physical_stop_pending());
    assert!(target.listener.accept().is_err());
}
