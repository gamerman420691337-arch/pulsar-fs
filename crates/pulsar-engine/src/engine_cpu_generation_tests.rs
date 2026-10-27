// Included inside authority::tests on Linux x86_64. These fixtures declare
// expected images only; no captured handle, native execution or qualification
// is fabricated, and no helper invokes capture_cpu_inference.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod cpu_generation_admission {
    use super::*;
    use crate::cpu_owner_policy::{CpuOwnerPolicy, LoadedCpuOwnerPolicy};
    use std::{
        ffi::CString,
        io::Read,
        os::unix::{ffi::OsStrExt, fs::PermissionsExt, io::FromRawFd},
    };

    struct CpuFixture {
        f: Fixture,
        owner: LoadedCpuOwnerPolicy,
        policy_path: PathBuf,
    }
    fn fixture_cpu(configured: bool) -> CpuFixture {
        let dir = tempfile::tempdir().unwrap();
        let policy = CpuOwnerPolicy {
            schema_version: 1,
            native_dependencies: NativeDependencySpec {
                schema_version: 1,
                libraries: CPU_RUNTIME_REQUIRED_ALIASES
                    .iter()
                    .enumerate()
                    .map(|(index, alias)| {
                        // Intent fixture bytes are deliberately not executable libraries.
                        let bytes = format!("authored nonexecuted library declaration {index}");
                        let path = dir.path().join(format!("library-{index}"));
                        fs::write(&path, &bytes).unwrap();
                        NativeLibrarySpec {
                            path,
                            sha256: format!("{:x}", Sha256::digest(bytes.as_bytes())),
                            byte_len: bytes.len() as u64,
                            names: vec![(*alias).into()],
                        }
                    })
                    .collect(),
            },
        };
        policy.validate().unwrap();
        let policy_path = dir.path().join("cpu-owner.json");
        fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
        fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600)).unwrap();
        let owner = LoadedCpuOwnerPolicy::load(&policy_path).unwrap();
        let worker = dir.path().join("nonexecuted-worker-fixture");
        fs::write(
            &worker,
            b"CPU admission test worker identity; never execute",
        )
        .unwrap();
        let mut config = EngineConfig::new(dir.path().join("state"), worker);
        config.cpu_owner_policy = configured.then(|| policy_path.clone());
        let engine = Engine::open(config).unwrap();
        let session = pair(&engine, "CPU admission fixture");
        let ResponseBody::Project(project) = engine
            .handle(request(
                &session,
                None,
                None,
                Command::CreateProject {
                    name: "CPU admission".into(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("project response")
        };
        CpuFixture {
            f: Fixture {
                engine,
                session,
                project: project.project_id,
                dir,
            },
            owner,
            policy_path,
        }
    }

    fn explicit_profile(
        model: ArtifactIdentity,
        owner: &LoadedCpuOwnerPolicy,
    ) -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model,
            runtime: owner.runtime_identity().unwrap(),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        }
    }

    fn retained_attempt(
        x: &CpuFixture,
        closed_owner: Option<&LoadedCpuOwnerPolicy>,
        actual_tools: bool,
    ) -> WorkerRequest {
        let (mut manifest, _) = staged_manifest(&x.f);
        let WorkerOperation::Generate {
            settings, model, ..
        } = &mut manifest.operation
        else {
            unreachable!()
        };
        let selected = explicit_profile(
            model.as_ref().unwrap().identity.clone(),
            closed_owner.unwrap_or(&x.owner),
        );
        settings.model_input = Some(selected.preprocessing.input.clone());
        settings.inference_profile = Some(selected.clone());
        manifest.budget = ResourceBudget {
            memory_bytes: 128 << 20,
            output_bytes: 16 << 20,
            wall_time_ms: 30_000,
            cpu_threads: 2,
        };
        if actual_tools {
            // Discovery pins existing files; no ffmpeg/ffprobe process executes.
            manifest.tools =
                x.f.engine
                    .tools
                    .as_ref()
                    .expect("recovery guard fixture requires installed ffmpeg/ffprobe identities")
                    .clone();
        }
        manifest
            .dependencies
            .push(x.f.engine.binary_identity.clone());
        let image = closed_owner.map(|owner| owner.image().unwrap());
        if let Some(image) = &image {
            manifest.tools.onnx_runtime = Some(ArtifactReference {
                identity: selected.runtime,
                path: Path::new(crate::cpu_mounts::CPU_LIBRARY_DIRECTORY)
                    .join("libonnxruntime.so.1"),
            });
            for identity in std::iter::once(image.identity().unwrap())
                .chain(std::iter::once(image.owner_policy.clone()))
                .chain(
                    image
                        .libraries
                        .iter()
                        .map(|library| library.identity.clone()),
                )
            {
                if !manifest.dependencies.contains(&identity) {
                    manifest.dependencies.push(identity);
                }
            }
        }
        manifest.validate().unwrap();
        let raw = encode(&manifest).unwrap();
        fs::write(manifest.output_dir.join("initial-checkpoint.json"), &raw).unwrap();
        let mut db = x.f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        tx.execute(
            "UPDATE jobs SET manifest=?1,state='interrupted' WHERE id=?2",
            params![raw, manifest.job_id.as_str()],
        )
        .unwrap();
        tx.execute(
            "UPDATE attempts SET manifest=?1,state='interrupted' WHERE id=?2",
            params![raw, manifest.attempt_id.as_str()],
        )
        .unwrap();
        if let Some(image) = image {
            // This is a durable EXPECTED image declaration, not a captured image.
            crate::cpu_runtime_db::declare(&tx, &manifest, &image).unwrap();
        }
        tx.commit().unwrap();
        manifest
    }

    fn history(x: &CpuFixture, manifest: &WorkerRequest) -> (String, String, i64, i64, String) {
        let db = x.f.engine.db.lock().unwrap();
        db.query_row("SELECT j.state,a.state,j.attempts,(SELECT count(*) FROM attempts WHERE job=j.id),j.manifest FROM jobs j JOIN attempts a ON a.id=j.attempt WHERE j.id=?1", [manifest.job_id.as_str()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap()
    }
    fn assert_no_workers(x: &CpuFixture) {
        assert!(x.f.engine.workers.lock().unwrap().is_empty());
        assert!(!x
            .f
            .engine
            .worker_fence
            .has_pending_worker_custody()
            .unwrap());
        assert_eq!(x.f.engine.pool.reserved_storage(), 0);
    }
    fn resume(x: &CpuFixture, manifest: &WorkerRequest) -> Request {
        request(
            &x.f.session,
            Some(&x.f.project),
            None,
            Command::ResumeJob {
                job_id: manifest.job_id.clone(),
            },
        )
    }

    #[test]
    fn configured_cpu_policy_refuses_implicit_model_profile_through_public_generate() {
        let x = fixture_cpu(true);
        let (manifest, _) = staged_manifest(&x.f);
        let WorkerOperation::Generate {
            preset,
            settings,
            model,
        } = &manifest.operation
        else {
            unreachable!()
        };
        assert!(settings.inference_profile.is_none());
        let before = history(&x, &manifest);
        let command = Command::Generate {
            source_version: manifest.source.source_version.clone(),
            preset: preset.clone(),
            settings: Some(*settings.clone()),
            model_path: Some(model.as_ref().unwrap().path.clone()),
        };
        let error =
            x.f.engine
                .handle(request(&x.f.session, Some(&x.f.project), Some(0), command))
                .result
                .expect_err("implicit profile must refuse");
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        assert!(error.message.contains("explicit inference profile"));
        assert_eq!(history(&x, &manifest), before);
        assert_no_workers(&x);
    }

    #[test]
    fn configured_explicit_cpu_generation_cannot_bypass_fresh_capture_at_publication() {
        let x = fixture_cpu(true);
        let manifest = retained_attempt(&x, None, false);
        let before = history(&x, &manifest);
        let request = resume(&x, &manifest);
        let mut db = x.f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        let error =
            x.f.engine
                .prepare_operation(
                    &tx,
                    &request,
                    &manifest.source.source_version,
                    manifest.operation.clone(),
                    Some(&manifest),
                )
                .err()
                .expect("missing captured CPU handle must refuse");
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.message.contains("freshly captured image"));
        tx.rollback().unwrap();
        drop(db);
        assert_eq!(history(&x, &manifest), before);
        assert_no_workers(&x);
    }

    #[test]
    fn ordinary_interrupted_attempt_cannot_change_to_configured_closed_cpu_mode() {
        let x = fixture_cpu(true);
        let manifest = retained_attempt(&x, None, false);
        let before = history(&x, &manifest);
        let error =
            x.f.engine
                .handle(resume(&x, &manifest))
                .result
                .expect_err("ordinary attempt cannot change runtime mode");
        assert_eq!(error.code, ErrorCode::DependencyMismatch);
        assert!(error.message.contains("runtime mode changed"));
        assert_eq!(history(&x, &manifest), before);
        assert_no_workers(&x);
    }

    #[test]
    fn retained_closed_attempt_requires_configured_exact_owner_policy() {
        for configured in [false, true] {
            let x = fixture_cpu(configured);
            // Same declared runtime bytes, different exact policy document.
            let mut bytes = fs::read(&x.policy_path).unwrap();
            bytes.push(b'\n');
            let original_path = x.f.dir.path().join("original-owner-policy.json");
            fs::write(&original_path, bytes).unwrap();
            fs::set_permissions(&original_path, fs::Permissions::from_mode(0o600)).unwrap();
            let original = LoadedCpuOwnerPolicy::load(&original_path).unwrap();
            assert_eq!(
                original.runtime_identity().unwrap(),
                x.owner.runtime_identity().unwrap()
            );
            assert_ne!(original.identity(), x.owner.identity());
            let manifest = retained_attempt(&x, Some(&original), false);
            let before = history(&x, &manifest);
            let error =
                x.f.engine
                    .handle(resume(&x, &manifest))
                    .result
                    .expect_err("closed attempt owner must match");
            if configured {
                assert_eq!(error.code, ErrorCode::DependencyMismatch);
                assert!(error.message.contains("exact original runtime policy"));
            } else {
                assert_eq!(error.code, ErrorCode::Unsupported);
                assert!(error.message.contains("owner-selected runtime policy"));
            }
            assert_eq!(history(&x, &manifest), before);
            assert_no_workers(&x);
        }
    }

    #[test]
    fn automatic_recovery_skips_closed_attempt_before_checkpoint_or_new_attempt() {
        let x = fixture_cpu(true);
        let manifest = retained_attempt(&x, Some(&x.owner), true);
        // Establish that unrelated dependency/ownership/checkpoint failures
        // cannot account for a skipped recovery attempt.
        x.f.engine.validate_dependencies(&manifest).unwrap();
        x.f.engine.validate_checkpoint(&manifest).unwrap();
        require_grant(
            &x.f.engine.db.lock().unwrap(),
            &x.f.session,
            &x.f.project,
            Scope::Generate,
        )
        .unwrap();
        let before = history(&x, &manifest);
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        assert!(fd >= 0, "inotify fixture initialization failed");
        let mut observer = unsafe { fs::File::from_raw_fd(fd) };
        let path = CString::new(
            manifest
                .output_dir
                .join("initial-checkpoint.json")
                .as_os_str()
                .as_bytes(),
        )
        .unwrap();
        assert!(
            unsafe { libc::inotify_add_watch(fd, path.as_ptr(), libc::IN_OPEN) } >= 0,
            "checkpoint watch failed"
        );
        let mut events = [0u8; 4096];
        // Positive control: the same production validator must produce an open.
        x.f.engine.validate_checkpoint(&manifest).unwrap();
        assert!(observer.read(&mut events).unwrap() > 0);
        x.f.engine.recover_interrupted();
        let error = observer
            .read(&mut events)
            .expect_err("closed recovery unexpectedly accessed its checkpoint");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(history(&x, &manifest), before);
        assert_no_workers(&x);
    }
}
