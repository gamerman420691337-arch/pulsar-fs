// Explicit native IPC scenario; authored source bytes and declared clocks do
// not establish rendered acquisition, learned motion or device qualification.
struct NativeTimingEngine(std::process::Child);
impl Drop for NativeTimingEngine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires explicitly pinned App and Worker; runs a private native Engine"]
fn native_media_retirement_probe_failure_settles_after_exact_timing_release() {
    use crate::{motion::ResponseBody as ClientBody, session::EngineApi};
    use pulsar_protocol::{
        BrowserMediaPreparationState, DeclaredSourceClock, ProjectTime, SourcePlacementSpec,
        SourceTimestamp,
    };
    use std::process::{Command as ProcessCommand, Stdio};

    let app = std::env::var_os("PULSAR_TIMING_NATIVE_APP").expect("explicit native App path");
    let worker =
        std::env::var_os("PULSAR_TIMING_NATIVE_WORKER").expect("explicit native Worker path");
    let root = tempfile::tempdir().unwrap();
    let state_dir = root.path().join("state");
    let log = std::fs::File::create(root.path().join("engine.log")).unwrap();
    let mut engine = NativeTimingEngine(
        ProcessCommand::new(&app)
            .arg("engine")
            .env("PULSAR_STATE_DIR", &state_dir)
            .env("PULSAR_WORKER_EXECUTABLE", &worker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let endpoint = state_dir.join("engine.sock");
    let end = Instant::now() + Duration::from_secs(10);
    while !endpoint.exists() {
        assert!(
            engine.0.try_wait().unwrap().is_none(),
            "private Engine exited: {}",
            std::fs::read_to_string(root.path().join("engine.log")).unwrap()
        );
        assert!(Instant::now() < end, "private Engine readiness deadline");
        thread::sleep(Duration::from_millis(5));
    }
    let mut client = SessionClient::connect(&endpoint, &state_dir.join("pairing.token")).unwrap();
    let ClientBody::Project(project) = client
        .execute(
            Command::CreateProject {
                name: "Native timing retirement".into(),
            },
            None,
            None,
        )
        .unwrap()
    else {
        panic!("project")
    };
    let project = project.project_id;
    let source_path = root.path().join("authored-source.bin");
    std::fs::write(
        &source_path,
        b"native timing source identity; no rendered pixels",
    )
    .unwrap();
    let ClientBody::Source { source_version } = client
        .execute(
            Command::ImportSource { path: source_path },
            Some(project.clone()),
            None,
        )
        .unwrap()
    else {
        panic!("source")
    };
    client
        .execute(
            Command::SetSourcePlacement {
                spec: SourcePlacementSpec {
                    source_version,
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(10, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    project_anchor: ProjectTime::from_nanos(0),
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
                replace: None,
            },
            Some(project.clone()),
            Some(pulsar_protocol::RevisionId::new(0)),
        )
        .unwrap();
    let ClientBody::BrowserMediaCatalog(catalog) = client
        .execute(
            Command::BrowserMediaCatalog,
            Some(project.clone()),
            Some(pulsar_protocol::RevisionId::new(1)),
        )
        .unwrap()
    else {
        panic!("media catalog")
    };
    assert_eq!(catalog.entries.len(), 1);
    let original = new_request_id();
    client
        .execute_identified(
            original.clone(),
            Command::PrepareBrowserMedia {
                engine_epoch: catalog.engine_epoch,
                selection: catalog.entries[0].selection.clone(),
                clock: DeclaredSourceClock {
                    presentation_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
                },
                lease_ms: 30_000,
                allow_declared_clock: true,
            },
            Some(project.clone()),
            Some(pulsar_protocol::RevisionId::new(1)),
        )
        .unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    let media = loop {
        let ClientBody::BrowserMediaPreparation(report) = client
            .execute(
                Command::BrowserMediaPreparationStatus {
                    original_request_id: original.clone(),
                },
                Some(project.clone()),
                None,
            )
            .unwrap()
        else {
            panic!("preparation status")
        };
        if report.state == BrowserMediaPreparationState::Ready {
            break report.binding.unwrap();
        }
        assert!(
            Instant::now() < end,
            "native media preparation: {:?}",
            report.state
        );
        thread::sleep(Duration::from_millis(2));
    };
    let mut timing = Timing::new(&client).unwrap();
    let player = player();
    timing
        .open(player.clone(), Arc::new(AtomicBool::new(false)), media)
        .unwrap();
    ready(&timing);
    client
        .execute(
            Command::ReleaseBrowserMedia {
                original_request_id: original,
            },
            Some(project),
            None,
        )
        .unwrap();
    timing
        .handle("/player/timing-probe", &player, b"", 1)
        .unwrap();
    assert!(reply(&timing, &player, 1).is_err());
    let cleanup = timing.shutdown();
    let diagnostic = {
        let records = state(&timing.shared);
        let record = &records.entries[0];
        assert!(record.retired && record.release == ReleaseState::Acknowledged);
        assert!(!record.report.as_ref().unwrap().current);
        assert_eq!(
            record.first_failure.as_ref().unwrap().category,
            "binding_not_current"
        );
        diagnostic(record)
    };
    assert!(
        cleanup.is_ok(),
        "native exact release must settle cleanup: {cleanup:?}"
    );
    // The unreaped Child handle pins this PID until wait; signal only this
    // campaign's Engine and require its ordinary shutdown to exit successfully.
    assert!(engine.0.try_wait().unwrap().is_none());
    let status = ProcessCommand::new("/bin/kill")
        .args(["-TERM", &engine.0.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let end = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = engine.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < end, "private Engine did not join");
        thread::sleep(Duration::from_millis(5));
    };
    assert!(status.success());
    if let Some(path) = std::env::var_os("PULSAR_TIMING_NATIVE_RECEIPT") {
        std::fs::write(path, serde_json::to_vec_pretty(&json!({"status":"PASS", "qualified":false,
            "physical_device_contacted":false, "rendered_acquisition_verified":false,
            "scope":"Actual Engine media release rejects an ordinary timing probe; exact native timing release settles custody while retaining the rejection diagnostic.",
            "diagnostic":diagnostic, "exact_native_release_acknowledged":true,
            "cleanup_confirmed":true, "engine_exit_code":status.code()})).unwrap()).unwrap();
    }
}
