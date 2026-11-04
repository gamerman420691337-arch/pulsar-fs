use super::*;

fn profile_fixture() -> Fixture {
    let Fixture {
        engine,
        session,
        project,
        dir,
    } = fixture();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut pending = engine;
    let mut owned = loop {
        match Arc::try_unwrap(pending) {
            Ok(engine) => break engine,
            Err(engine) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "fixture sweeper did not release Engine"
                );
                pending = engine;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    };
    // Old weak references cannot reach the moved Engine. Join them before replacing
    // task ownership; these budget tests never execute model inference.
    owned.background.join().unwrap();
    owned.background = crate::task_group::TaskGroup::default();
    let path = dir.path().join("nonexecuted-runtime");
    fs::write(&path, b"RAM-budget runtime identity fixture").unwrap();
    let (sha256, byte_len) = artifacts::digest_file(&path, 1024).unwrap();
    owned.tools.as_mut().unwrap().onnx_runtime = Some(ArtifactReference {
        path,
        identity: ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len,
        },
    });
    Fixture {
        engine: Arc::new(owned),
        session,
        project,
        dir,
    }
}

fn operation(f: &Fixture, minimum_memory_bytes: u64) -> (SourceVersionId, WorkerOperation) {
    let source_path = f.dir.path().join("pair.rgbpair");
    fs::write(&source_path, vec![0; 64 * 64 * 3 * 2]).unwrap();
    let ResponseBody::Source { source_version } = f
        .engine
        .handle(request(
            &f.session,
            Some(&f.project),
            None,
            Command::ImportSource { path: source_path },
        ))
        .result
        .unwrap()
    else {
        panic!("source")
    };
    // Identity/preparation fixture only: no model inference or quality claim.
    let model_path = f.dir.path().join("nonexecuted.onnx");
    fs::write(&model_path, b"profile budget identity fixture").unwrap();
    let model = f.engine.snapshot_model(&model_path).unwrap();
    let input = ModelInputContract {
        width: 64,
        height: 64,
        channels: 3,
        class_count: 1,
        layout: TensorLayout::Nchw,
        decoder: DetectorDecoder::YoloChannelMajor,
    };
    let profile = DeclaredInferenceProfile {
        schema_version: 1,
        model: model.identity.clone(),
        runtime: f
            .engine
            .tools
            .as_ref()
            .unwrap()
            .onnx_runtime
            .as_ref()
            .unwrap()
            .identity
            .clone(),
        preprocessing: InferencePreprocessing {
            input: input.clone(),
            resize: InferenceResize::Stretch,
            color: InferenceColor::Rgb,
            scale: vec![1.0 / 255.0; 3],
            bias: vec![0.0; 3],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cpu,
        minimum_memory_bytes,
        minimum_output_bytes: 1024 * 1024,
        cpu_threads: 2,
        cuda_resources: None,
    };
    (
        source_version,
        WorkerOperation::Generate {
            preset: "default".into(),
            settings: Box::new(GenerationSettings {
                analysis_width: 64,
                analysis_height: 64,
                model_input: Some(input),
                inference_profile: Some(profile),
                ..Default::default()
            }),
            model: Some(model),
        },
    )
}

#[test]
fn declared_ram_reaches_durable_manifest_and_aggregate_reservation() {
    let f = profile_fixture();
    let (source, operation) = operation(&f, 3 << 30);
    let req = request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::Generate {
            source_version: source.clone(),
            preset: "default".into(),
            settings: None,
            model_path: None,
        },
    );
    let mut db = f.engine.db.lock().unwrap();
    let tx = db.transaction().unwrap();
    let (manifest, reservation, _) = f
        .engine
        .prepare_operation(&tx, &req, &source, operation, None)
        .unwrap();
    assert_eq!(manifest.budget.memory_bytes, 3 << 30);
    assert_eq!(manifest.budget.output_bytes, OUTPUT_LIMIT);
    assert_eq!(manifest.budget.cpu_threads, 2);
    let raw: String = tx
        .query_row(
            "SELECT manifest FROM attempts WHERE id=?1",
            [manifest.attempt_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    let stored: WorkerRequest = decode(&raw).unwrap();
    assert_eq!(stored.budget.memory_bytes, 3 << 30);
    assert_eq!(
        f.engine.pool.reserved_cpu_threads().unwrap(),
        u32::from(manifest.budget.cpu_threads)
    );
    // Plenty of RAM/job capacity for this tiny claim; CPU alone refuses it.
    assert_eq!(
        f.engine.pool.reserve_worker(1, 3).err().unwrap().code,
        ErrorCode::ResourceExhausted
    );
    assert!(f.engine.pool.reserve(WORKER_MEMORY).is_err());
    drop(reservation);
    assert_eq!(f.engine.pool.reserved_cpu_threads().unwrap(), 0);
    assert!(f.engine.pool.reserve_worker(1, 4).is_ok());
    assert!(f.engine.pool.reserve(WORKER_MEMORY).is_ok());
}

#[test]
fn over_owner_ram_refuses_before_job_attempt_or_output_creation() {
    let f = profile_fixture();
    let (source, operation) = operation(&f, (4 << 30) + 1);
    let req = request(
        &f.session,
        Some(&f.project),
        Some(0),
        Command::Generate {
            source_version: source.clone(),
            preset: "default".into(),
            settings: None,
            model_path: None,
        },
    );
    let mut db = f.engine.db.lock().unwrap();
    let tx = db.transaction().unwrap();
    let result = f
        .engine
        .prepare_operation(&tx, &req, &source, operation, None);
    let Err(error) = result else {
        panic!("over-owner RAM admitted")
    };
    assert_eq!(error.code, ErrorCode::ResourceExhausted);
    for table in ["jobs", "attempts"] {
        let n: u64 = tx
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
    assert_eq!(
        fs::read_dir(f.engine.config.state_dir.join("attempts"))
            .unwrap()
            .count(),
        0
    );
}
