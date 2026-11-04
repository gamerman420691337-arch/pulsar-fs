//! Synthetic native socket responses; no real controller or device qualification.
use super::*;
use std::os::unix::{
    fs::PermissionsExt,
    net::{UnixListener, UnixStream},
};
fn accept(listener: &UnixListener) -> UnixStream {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                s.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                return s;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < end, "synthetic socket accept deadline");
                thread::sleep(Duration::from_millis(2));
            }
            Err(e) => panic!("synthetic socket accept failed: {e}"),
        }
    }
}
struct Joined<T>(Option<thread::JoinHandle<T>>);
impl<T> Drop for Joined<T> {
    fn drop(&mut self) {
        if let Some(t) = self.0.take() {
            let _ = t.join();
        }
    }
}

#[test]
fn blocked_or_lost_bind_never_blocks_local_stop_and_late_success_releases_exact_binding() {
    for lose_reply in [false, true] {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let proof = root.path().join("handy-owner.token");
        std::fs::write(&proof, "a".repeat(64)).unwrap();
        std::fs::set_permissions(&proof, std::fs::Permissions::from_mode(0o600)).unwrap();
        let endpoint = root.path().join("engine.sock");
        let listener = UnixListener::bind(&endpoint).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (entered_tx, entered) = mpsc::sync_channel(1);
        let (release, release_rx) = mpsc::sync_channel(1);
        let server = thread::spawn(move || {
            let mut socket = accept(&listener);
            let bind: Request = read_message(&mut socket).unwrap();
            let Command::BindHandyPlayerGuard {
                selection,
                player,
                policy,
                allow_monitoring: true,
                ..
            } = &bind.command
            else {
                panic!("expected one original binding")
            };
            let mut controller: HandyControllerReceipt = serde_json::from_str(include_str!(
                "../../pulsar-protocol/tests/fixtures/handy-controller.json"
            ))
            .unwrap();
            controller.lease.state = HandyControllerState::Playing;
            controller.lease.physical_stop_pending = true;
            let report = HandyPlayerGuardReport {
                binding: HandyPlayerGuardBinding {
                    request_id: bind.request_id.clone(),
                    binding_id: ArtifactId::new("b".repeat(64)).unwrap(),
                    selection: selection.clone(),
                    player: player.clone(),
                    policy: policy.clone(),
                    browser_media: None,
                    controller: controller.lease,
                },
                bound_at_boottime_ns: 1,
                heartbeat_deadline_boottime_ns: 1000,
                media_progress_deadline_boottime_ns: 1000,
                last_sequence: 0,
                last_sample: None,
                current: true,
                synchronized: false,
                playback_enabled: false,
                physical_stop_pending: true,
            };
            report.validate().unwrap();
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            let reply_id = if lose_reply {
                drop(socket);
                socket = accept(&listener);
                let status: Request = read_message(&mut socket).unwrap();
                assert!(
                    matches!(status.command,Command::HandyPlayerGuardStatus {binding_request_id,..} if binding_request_id==bind.request_id)
                );
                status.request_id
            } else {
                bind.request_id.clone()
            };
            write_message(
                &mut socket,
                &Response::success(
                    reply_id,
                    pulsar_protocol::ResponseBody::HandyPlayerGuard(Box::new(report.clone())),
                ),
            )
            .unwrap();
            drop(socket);
            let mut socket = accept(&listener);
            let released: Request = read_message(&mut socket).unwrap();
            assert!(
                matches!(&released.command,Command::ReleaseHandyPlayerGuard {binding_id,player,..} if binding_id==&report.binding.binding_id && player==&report.binding.player)
            );
            let mut retired = report.clone();
            retired.current = false;
            write_message(
                &mut socket,
                &Response::success(
                    released.request_id,
                    pulsar_protocol::ResponseBody::HandyPlayerGuard(Box::new(retired)),
                ),
            )
            .unwrap();
            (bind.request_id, report.binding.binding_id)
        });
        let mut server = Joined(Some(server));
        let client = SessionClient::from_credentials(
            &endpoint,
            SessionId::new("synthetic-guard-session").unwrap(),
            "synthetic-guard-authentication".into(),
        )
        .unwrap();
        let config = GuardConfig {
            selection: HandyContinuousTimingSelection {
                acquisition_request_id: RequestId::new("acquire-fixture").unwrap(),
                controller_id: ClientId::new("fixture-controller").unwrap(),
                expected_generation: 0,
                play_request_id: RequestId::new("play-fixture").unwrap(),
                play_record_id: ArtifactId::new("c".repeat(64)).unwrap(),
            },
            policy: HandyPlayerGuardPolicy {
                heartbeat_timeout_ms: 1000,
                media_stall_timeout_ms: 1000,
                challenge_timeout_ms: 500,
            },
        };
        let mut control = PlayerControl::new(
            &client,
            ProjectId::new("fixture-project").unwrap(),
            RevisionId::new(1),
            Some(config),
        )
        .unwrap();
        let page = control.page().unwrap();
        let mut scope = Scope {
            launch: page["launch_id"].as_str().unwrap().into(),
            page: page["page_id"].as_str().unwrap().into(),
            page_capability: page["page_capability"].as_str().unwrap().into(),
            ..Scope::default()
        };
        let ControlReply::Ready(value)=control.handle("/player/start",&scope,br#"{"source_id":"synthetic-source","generation":1,"time_basis":"source_media_presentation"}"#,1).unwrap() else {panic!()};
        scope.player = Some(value["player"]["player_id"].as_str().unwrap().into());
        scope.source = Some("synthetic-source".into());
        scope.generation = Some(1);
        scope.player_capability = Some(value["player_capability"].as_str().unwrap().into());
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let ControlReply::Ready(pending) =
            control.handle("/player/challenge", &scope, b"", 2).unwrap()
        else {
            panic!()
        };
        assert_eq!(pending["pending"], true);
        assert!(!control
            .player
            .as_ref()
            .unwrap()
            .binding
            .as_ref()
            .is_some_and(|r| r.current));
        let start = Instant::now();
        assert!(matches!(
            control.handle("/player/stop", &scope, b"", 3).unwrap(),
            ControlReply::Ready(_)
        ));
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(!control.active());
        release.send(()).unwrap();
        wait_for(&mut control, |control| {
            records(&control.native.as_ref().unwrap().diagnostics)
                .records
                .iter()
                .any(|r| r.diagnostic.phase == GuardPhase::Released)
        });
        // Binder already released the cancelled scope; the queued successful Bind
        // event must not schedule a second Release or downgrade its checked ack.
        control.poll();
        control.poll();
        control.shutdown().unwrap();
        assert!(control.native.as_ref().unwrap().threads.is_empty());
        assert!(control.native.as_ref().unwrap().failure().is_none());
        let (_, binding) = server.0.take().unwrap().join().unwrap();
        assert_eq!(binding.as_str(), "b".repeat(64));
    }
}

fn wait_for(control: &mut PlayerControl, done: impl Fn(&PlayerControl) -> bool) {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        control.poll();
        if done(control) {
            return;
        }
        assert!(Instant::now() < end, "synthetic guard operation deadline");
        thread::sleep(Duration::from_millis(2));
    }
}
fn report_for(bind: &Request) -> HandyPlayerGuardReport {
    let Command::BindHandyPlayerGuard {
        selection,
        player,
        policy,
        allow_monitoring: true,
        ..
    } = &bind.command
    else {
        panic!("expected original Bind only")
    };
    let mut controller: HandyControllerReceipt = serde_json::from_str(include_str!(
        "../../pulsar-protocol/tests/fixtures/handy-controller.json"
    ))
    .unwrap();
    controller.lease.state = HandyControllerState::Playing;
    controller.lease.physical_stop_pending = true;
    let report = HandyPlayerGuardReport {
        binding: HandyPlayerGuardBinding {
            request_id: bind.request_id.clone(),
            binding_id: ArtifactId::new("b".repeat(64)).unwrap(),
            selection: selection.clone(),
            player: player.clone(),
            policy: policy.clone(),
            browser_media: None,
            controller: controller.lease,
        },
        bound_at_boottime_ns: 1,
        heartbeat_deadline_boottime_ns: 1000,
        media_progress_deadline_boottime_ns: 1000,
        last_sequence: 0,
        last_sample: None,
        current: true,
        synchronized: false,
        playback_enabled: false,
        physical_stop_pending: true,
    };
    report.validate().unwrap();
    report
}
fn fixture() -> (tempfile::TempDir, UnixListener, PlayerControl, Scope) {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let proof = root.path().join("handy-owner.token");
    std::fs::write(&proof, "a".repeat(64)).unwrap();
    std::fs::set_permissions(&proof, std::fs::Permissions::from_mode(0o600)).unwrap();
    let endpoint = root.path().join("engine.sock");
    let listener = UnixListener::bind(&endpoint).unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = SessionClient::from_credentials(
        &endpoint,
        SessionId::new("synthetic-guard-session").unwrap(),
        "synthetic-guard-authentication".into(),
    )
    .unwrap();
    let config = GuardConfig {
        selection: HandyContinuousTimingSelection {
            acquisition_request_id: RequestId::new("acquire-fixture").unwrap(),
            controller_id: ClientId::new("fixture-controller").unwrap(),
            expected_generation: 0,
            play_request_id: RequestId::new("play-fixture").unwrap(),
            play_record_id: ArtifactId::new("c".repeat(64)).unwrap(),
        },
        policy: HandyPlayerGuardPolicy {
            heartbeat_timeout_ms: 1000,
            media_stall_timeout_ms: 1000,
            challenge_timeout_ms: 500,
        },
    };
    let mut control = PlayerControl::new(
        &client,
        ProjectId::new("fixture-project").unwrap(),
        RevisionId::new(1),
        Some(config),
    )
    .unwrap();
    let page = control.page().unwrap();
    let scope = Scope {
        launch: page["launch_id"].as_str().unwrap().into(),
        page: page["page_id"].as_str().unwrap().into(),
        page_capability: page["page_capability"].as_str().unwrap().into(),
        ..Scope::default()
    };
    (root, listener, control, scope)
}
fn begin(control: &mut PlayerControl, scope: &mut Scope) {
    let ControlReply::Ready(value)=control.handle("/player/start",scope,br#"{"source_id":"synthetic-source","generation":1,"time_basis":"source_media_presentation"}"#,1).unwrap() else {panic!()};
    scope.player = Some(value["player"]["player_id"].as_str().unwrap().into());
    scope.source = Some("synthetic-source".into());
    scope.generation = Some(1);
    scope.player_capability = Some(value["player_capability"].as_str().unwrap().into());
}
fn assert_sanitized(message: &str) {
    let proof = "a".repeat(64);
    for forbidden in [
        "synthetic-guard-authentication",
        proof.as_str(),
        "malicious-remote-secret",
    ] {
        assert!(!message.contains(forbidden));
    }
    assert!(message.contains("physical stopping is unconfirmed"));
}
#[derive(Clone, Copy, Debug)]
enum BadRelease {
    Dropped,
    ForeignBinding,
    ForeignPlayer,
    StillCurrent,
    WrongKind,
    CredentialError,
}
#[test]
fn release_uncertainty_keeps_original_subject_and_joins_without_replacement_bind() {
    for case in [
        BadRelease::Dropped,
        BadRelease::ForeignBinding,
        BadRelease::ForeignPlayer,
        BadRelease::StillCurrent,
        BadRelease::WrongKind,
        BadRelease::CredentialError,
    ] {
        let (_root, listener, mut control, mut scope) = fixture();
        let (finish, finished) = mpsc::sync_channel(1);
        let (identity_tx, identity_rx) = mpsc::sync_channel(1);
        let mut server = Joined(Some(thread::spawn(move || {
            let mut socket = accept(&listener);
            let bind: Request = read_message(&mut socket).unwrap();
            let report = report_for(&bind);
            identity_tx
                .send((
                    bind.request_id.clone(),
                    report.binding.binding_id.clone(),
                    report.binding.player.player_id.clone(),
                ))
                .unwrap();
            write_message(
                &mut socket,
                &Response::success(
                    bind.request_id,
                    pulsar_protocol::ResponseBody::HandyPlayerGuard(Box::new(report.clone())),
                ),
            )
            .unwrap();
            drop(socket);
            let mut socket = accept(&listener);
            let released: Request = read_message(&mut socket).unwrap();
            assert!(
                matches!(&released.command,Command::ReleaseHandyPlayerGuard {binding_id,player,..} if binding_id==&report.binding.binding_id && player==&report.binding.player)
            );
            if !matches!(case, BadRelease::Dropped) {
                let mut reply = report;
                reply.current = false;
                match case {
                    BadRelease::ForeignBinding => {
                        reply.binding.binding_id = ArtifactId::new("d".repeat(64)).unwrap()
                    }
                    BadRelease::ForeignPlayer => {
                        reply.binding.player.player_id = AttemptId::new("foreign-player").unwrap()
                    }
                    BadRelease::StillCurrent => reply.current = true,
                    _ => {}
                }
                let response = if matches!(case, BadRelease::CredentialError) {
                    Response {
                        version: PROTOCOL_VERSION,
                        request_id: released.request_id,
                        result: Err(ProtocolError::new(
                            ErrorCode::Unavailable,
                            format!(
                                "malicious-remote-secret synthetic-guard-authentication {}",
                                "a".repeat(64)
                            ),
                        )),
                    }
                } else {
                    Response::success(
                        released.request_id,
                        if matches!(case, BadRelease::WrongKind) {
                            pulsar_protocol::ResponseBody::Ack
                        } else {
                            pulsar_protocol::ResponseBody::HandyPlayerGuard(Box::new(reply))
                        },
                    )
                };
                write_message(&mut socket, &response).unwrap();
            }
            drop(socket);
            finished.recv_timeout(Duration::from_secs(2)).unwrap();
            assert!(
                matches!(listener.accept(),Err(e)if e.kind()==std::io::ErrorKind::WouldBlock),
                "unexpected replacement/repeated operation after {case:?}"
            );
        })));
        begin(&mut control, &mut scope);
        let (original, binding, player) = identity_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        wait_for(&mut control, |c| {
            c.player.as_ref().unwrap().binding.is_some()
        });
        control.handle("/player/stop", &scope, b"", 2).unwrap();
        wait_for(&mut control, |c| {
            c.native.as_ref().unwrap().failure().is_some()
        });
        let message = control
            .handle("/player/challenge", &scope, b"", 3)
            .err()
            .unwrap()
            .to_string();
        for id in [original.as_str(), binding.as_str(), player.as_str()] {
            assert!(message.contains(id));
        }
        assert_sanitized(&message);
        let replacement=control.handle("/player/start",&scope,br#"{"source_id":"replacement-source","generation":2,"time_basis":"source_media_presentation"}"#,4).err().unwrap().to_string();
        assert!(replacement.contains(original.as_str()));
        let shutdown = control.shutdown().unwrap_err().to_string();
        assert!(shutdown.contains(original.as_str()));
        assert_sanitized(&shutdown);
        assert!(control.native.as_ref().unwrap().threads.is_empty());
        assert_eq!(
            records(&control.native.as_ref().unwrap().diagnostics).records[0]
                .diagnostic
                .phase,
            GuardPhase::Uncertain
        );
        finish.send(()).unwrap();
        server.0.take().unwrap().join().unwrap();
    }
}

#[test]
fn failed_bind_and_original_status_retain_id_and_never_dispatch_replacement_bind() {
    let (_root, listener, mut control, mut scope) = fixture();
    let (sent, identity) = mpsc::sync_channel(1);
    let (finish, finished) = mpsc::sync_channel(1);
    let mut server = Joined(Some(thread::spawn(move || {
        let mut socket = accept(&listener);
        let bind: Request = read_message(&mut socket).unwrap();
        assert!(matches!(bind.command, Command::BindHandyPlayerGuard { .. }));
        sent.send(bind.request_id.clone()).unwrap();
        drop(socket);
        let mut socket = accept(&listener);
        let status: Request = read_message(&mut socket).unwrap();
        assert!(
            matches!(status.command,Command::HandyPlayerGuardStatus {binding_request_id,..} if binding_request_id==bind.request_id)
        );
        write_message(
            &mut socket,
            &Response {
                version: PROTOCOL_VERSION,
                request_id: status.request_id,
                result: Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "malicious-remote-secret synthetic-guard-authentication",
                )),
            },
        )
        .unwrap();
        drop(socket);
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(listener.accept(),Err(e)if e.kind()==std::io::ErrorKind::WouldBlock));
    })));
    begin(&mut control, &mut scope);
    let original = identity.recv_timeout(Duration::from_secs(2)).unwrap();
    wait_for(&mut control, |c| {
        c.native.as_ref().unwrap().failure().is_some()
    });
    for path in ["/player/challenge", "/player/start"] {
        let error=control.handle(path,&scope,br#"{"source_id":"replacement","generation":2,"time_basis":"source_media_presentation"}"#,5).err().unwrap().to_string();
        assert!(error.contains(original.as_str()));
        assert_sanitized(&error);
    }
    let error = control.shutdown().unwrap_err().to_string();
    assert!(error.contains(original.as_str()));
    assert!(error.contains("bind_and_original_status_unconfirmed"));
    assert_sanitized(&error);
    assert!(control.native.as_ref().unwrap().threads.is_empty());
    assert!(
        records(&control.native.as_ref().unwrap().diagnostics).records[0]
            .diagnostic
            .binding_id
            .is_none()
    );
    finish.send(()).unwrap();
    server.0.take().unwrap().join().unwrap();
}

#[test]
fn shutdown_releases_successful_bind_even_without_polling_its_event() {
    let (_root, listener, mut control, mut scope) = fixture();
    let mut server = Joined(Some(thread::spawn(move || {
        let mut socket = accept(&listener);
        let bind: Request = read_message(&mut socket).unwrap();
        let mut report = report_for(&bind);
        write_message(
            &mut socket,
            &Response::success(
                bind.request_id,
                pulsar_protocol::ResponseBody::HandyPlayerGuard(Box::new(report.clone())),
            ),
        )
        .unwrap();
        drop(socket);
        let mut socket = accept(&listener);
        let release: Request = read_message(&mut socket).unwrap();
        assert!(
            matches!(&release.command,Command::ReleaseHandyPlayerGuard {binding_id,player,..} if binding_id==&report.binding.binding_id && player==&report.binding.player)
        );
        report.current = false;
        write_message(
            &mut socket,
            &Response::success(
                release.request_id,
                pulsar_protocol::ResponseBody::HandyPlayerGuard(Box::new(report)),
            ),
        )
        .unwrap();
    })));
    begin(&mut control, &mut scope);
    let event = control
        .native
        .as_ref()
        .unwrap()
        .events
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(event.result.is_ok());
    assert!(control.player.as_ref().unwrap().binding.is_none());
    control.shutdown().unwrap();
    assert!(control.native.as_ref().unwrap().threads.is_empty());
    assert_eq!(
        records(&control.native.as_ref().unwrap().diagnostics).records[0]
            .diagnostic
            .phase,
        GuardPhase::Released
    );
    server.0.take().unwrap().join().unwrap();
}

#[test]
fn disconnected_release_queue_is_reported_and_does_not_silently_retry_at_join() {
    let (_root, listener, mut control, mut scope) = fixture();
    let (finish, finished) = mpsc::sync_channel(1);
    let mut server = Joined(Some(thread::spawn(move || {
        let mut socket = accept(&listener);
        let bind: Request = read_message(&mut socket).unwrap();
        let report = report_for(&bind);
        let original = bind.request_id.clone();
        write_message(
            &mut socket,
            &Response::success(
                bind.request_id,
                pulsar_protocol::ResponseBody::HandyPlayerGuard(Box::new(report)),
            ),
        )
        .unwrap();
        drop(socket);
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(listener.accept(),Err(e)if e.kind()==std::io::ErrorKind::WouldBlock));
        original
    })));
    begin(&mut control, &mut scope);
    wait_for(&mut control, |c| {
        c.player.as_ref().unwrap().binding.is_some()
    });
    let original = control
        .player
        .as_ref()
        .unwrap()
        .binding
        .as_ref()
        .unwrap()
        .binding
        .request_id
        .clone();
    control.native.as_mut().unwrap().releases.take();
    let error = control
        .handle("/player/stop", &scope, b"", 2)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("release_queue_unavailable") && error.contains(original.as_str()));
    assert!(!control.active());
    let shutdown = control.shutdown().unwrap_err().to_string();
    assert!(shutdown.contains(original.as_str()));
    assert!(control.native.as_ref().unwrap().threads.is_empty());
    finish.send(()).unwrap();
    assert_eq!(server.0.take().unwrap().join().unwrap(), original);
}
