// Authored public Engine generation, using the real worker and retained portable closure.
pub(crate) fn verify_public_annotated_generation(example: &WorkerRequest) {
    assert!(launch::available(), "requires admitted Linux confinement");
    let dir = tempfile::tempdir().unwrap();
    let binary = PathBuf::from(
        std::env::var_os("PULSAR_ANNOTATED_ENGINE_WORKER").expect("current worker executable"),
    );
    let mut config = EngineConfig::new(dir.path().join("state"), binary);
    config.worker_timeout = std::time::Duration::from_secs(60);
    let engine = Engine::open(config.clone()).unwrap();
    let session = pair(&engine, "annotated public campaign");
    let ResponseBody::Project(project) = engine
        .handle(request(
            &session,
            None,
            None,
            Command::CreateProject {
                name: "Authored annotated generation".into(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("project")
    };
    let project = project.project_id;
    let ResponseBody::Source { source_version } = engine
        .handle(request(
            &session,
            Some(&project),
            Some(0),
            Command::ImportSource {
                path: example.source.path.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("source")
    };
    let WorkerOperation::Generate {
        settings,
        model: Some(model),
        preset,
    } = &example.operation
    else {
        panic!("generation")
    };
    let mut settings = settings.clone();
    settings.inference_profile.as_mut().unwrap().cpu_threads = 2;
    let mut spec = settings.source_placement.as_ref().unwrap().spec.clone();
    spec.source_version = source_version.clone();
    let ResponseBody::Project(state) = engine
        .handle(request(
            &session,
            Some(&project),
            Some(0),
            Command::SetSourcePlacement {
                spec,
                replace: None,
            },
        ))
        .result
        .unwrap()
    else {
        panic!("placement")
    };
    let base_revision = state.revision;
    settings.source_placement = Some(state.placements[0].clone());
    let snapshot_count = fs::read_dir(config.state_dir.join("snapshots"))
        .unwrap()
        .count();
    let mut bad = settings.clone();
    bad.annotated_view.as_mut().unwrap().source_sha256 = "f".repeat(64);
    let error = engine
        .handle(request(
            &session,
            Some(&project),
            Some(base_revision.value()),
            Command::Generate {
                source_version: source_version.clone(),
                preset: preset.clone(),
                settings: Some(*bad),
                model_path: Some(dir.path().join("absent.onnx")),
            },
        ))
        .result
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DependencyMismatch);
    assert_eq!(
        fs::read_dir(config.state_dir.join("snapshots"))
            .unwrap()
            .count(),
        snapshot_count
    );
    let submit = request(
        &session,
        Some(&project),
        Some(base_revision.value()),
        Command::Generate {
            source_version,
            preset: preset.clone(),
            settings: Some(*settings),
            model_path: Some(model.path.clone()),
        },
    );
    let ResponseBody::Job(start) = engine.handle(submit.clone()).result.unwrap() else {
        panic!("job")
    };
    let ResponseBody::Job(replay) = engine.handle(submit).result.unwrap() else {
        panic!("job replay")
    };
    assert_eq!(start.job_id, replay.job_id);
    assert_eq!(start.attempt_id, replay.attempt_id);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(65);
    let done = loop {
        let ResponseBody::Job(job) = engine
            .handle(request(
                &session,
                Some(&project),
                None,
                Command::JobStatus {
                    job_id: start.job_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("status")
        };
        if job.state == "completed" {
            break job;
        }
        assert!(
            !["failed", "cancelled", "interrupted"].contains(&job.state.as_str()),
            "{job:?}"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "job deadline: {job:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let candidate = done.candidate_id.unwrap();
    let ResponseBody::Project(state) = engine
        .handle(request(
            &session,
            Some(&project),
            Some(base_revision.value()),
            Command::CommitCandidate {
                candidate_id: candidate.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("commit")
    };
    assert_eq!(state.revision, base_revision.checked_next().unwrap());
    let committed_revision = state.revision;
    while !engine.workers.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    drop(engine);
    let engine = reopen_after_transient_leases(config.clone()).unwrap();
    // The original persisted session retains its project grants across restart.
    let ResponseBody::Job(recovered) = engine
        .handle(request(
            &session,
            Some(&project),
            None,
            Command::JobStatus {
                job_id: start.job_id,
            },
        ))
        .result
        .unwrap()
    else {
        panic!("recovered job")
    };
    assert_eq!(recovered.state, "completed");
    assert_eq!(recovered.candidate_id, Some(candidate));
    let token = TOKENS.with(|tokens| tokens.borrow()[session.as_str()].clone());
    let plan =
        project_packages::capture(&engine, &session, &token, &project, committed_revision).unwrap();
    assert_eq!(plan.manifest.format_version, 11);
    assert_eq!(plan.manifest.generated_origins.len(), 1);
    let retained_model = plan.manifest.generated_origins[0]
        .annotated_model
        .clone()
        .unwrap();
    assert_eq!(retained_model.sha256, model.identity.sha256);
    let mut bytes = Vec::new();
    crate::project_package_format::write_package(
        &plan.manifest,
        &mut bytes,
        |object| plan.open_object(object),
        crate::project_package_format::PackageCodecLimits {
            max_package_bytes: DEFAULT_MAX_PROJECT_PACKAGE_BYTES,
        },
    )
    .unwrap();
    drop(plan);
    drop(engine);
    crate::authority::project_package_imports::exercise_authored_annotated_portable_lifecycle(
        &bytes,
        &retained_model,
        None,
    );
}
