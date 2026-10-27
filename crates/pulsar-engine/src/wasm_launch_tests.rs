use super::*;
use pulsar_core::{ArtifactId, AttemptId};

fn execute(
    config: &EngineConfig,
    pool: &ResourcePool,
    fence: Arc<WorkerFence>,
    envelope: &WasmWorkerRequest,
    cancel: &AtomicBool,
) -> Result<Execution, ProtocolError> {
    let bytes = std::fs::read(&config.worker_executable).unwrap();
    let expected = ArtifactIdentity {
        artifact_id: ArtifactId::new("authored-worker").unwrap(),
        sha256: digest(&bytes),
        byte_len: bytes.len() as u64,
    };
    super::execute(config, pool, fence, envelope, &expected, cancel)
}

fn envelope() -> WasmWorkerRequest {
    let module = wat::parse_str(
        r#"(module (memory (export "memory") 1)
        (func (export "pulsar_alloc") (param i32) (result i32) i32.const 16)
        (func (export "pulsar_process") (param i32 i32) (result i64)
            local.get 0 local.get 0 i32.load8_u i32.const 255 i32.xor i32.store8
            local.get 0 i64.extend_i32_u i64.const 32 i64.shl
            local.get 1 i64.extend_i32_u i64.or))"#,
    )
    .unwrap();
    WasmWorkerRequest {
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new("wasm-fixture").unwrap(),
        request: WasmProcessingRequest {
            manifest: ExtensionManifest {
                schema_version: PROTOCOL_VERSION,
                name: "fixture".into(),
                role: ExtensionRole::Processor,
                artifact: ArtifactIdentity {
                    artifact_id: ArtifactId::new("module").unwrap(),
                    sha256: digest(&module),
                    byte_len: module.len() as u64,
                },
                abi: WASM_PROCESSING_ABI.into(),
                requested_scopes: vec![],
                budget: ResourceBudget {
                    memory_bytes: 256 * 1024 * 1024,
                    output_bytes: WASM_BUFFER_MAX_BYTES as u64,
                    wall_time_ms: 5000,
                    cpu_threads: 2,
                },
                confinement: ConfinementRequirement::Wasm,
            },
            module,
            input: vec![1, 2, 3],
            guest_memory_bytes: 65536,
        },
    }
}
fn response(r: &WasmWorkerRequest) -> WasmWorkerResponse {
    WasmWorkerResponse {
        version: PROTOCOL_VERSION,
        attempt_id: r.attempt_id.clone(),
        output: WasmProcessingResult {
            abi: WASM_PROCESSING_ABI.into(),
            module_sha256: r.request.manifest.artifact.sha256.clone(),
            input_sha256: digest(&r.request.input),
            output_sha256: digest(&[254, 2, 3]),
            output: vec![254, 2, 3],
            fuel_consumed: 10,
        },
    }
}
#[test]
fn response_requires_exact_attempt_and_buffer_identities() {
    let r = envelope();
    let good = response(&r);
    validate_response(&r, &good).unwrap();
    for field in [
        "version", "attempt", "abi", "module", "input", "output", "fuel", "size",
    ] {
        let mut bad = good.clone();
        match field {
            "version" => bad.version += 1,
            "attempt" => bad.attempt_id = AttemptId::new("stale").unwrap(),
            "abi" => bad.output.abi.push('x'),
            "module" => bad.output.module_sha256 = "b".repeat(64),
            "input" => bad.output.input_sha256 = "b".repeat(64),
            "output" => bad.output.output[0] ^= 1,
            "fuel" => bad.output.fuel_consumed = WASM_FUEL_LIMIT + 1,
            _ => {
                bad.output.output.resize(WASM_BUFFER_MAX_BYTES + 1, 0);
                bad.output.output_sha256 = digest(&bad.output.output);
            }
        }
        assert!(validate_response(&r, &bad).is_err(), "{field}");
    }
}

fn fake(root: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(name);
    std::fs::write(
        &path,
        format!("#!/usr/bin/python3\nimport sys,json,hashlib,time,os\n{body}\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn released(pool: &ResourcePool, root: &std::path::Path) {
    let end = Instant::now() + Duration::from_secs(5);
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
            && (ledger["staging"].is_null() || ledger["staging"].as_object().unwrap().is_empty())
            && pool.reserve(1).is_ok()
        {
            break;
        }
        assert!(
            Instant::now() < end,
            "WASM custody/reservation was not retired"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires an actual current WASM worker and user-delegated Linux cgroup"]
fn actual_confined_worker_and_faults_retire_before_releasing_resources() {
    use std::os::unix::fs::PermissionsExt;
    let binary = std::fs::canonicalize(
        std::env::var_os("PULSAR_TEST_WASM_WORKER").expect("current worker fixture"),
    )
    .unwrap();
    assert!(crate::launch::available(), "Linux confinement unavailable");
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let fence = Arc::new(WorkerFence::open(root.path()).unwrap());
    fence.enable_creation().unwrap();
    let pool = ResourcePool::new(512 * 1024 * 1024, 1);
    let mut config = EngineConfig::new(root.path().to_path_buf(), binary);
    let r = envelope();
    let cancel = Arc::new(AtomicBool::new(false));
    let value = execute(&config, &pool, fence.clone(), &r, &cancel).unwrap();
    assert_eq!(value.output.output, vec![254, 2, 3]);
    assert_eq!(
        value.worker_executable_sha256,
        digest(&std::fs::read(&config.worker_executable).unwrap())
    );
    assert_eq!(
        value.worker_executable_bytes,
        std::fs::metadata(&config.worker_executable).unwrap().len()
    );
    assert!(value.worker_peak_memory_bytes > 0);
    assert!(value.worker_peak_memory_bytes <= r.request.manifest.budget.memory_bytes);
    released(&pool, root.path());
    println!(
        "actual confined WASM transform and measured cgroup peak: {}",
        value.worker_peak_memory_bytes
    );

    // A real pipe WouldBlock proves cancellation occurs while writing, rather
    // than before spawning or merely because the timer happened to fire.
    config.worker_executable = fake(root.path(), "blocked-input", "time.sleep(60)");
    let mut large = r.clone();
    large.request.input.resize(WASM_BUFFER_MAX_BYTES, 0);
    let blocked = Arc::new(AtomicBool::new(false));
    let observed = blocked.clone();
    let token = cancel.clone();
    let held = pool.clone();
    crate::launch_pipes::blocked_write_hook_for_test(Box::new(move || {
        assert!(
            held.reserve(1).is_err(),
            "reservation released while worker pipe remained open"
        );
        observed.store(true, Ordering::Release);
        token.store(true, Ordering::Release);
    }));
    assert_eq!(
        execute(&config, &pool, fence.clone(), &large, &cancel)
            .err()
            .unwrap()
            .code,
        ErrorCode::Cancelled
    );
    assert!(blocked.load(Ordering::Acquire));
    released(&pool, root.path());
    cancel.store(false, Ordering::Release);
    println!("blocked stdin cancellation and held admission: PASS");

    let good = serde_json::to_string(&response(&r)).unwrap();
    config.worker_executable = fake(root.path(), "namespace-exit",
        &format!("sys.stdin.read()\nif os.fork()==0: time.sleep(60); os._exit(0)\nprint({good:?},flush=True)"));
    assert_eq!(
        execute(&config, &pool, fence.clone(), &r, &cancel)
            .unwrap()
            .output
            .output,
        vec![254, 2, 3]
    );
    released(&pool, root.path());
    println!("namespace exit retires descendant before result admission: PASS");
    let cases=[
        ("stale-attempt",format!("sys.stdin.read()\nv=json.loads({good:?})\nv['attempt_id']='stale'\nprint(json.dumps(v),flush=True)")),
        ("corrupt-output",format!("sys.stdin.read()\nv=json.loads({good:?})\nv['output']['output'][0]=0\nprint(json.dumps(v),flush=True)")),
        ("stdout-flood",format!("sys.stdin.read()\nsys.stdout.write('x'*{})\nsys.stdout.flush()",WASM_RESPONSE_MAX_BYTES+1)),
        ("nonzero-exit",format!("sys.stdin.read()\nprint({good:?},flush=True)\nsys.exit(1)")),
        ("descendant-held-pipe",format!("sys.stdin.read()\nif os.fork()==0: time.sleep(60); os._exit(0)\nprint({good:?},flush=True)\nos.close(1)\ntime.sleep(60)")),
        ("stderr-flood", "sys.stdin.read()\nwhile True: os.write(2,b'x'*8192)".into()),
    ];
    for (name, body) in cases {
        config.worker_executable = fake(root.path(), name, &body);
        let failure = execute(&config, &pool, fence.clone(), &r, &cancel)
            .err()
            .expect(name);
        println!("{name}: {:?}: {}", failure.code, failure.message);
        released(&pool, root.path());
    }
}

#[test]
fn native_image_size_is_bounded_before_publication_and_capture() {
    assert!(!executable_size_admitted(0));
    assert!(executable_size_admitted(128 * 1024 * 1024));
    assert!(!executable_size_admitted(128 * 1024 * 1024 + 1));
}
