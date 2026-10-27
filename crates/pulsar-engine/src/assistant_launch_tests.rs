use super::*;
use sha2::{Digest, Sha256};
use std::{os::unix::fs::PermissionsExt, path::Path};
fn identity(path: &Path) -> ArtifactIdentity {
    let bytes = std::fs::read(path).unwrap();
    ArtifactIdentity {
        artifact_id: ArtifactId::new("fixture").unwrap(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        byte_len: bytes.len() as u64,
    }
}
fn request() -> AssistantWorkerRequest {
    let artifact = ArtifactIdentity {
        artifact_id: ArtifactId::new("fixture").unwrap(),
        sha256: "a".repeat(64),
        byte_len: 1,
    };
    AssistantWorkerRequest {
        vision: None,
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new("assistant-fixture").unwrap(),
        runtime: artifact.clone(),
        model: artifact,
        system_context: "fixture context; not a trained model".into(),
        user_request: "No action".into(),
        scope: AssistantProposalScope::default(),
        cpu_threads: 1,
        wall_time_ms: 2000,
    }
}
#[test]
fn parent_requires_original_identity_and_its_own_context() {
    let r = request();
    let good = AssistantWorkerResponse {
        vision: None,
        version: r.version,
        attempt_id: r.attempt_id.clone(),
        runtime: r.runtime.clone(),
        model: r.model.clone(),
        proposal: AssistantProposal::NoAction {},
    };
    validate_response(&r, &good).unwrap();
    for field in ["version", "attempt", "runtime", "model", "reference"] {
        let mut bad = good.clone();
        match field {
            "version" => bad.version += 1,
            "attempt" => bad.attempt_id = AttemptId::new("other").unwrap(),
            "runtime" => bad.runtime.sha256 = "b".repeat(64),
            "model" => bad.model.byte_len += 1,
            _ => {
                bad.proposal = AssistantProposal::SelectTarget {
                    target_id: "invented".into(),
                }
            }
        }
        assert!(validate_response(&r, &bad).is_err(), "{field}");
    }
}
fn fake(root: &Path, body: &str) -> PathBuf {
    let path = root.join("worker");
    std::fs::write(
        &path,
        format!("#!/usr/bin/python3\nimport sys,json,time,os\nr=json.load(sys.stdin)\n{body}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn released(pool: &ResourcePool, root: &Path, maximum: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("worker-fence.json")).unwrap())
                .unwrap();
        if ledger["groups"].as_object().unwrap().is_empty()
            && ledger["retired"].as_object().unwrap().is_empty()
            && ledger["creation"]["records"]
                .as_object()
                .unwrap()
                .is_empty()
            && pool.reserve(maximum).is_ok()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "assistant custody or full admission not retired"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "requires actual delegated Linux cgroup; scripted transport faults, not learned quality"]
fn real_parent_owns_sealed_assets_and_retires_on_transport_faults() {
    assert!(
        crate::launch::available(),
        "delegated Linux confinement required"
    );
    for (case,body,success) in [
        ("good","assert open('/assistant/runtime','rb').read()==b'fixture runtime'\nassert open('/assistant/model.gguf','rb').read()==b'fixture model'\nprint(json.dumps(dict(version=r['version'],attempt_id=r['attempt_id'],runtime=r['runtime'],model=r['model'],proposal={'kind':'no_action'})))",true),
        ("stale","print(json.dumps(dict(version=r['version'],attempt_id='late',runtime=r['runtime'],model=r['model'],proposal={'kind':'no_action'})))",false),
        ("oversize","print('x'*8193)",false),
        ("deadline","time.sleep(30)",false),
        ("descendant","pid=os.fork()\nif pid==0:\n time.sleep(30)\n os._exit(0)\nprint(json.dumps(dict(version=r['version'],attempt_id=r['attempt_id'],runtime=r['runtime'],model=r['model'],proposal={'kind':'no_action'})))",true),
    ] {
        let root=tempfile::tempdir().unwrap();std::fs::set_permissions(root.path(),std::fs::Permissions::from_mode(0o700)).unwrap();
        let worker=fake(root.path(),body);let runtime=root.path().join("runtime");let model=root.path().join("model");
        std::fs::write(&runtime,b"fixture runtime").unwrap();std::fs::set_permissions(&runtime,std::fs::Permissions::from_mode(0o700)).unwrap();std::fs::write(&model,b"fixture model").unwrap();
        let mut r=request();r.runtime=identity(&runtime);r.model=identity(&model);
        let fence=Arc::new(WorkerFence::open(root.path()).unwrap());fence.enable_creation().unwrap();
        let maximum=768*1024*1024;let pool=ResourcePool::new(maximum,1);
        let config=EngineConfig::new(root.path().to_path_buf(),worker.clone());
        let paths=AssistantLaunchAssets{ vision: None,runtime_path:runtime,model_path:model,worker_memory_bytes:512*1024*1024};
        let result=execute(Parent{config:&config,pool:&pool,fence,worker_identity:&identity(&worker),admission:None},&r,&paths,&AtomicBool::new(false),Instant::now()+Duration::from_secs(2));
        assert_eq!(result.is_ok(),success,"{case}: {result:?}");
        if success {
            let ledger:serde_json::Value=serde_json::from_slice(&std::fs::read(root.path().join("worker-fence.json")).unwrap()).unwrap();
            assert!(ledger["groups"].as_object().unwrap().is_empty(),"{case}: returned before group retirement");
            assert!(ledger["creation"]["records"].as_object().unwrap().is_empty(),"{case}: retained creation custody");
            assert!(pool.reserve(maximum).is_ok(),"{case}: returned with outstanding admission");
        }
        released(&pool,root.path(),maximum);
    }
}
#[test]
#[ignore = "explicit pinned learned model and current worker; actual Engine custody, no effect qualification"]
fn actual_engine_parent_runs_pinned_learned_proposal() {
    let worker =
        PathBuf::from(std::env::var_os("PULSAR_TEST_ASSISTANT_WORKER").expect("current worker"));
    let runtime =
        PathBuf::from(std::env::var_os("PULSAR_ASSISTANT_RUNTIME").expect("pinned runtime"));
    let model = PathBuf::from(std::env::var_os("PULSAR_ASSISTANT_MODEL").expect("pinned model"));
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = EngineConfig::new(root.path().to_path_buf(), worker);
    let engine = crate::Engine::open(config).unwrap();
    let mut r = request();
    r.runtime = identity(&runtime);
    r.model = identity(&model);
    assert_eq!(
        r.runtime.sha256,
        "f332257812cf358954a1701c9fb205e5ac50b4fbdf0e2c09944cc25ea58459ae"
    );
    assert_eq!(
        r.model.sha256,
        "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031"
    );
    r.wall_time_ms = 45_000;
    r.cpu_threads = 4;
    r.user_request = "Track the second cup.".into();
    r.system_context="Return one supported JSON proposal. Targets cup1=first cup, cup2=second cup. Use {\"kind\":\"select_target\",\"target_id\":\"ID\"} or {\"kind\":\"no_action\"}.".into();
    r.scope.target_ids = vec!["cup1".into(), "cup2".into()];
    let paths = AssistantLaunchAssets {
        vision: None,
        runtime_path: runtime,
        model_path: model,
        worker_memory_bytes: 2 * 1024 * 1024 * 1024,
    };
    let output = engine
        .propose_assistant(&r, &paths, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        output.response.proposal,
        AssistantProposal::SelectTarget {
            target_id: "cup2".into()
        }
    );
    assert!(
        output.worker_peak_memory_bytes > 0
            && output.worker_peak_memory_bytes <= paths.worker_memory_bytes
    );
    println!(
        "retired assistant worker peak {} bytes",
        output.worker_peak_memory_bytes
    );
    let ledger: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("worker-fence.json")).unwrap())
            .unwrap();
    assert!(ledger["groups"].as_object().unwrap().is_empty());
    assert!(ledger["retired"].as_object().unwrap().is_empty());
    assert!(ledger["creation"]["records"]
        .as_object()
        .unwrap()
        .is_empty());
}

#[test]
fn escaped_context_refuses_before_asset_reads_or_admission() {
    let mut r = request();
    r.system_context = "\u{1}".repeat(8192);
    assert!(r.validate().is_ok());
    assert_eq!(
        validate_request(&r).unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
}

#[test]
#[ignore = "requires actual delegated Linux cgroup and live child observation"]
fn cancellation_signals_only_after_owned_tree_is_observed_and_retires_admission() {
    assert!(crate::launch::available(), "delegated confinement required");
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let worker = fake(root.path(), "time.sleep(30)");
    let runtime = root.path().join("runtime");
    let model = root.path().join("model");
    std::fs::write(&runtime, b"runtime").unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&model, b"model").unwrap();
    let mut r = request();
    r.runtime = identity(&runtime);
    r.model = identity(&model);
    r.wall_time_ms = 15000;
    let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
    fence.enable_creation().unwrap();
    let maximum = 768 * 1024 * 1024;
    let pool = ResourcePool::new(maximum, 1);
    let config = EngineConfig::new(root.path().to_path_buf(), worker.clone());
    let paths = AssistantLaunchAssets {
        vision: None,
        runtime_path: runtime,
        model_path: model,
        worker_memory_bytes: 512 * 1024 * 1024,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let observed_pool = pool.clone();
    let ledger = root.path().join("worker-fence.json");
    let observer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let bytes = std::fs::read(&ledger).unwrap();
            let state: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            for group in state["groups"].as_object().unwrap().values() {
                let path = Path::new(group["path"].as_str().unwrap());
                if std::fs::read_to_string(path.join("cgroup.procs"))
                    .is_ok_and(|pids| !pids.trim().is_empty())
                {
                    assert!(
                        observed_pool.reserve(maximum).is_err(),
                        "live tree released aggregate admission"
                    );
                    signal.store(true, Ordering::Release);
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    });
    let result = execute(
        Parent {
            config: &config,
            pool: &pool,
            fence,
            worker_identity: &identity(&worker),
            admission: None,
        },
        &r,
        &paths,
        &cancel,
        Instant::now() + Duration::from_secs(15),
    );
    assert!(observer.join().unwrap(), "no live owned child was observed");
    assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled);
    released(&pool, root.path(), maximum);
}

fn vision_request() -> AssistantWorkerRequest {
    let mut r = request();
    r.vision = Some(AssistantVisionInput {
        projector: r.model.clone(),
        rgb: ArtifactIdentity {
            byte_len: 3,
            ..r.model.clone()
        },
        width: 1,
        height: 1,
    });
    r.scope.creative_patterns = Some(CreativePatternScope {
        axes: vec![Axis::Stroke],
        duration: ProjectTime::from_nanos(2_000_000_000),
        sample_hz: 8,
        max_amplitude: 0.4,
    });
    r
}
#[test]
fn parent_rejects_absent_or_changed_vision_bindings() {
    let r = vision_request();
    r.validate().unwrap();
    let good = AssistantWorkerResponse {
        vision: r.vision.clone(),
        version: r.version,
        attempt_id: r.attempt_id.clone(),
        runtime: r.runtime.clone(),
        model: r.model.clone(),
        proposal: AssistantProposal::NoAction {},
    };
    validate_response(&r, &good).unwrap();
    for case in 0..5 {
        let mut bad = good.clone();
        match case {
            0 => bad.vision = None,
            1 => bad.vision.as_mut().unwrap().projector.sha256 = "b".repeat(64),
            2 => bad.vision.as_mut().unwrap().rgb.sha256 = "b".repeat(64),
            3 => bad.vision.as_mut().unwrap().width += 1,
            _ => bad.vision.as_mut().unwrap().rgb.artifact_id = ArtifactId::new("other").unwrap(),
        }
        assert!(validate_response(&r, &bad).is_err(), "{case}");
    }
    assert!(validate_response(&request(), &good).is_err());
}
#[test]
fn vision_admission_counts_both_copied_assets_and_rejects_unpaired_paths() {
    let root = tempfile::tempdir().unwrap();
    let worker = root.path().join("worker");
    let runtime = root.path().join("runtime");
    let model = root.path().join("model");
    let projector = root.path().join("projector");
    let rgb = root.path().join("rgb");
    for p in [&worker, &runtime, &model, &projector] {
        std::fs::write(p, b"asset").unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(&rgb, [1, 2, 3]).unwrap();
    let config = EngineConfig::new(root.path().to_path_buf(), worker.clone());
    let mut r = vision_request();
    r.runtime = identity(&runtime);
    r.model = identity(&model);
    r.vision = Some(AssistantVisionInput {
        projector: identity(&projector),
        rgb: identity(&rgb),
        width: 1,
        height: 1,
    });
    let mut paths = AssistantLaunchAssets {
        vision: Some(AssistantVisionAssets {
            projector_path: projector.clone(),
            rgb_path: rgb.clone(),
        }),
        runtime_path: runtime,
        model_path: model,
        worker_memory_bytes: 512 << 20,
    };
    let claim = admission_bytes(&config, &identity(&worker), &r, &paths).unwrap();
    assert_eq!(claim, (512u64 << 20) + (2 << 20) + 4 * 5 + 3);
    paths.vision = None;
    assert!(admission_bytes(&config, &identity(&worker), &r, &paths).is_err());
    paths.vision = Some(AssistantVisionAssets {
        projector_path: projector,
        rgb_path: rgb,
    });
    r.vision = None;
    assert!(admission_bytes(&config, &identity(&worker), &r, &paths).is_err());
}

#[test]
#[ignore = "pinned vision assets/current worker and actual delegated cgroups; authored controls only; serial"]
fn actual_engine_parent_runs_bound_vision_creative_proposals() {
    let asset = |name| {
        PathBuf::from(std::env::var_os(name).expect("explicit pinned vision asset required"))
    };
    let worker = asset("PULSAR_TEST_ASSISTANT_WORKER");
    let runtime = asset("PULSAR_VISION_RUNTIME");
    let model = asset("PULSAR_VISION_MODEL");
    let projector = asset("PULSAR_VISION_PROJECTOR");
    assert_eq!(
        identity(&runtime).sha256,
        "c38a6afc4790b64e2cfc86e4de5a08f818793fa1cea44e3f9eb5df9f82a90601"
    );
    assert_eq!(
        identity(&model).sha256,
        "2a31195d3769c0b0fd0a4906201666108834848db768af11de1d2cef7cd35e65"
    );
    assert_eq!(
        identity(&projector).sha256,
        "0802360aca1748f112ea510b8ff277c65b1361c8ef30ed89b83c9c7a60d08e96"
    );
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = EngineConfig::new(root.path().to_path_buf(), worker);
    let engine = crate::Engine::open(config).unwrap();
    for shape in ["circle", "square"] {
        let rgb = root.path().join(format!("{shape}.rgb"));
        let mut pixels = Vec::with_capacity(384 * 384 * 3);
        for y in 0i32..384 {
            for x in 0i32..384 {
                let colored = if shape == "circle" {
                    (x - 192).pow(2) + (y - 192).pow(2) <= 112 * 112
                } else {
                    (80..=304).contains(&x) && (80..=304).contains(&y)
                };
                pixels.extend_from_slice(if colored {
                    &[255, 0, 0]
                } else {
                    &[255, 255, 255]
                });
            }
        }
        std::fs::write(&rgb, pixels).unwrap();
        let mut r = vision_request();
        r.attempt_id = AttemptId::new(format!("vision-{shape}")).unwrap();
        r.runtime = identity(&runtime);
        r.model = identity(&model);
        r.vision = Some(AssistantVisionInput {
            projector: identity(&projector),
            rgb: identity(&rgb),
            width: 384,
            height: 384,
        });
        r.cpu_threads = 2;
        r.wall_time_ms = 120000;
        r.system_context = "Return one Pulsar JSON creative proposal. This still image is a visual theme, not observed temporal motion. Use synthesize_patterns with exactly one stroke pattern, duration 2000000000 nanoseconds, sample_hz 8, amplitude 0.4, offset 0.5, frequency_hz 1, phase_cycles 0, duty_cycle 0.5. Use no_action for unsupported requests. Never actuate hardware.".into();
        r.user_request =
            "Use sine for a circle or triangle for a square, based on the pixels.".into();
        let paths = AssistantLaunchAssets {
            vision: Some(AssistantVisionAssets {
                projector_path: projector.clone(),
                rgb_path: rgb,
            }),
            runtime_path: runtime.clone(),
            model_path: model.clone(),
            worker_memory_bytes: 2u64 << 30,
        };
        let output = engine
            .propose_assistant(&r, &paths, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(output.response.vision, r.vision);
        AssistantProposal::decode(
            &serde_json::to_vec(&output.response.proposal).unwrap(),
            &r.scope,
        )
        .unwrap();
        assert!(
            output.worker_peak_memory_bytes > 0
                && output.worker_peak_memory_bytes <= paths.worker_memory_bytes
        );
        println!(
            "VISION_RESULT {}",
            serde_json::json!({"case":shape,"response":output.response,"peak_memory_bytes":output.worker_peak_memory_bytes})
        );
        let ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.path().join("worker-fence.json")).unwrap())
                .unwrap();
        assert!(ledger["groups"].as_object().unwrap().is_empty());
        assert!(ledger["retired"].as_object().unwrap().is_empty());
        assert!(ledger["creation"]["records"]
            .as_object()
            .unwrap()
            .is_empty());
    }
}

#[test]
#[ignore = "actual delegated Linux cgroups; scripted vision mount/transport faults and cancellation, not learned quality"]
fn real_vision_parent_retains_pair_and_admission_through_faults_and_cancellation() {
    assert!(crate::launch::available(), "delegated confinement required");
    let good = "print(json.dumps(dict(version=r['version'],attempt_id=r['attempt_id'],runtime=r['runtime'],model=r['model'],vision=r['vision'],proposal={'kind':'no_action'})))";
    for (case, body, success) in [
        ("good", good, true),
        ("stale-projector", "r['vision']['projector']['sha256']='b'*64\n" , false),
        ("stale-rgb", "r['vision']['rgb']['byte_len']+=1\n", false),
        ("missing-pair", "r.pop('vision')\nprint(json.dumps(dict(version=r['version'],attempt_id=r['attempt_id'],runtime=r['runtime'],model=r['model'],proposal={'kind':'no_action'})))", false),
        ("output-ceiling", "print('x'*8193)", false),
        ("deadline", "time.sleep(30)", false),
        ("cancel", "time.sleep(30)", false),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut body = format!("assert open('/assistant/mmproj.gguf','rb').read()==b'projector'\nassert open('/assistant/image.rgb','rb').read()==bytes([255,0,0])\n{body}");
        if matches!(case, "stale-projector" | "stale-rgb") { body.push_str(good); }
        let worker = fake(root.path(), &body);
        let runtime = root.path().join("runtime"); let model = root.path().join("model"); let projector = root.path().join("projector"); let rgb = root.path().join("rgb");
        std::fs::write(&runtime, b"runtime").unwrap(); std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&model, b"model").unwrap(); std::fs::write(&projector, b"projector").unwrap(); std::fs::write(&rgb, [255,0,0]).unwrap();
        let mut r = vision_request(); r.runtime = identity(&runtime); r.model = identity(&model);
        r.vision = Some(AssistantVisionInput { projector: identity(&projector), rgb: identity(&rgb), width: 1, height: 1 });
        r.wall_time_ms = if case == "deadline" { 250 } else { 5000 };
        let paths = AssistantLaunchAssets { vision: Some(AssistantVisionAssets { projector_path: projector, rgb_path: rgb }), runtime_path: runtime, model_path: model, worker_memory_bytes: 512u64 << 20 };
        let config = EngineConfig::new(root.path().to_path_buf(), worker.clone());
        let maximum = 768u64 << 20; let pool = ResourcePool::new(maximum, 1);
        let fence = Arc::new(WorkerFence::open(root.path()).unwrap()); fence.enable_creation().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let observer = if case == "cancel" {
            let signal = cancel.clone(); let observed_pool = pool.clone(); let ledger = root.path().join("worker-fence.json");
            Some(std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(4);
                while Instant::now() < deadline {
                    let state: serde_json::Value = serde_json::from_slice(&std::fs::read(&ledger).unwrap()).unwrap();
                    for group in state["groups"].as_object().unwrap().values() {
                        let path = Path::new(group["path"].as_str().unwrap());
                        if std::fs::read_to_string(path.join("cgroup.procs")).is_ok_and(|pids| !pids.trim().is_empty()) {
                            assert!(observed_pool.reserve(maximum).is_err());
                            assert_eq!(observed_pool.reserved_cpu_threads().unwrap(), 1);
                            signal.store(true, Ordering::Release); return true;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                false
            }))
        } else { None };
        let result = execute(Parent { config: &config, pool: &pool, fence, worker_identity: &identity(&worker), admission: None }, &r, &paths, &cancel, Instant::now()+Duration::from_millis(r.wall_time_ms));
        if let Some(observer) = observer { assert!(observer.join().unwrap(), "live tree was not observed"); }
        assert_eq!(result.is_ok(), success, "{case}: {result:?}");
        if case == "cancel" { assert_eq!(result.unwrap_err().code, ErrorCode::Cancelled); }
        released(&pool, root.path(), maximum); assert_eq!(pool.reserved_cpu_threads().unwrap(), 0);
        println!("VISION_FAULT_RESULT {case}: success={success}; empty custody and zero CPU");
    }
}
