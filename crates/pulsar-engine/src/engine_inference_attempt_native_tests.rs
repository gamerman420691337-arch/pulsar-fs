// Actual confined CPU workers and public Engine history; no fixture acceptance.
#[cfg(target_os = "linux")]
mod inference_attempt_native_tests {
    use super::*;
    use anyhow::{ensure, Context};
    use serde_json::{json, Value};
    use std::{
        io::{Read, Write},
        os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        panic::{catch_unwind, AssertUnwindSafe},
        time::{Duration, Instant},
    };

    const MEDIA: &str = "/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-23/hsp-continuity/rendered-fixture/sterile-test-pattern-40s.mp4";
    const MODEL: &str = "/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-23/hsp-live-source/native-cpu-fixture/authored-temporal.onnx";
    const MEDIA_SHA: &str = "cfaafb105b7db1b5406ddfcaa913d9d635c990ec3f4dacd6818dee0b3c4eef12";
    const MODEL_SHA: &str = "9a10ed332a69f241c880c0ff8b08f8d234371e6702a59f868510fef7550078c8";
    const ORT_SHA: &str = "b5c9d4f124d24707f514dad926dc181820807178855df1c528e3addb2dd0e6f7";
    const JSON_LIMIT: u64 = 2 * 1024 * 1024;

    fn env_path(name: &str) -> Result<PathBuf> {
        let path =
            PathBuf::from(std::env::var_os(name).with_context(|| format!("{name} required"))?);
        ensure!(path.is_absolute(), "{name} must be absolute");
        Ok(path)
    }

    fn save(output: &Path, name: &str, value: &impl serde::Serialize) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        ensure!(
            (bytes.len() as u64) < JSON_LIMIT,
            "receipt exceeds bounded export: {name}"
        );
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(output.join(name))?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    }

    fn read_json(path: &Path) -> Result<Value> {
        let file = artifacts::open_regular(path)?;
        ensure!(
            file.metadata()?.len() <= JSON_LIMIT,
            "JSON receipt exceeds bound"
        );
        let mut bytes = Vec::new();
        file.take(JSON_LIMIT + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= JSON_LIMIT,
            "JSON receipt grew beyond bound"
        );
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn identity(path: &Path, expected: Option<&str>) -> Result<ArtifactIdentity> {
        let (sha256, byte_len) = artifacts::digest_file(path, 512 * 1024 * 1024)?;
        ensure!(
            byte_len > 0 && expected.is_none_or(|value| value == sha256),
            "pinned input differs: {}",
            path.display()
        );
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone())?,
            sha256,
            byte_len,
        })
    }

    fn call(
        engine: &Arc<Engine>,
        session: &SessionId,
        project: Option<&ProjectId>,
        revision: Option<u64>,
        command: Command,
    ) -> Result<ResponseBody> {
        engine
            .handle(request(session, project, revision, command))
            .result
            .map_err(anyhow::Error::from)
    }

    fn snapshot(
        engine: &Arc<Engine>,
        session: &SessionId,
        project: &ProjectId,
    ) -> Result<ProjectSnapshot> {
        match call(engine, session, Some(project), None, Command::GetSnapshot)? {
            ResponseBody::Project(value) => Ok(*value),
            other => anyhow::bail!("expected project snapshot, received {other:?}"),
        }
    }

    fn page(
        engine: &Arc<Engine>,
        session: &SessionId,
        project: &ProjectId,
        job: &JobId,
    ) -> Result<JobAttemptPage> {
        match call(
            engine,
            session,
            Some(project),
            None,
            Command::JobAttempts {
                job_id: job.clone(),
                after_attempt: None,
                limit: 8,
            },
        )? {
            ResponseBody::JobAttempts(value) => {
                value.validate()?;
                Ok(*value)
            }
            other => anyhow::bail!("expected attempt history, received {other:?}"),
        }
    }

    fn manifest(engine: &Arc<Engine>, job: &JobSnapshot) -> Result<WorkerRequest> {
        let db = engine
            .db
            .lock()
            .map_err(|_| anyhow::anyhow!("history DB lock poisoned"))?;
        let raw: String = db.query_row("SELECT CASE WHEN length(CAST(manifest AS BLOB))<=1048576 THEN manifest ELSE NULL END FROM attempts WHERE id=?1 AND job=?2", params![job.attempt_id.as_str(), job.job_id.as_str()], |row| row.get(0))?;
        let manifest: WorkerRequest = serde_json::from_str(&raw)?;
        manifest.validate()?;
        ensure!(
            manifest.attempt_id == job.attempt_id && manifest.job_id == job.job_id,
            "retained attempt identity changed"
        );
        Ok(manifest)
    }

    fn wait_terminal(
        engine: &Arc<Engine>,
        session: &SessionId,
        project: &ProjectId,
        started: &JobSnapshot,
        deadline: Instant,
    ) -> Result<JobSnapshot> {
        loop {
            let status = match call(
                engine,
                session,
                Some(project),
                None,
                Command::JobStatus {
                    job_id: started.job_id.clone(),
                },
            )? {
                ResponseBody::Job(value) => value,
                other => anyhow::bail!("expected job status, received {other:?}"),
            };
            ensure!(
                status.attempt_id == started.attempt_id,
                "unexpected replacement attempt"
            );
            if ["completed", "failed", "cancelled", "interrupted"].contains(&status.state.as_str())
            {
                return Ok(status);
            }
            ensure!(
                Instant::now() < deadline,
                "native job did not settle within 60 seconds: {status:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn history_matches(
        page: &JobAttemptPage,
        job: &JobSnapshot,
        profile: &DeclaredInferenceProfile,
        manifest: &WorkerRequest,
        epoch: &str,
    ) -> Result<()> {
        page.validate()?;
        ensure!(
            page.history_only
                && page.job_id == job.job_id
                && page.current_attempt == job.attempt_id
                && page.attempts.len() == 1
                && page.next_after.is_none(),
            "history page identity/cardinality changed"
        );
        let record = &page.attempts[0];
        let expected = crate::inference_profiles::profile_identity(profile)?;
        ensure!(
            record.attempt_id == job.attempt_id && record.state == job.state,
            "history state differs from original job"
        );
        ensure!(
            record.declared_profile_sha256.as_deref() == Some(expected.sha256.as_str())
                && serde_json::to_vec(
                    record
                        .declared_profile
                        .as_ref()
                        .context("explicit profile absent")?
                )? == serde_json::to_vec(profile)?,
            "explicit profile/hash differs"
        );
        let outcome = record
            .outcome
            .as_ref()
            .context("terminal attempt outcome absent")?;
        outcome.validate_for(manifest)?;
        ensure!(
            outcome.engine_epoch.as_str() == epoch,
            "outcome Engine epoch changed"
        );
        Ok(())
    }

    fn profile(engine: &Arc<Engine>, path: &Path) -> Result<DeclaredInferenceProfile> {
        let model = engine.snapshot_model(path)?;
        let runtime = engine
            .tools
            .as_ref()
            .map_err(|error| anyhow::anyhow!("Engine tools unavailable: {error}"))?
            .onnx_runtime
            .as_ref()
            .context("pinned ONNX runtime absent")?;
        ensure!(
            runtime.identity.sha256 == ORT_SHA,
            "Engine runtime differs from selected pinned runtime"
        );
        Ok(DeclaredInferenceProfile {
            schema_version: 1,
            model: model.identity,
            runtime: runtime.identity.clone(),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 6,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::TemporalScriptPosition,
                },
                resize: InferenceResize::NearestStretch,
                color: InferenceColor::RgbFramePair,
                scale: vec![1.0 / 255.0; 6],
                bias: vec![0.0; 6],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 128 * 1024 * 1024,
            minimum_output_bytes: 1024 * 1024,
            cpu_threads: 2,
            cuda_resources: None,
        })
    }

    fn generate(
        engine: &Arc<Engine>,
        session: &SessionId,
        project: &ProjectId,
        source: &SourceVersionId,
        profile: &DeclaredInferenceProfile,
        model: &Path,
    ) -> Result<JobSnapshot> {
        let settings = GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            source_sample_stride: 2,
            model_input: Some(profile.preprocessing.input.clone()),
            inference_profile: Some(profile.clone()),
            ..GenerationSettings::default()
        };
        match call(
            engine,
            session,
            Some(project),
            Some(0),
            Command::Generate {
                source_version: source.clone(),
                preset: "default".into(),
                settings: Some(settings),
                model_path: Some(model.to_owned()),
            },
        )? {
            ResponseBody::Job(value) => Ok(value),
            other => anyhow::bail!("expected public Generate job, received {other:?}"),
        }
    }

    struct Saved {
        session: SessionId,
        project: ProjectId,
        original: ProjectSnapshot,
        epoch: String,
        success: JobSnapshot,
        failure: JobSnapshot,
        success_history: JobAttemptPage,
        failure_history: JobAttemptPage,
    }

    fn first_run(
        engine: &Arc<Engine>,
        directory: &Path,
        output: &Path,
        completion: &mut Value,
    ) -> Result<Saved> {
        ensure!(
            launch::available(),
            "actual admitted Linux worker confinement required"
        );
        let runtime_path = env_path("PULSAR_ORT_DYLIB")?;
        let worker_path = env_path("PULSAR_ATTEMPT_HISTORY_WORKER")?;
        let media = identity(Path::new(MEDIA), Some(MEDIA_SHA))?;
        let model = identity(Path::new(MODEL), Some(MODEL_SHA))?;
        let runtime = identity(&runtime_path, Some(ORT_SHA))?;
        let worker = identity(&worker_path, None)?;
        save(
            output,
            "subjects.json",
            &json!({"media":media,"model":model,"runtime":runtime,"worker":worker,"authored_neutral_inputs":true,"trained_model":false,"physical_device":false,"qualified":false}),
        )?;
        let session = pair(engine, "authored CPU attempt history");
        let project = match call(
            engine,
            &session,
            None,
            None,
            Command::CreateProject {
                name: "Authored CPU attempt history".into(),
            },
        )? {
            ResponseBody::Project(project) => project.project_id.clone(),
            other => anyhow::bail!("project creation response: {other:?}"),
        };
        let source = match call(
            engine,
            &session,
            Some(&project),
            Some(0),
            Command::ImportSource {
                path: PathBuf::from(MEDIA),
            },
        )? {
            ResponseBody::Source { source_version } => source_version,
            other => anyhow::bail!("source import response: {other:?}"),
        };
        let original = snapshot(engine, &session, &project)?;
        ensure!(
            original.revision.value() == 0 && original.motion.program.action_count() == 0,
            "fixture master was not initially empty"
        );
        save(output, "master-before.json", &original)?;
        let good_profile = profile(engine, Path::new(MODEL))?;
        ensure!(
            good_profile.model.sha256 == MODEL_SHA,
            "captured model identity differs"
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        let started = generate(
            engine,
            &session,
            &project,
            &source,
            &good_profile,
            Path::new(MODEL),
        )?;
        save(output, "success-start.json", &started)?;
        let success = wait_terminal(engine, &session, &project, &started, deadline)?;
        save(output, "success-completed.json", &success)?;
        let success_history = page(engine, &session, &project, &success.job_id)?;
        save(output, "success-history.json", &success_history)?;
        let success_manifest = manifest(engine, &success)?;
        history_matches(
            &success_history,
            &success,
            &good_profile,
            &success_manifest,
            &engine.engine_epoch,
        )?;
        ensure!(
            success.state == "completed",
            "actual CPU generation failed: {success:?}"
        );
        let outcome = success_history.attempts[0]
            .outcome
            .as_ref()
            .context("success outcome absent")?;
        let retired = outcome
            .worker_retirement
            .as_ref()
            .context("success lacks confirmed worker retirement")?;
        ensure!(
            outcome.validation_error.is_none()
                && retired.worker_error.is_none()
                && retired.provider_unavailable.is_none(),
            "successful worker history contains a failure"
        );
        let candidate_id = success
            .candidate_id
            .as_ref()
            .context("successful generation has no candidate")?;
        let candidate = match call(
            engine,
            &session,
            Some(&project),
            None,
            Command::GetCandidate {
                candidate_id: candidate_id.clone(),
            },
        )? {
            ResponseBody::Candidate(candidate) => candidate,
            other => anyhow::bail!("candidate response: {other:?}"),
        };
        candidate.motion.validate()?;
        ensure!(
            candidate.job_id.as_ref() == Some(&success.job_id)
                && candidate.motion.program.action_count() >= 2,
            "actual CPU candidate is absent or empty"
        );
        save(output, "success-candidate.json", &candidate)?;
        let receipt = read_json(&success_manifest.output_dir.join("receipt.json"))?;
        save(output, "success-native-receipt.json", &receipt)?;
        ensure!(
            receipt["qualification"] == "unqualified"
                && receipt["attempt_id"] == success.attempt_id.as_str()
                && receipt["inference_execution"]["provider"] == "cpu"
                && receipt["inference_execution"]["successful_runs"]
                    .as_u64()
                    .is_some_and(|n| n > 0),
            "actual CPU execution receipt absent or mismatched"
        );
        let program_file =
            artifacts::open_regular(&success_manifest.output_dir.join("program.json"))?;
        ensure!(
            program_file.metadata()?.len() <= MAX_MOTION_BYTES,
            "program exceeds bounded read"
        );
        let program: MotionProgram =
            serde_json::from_reader(program_file.take(MAX_MOTION_BYTES + 1))?;
        validate_program(&program)?;
        let track = program
            .track(Axis::Stroke)
            .context("CPU program has no Stroke track")?;
        ensure!(
            track.actions().len() >= 2
                && track
                    .actions()
                    .iter()
                    .all(|action| action.time().as_nanos() >= 0
                        && action.time().as_nanos() <= 40_000_000_000
                        && action.position().value().is_finite()),
            "CPU motion is not finite within the authored source interval"
        );
        completion["actual_cpu_inference"] = json!(true);

        let invalid_path = directory.join("accurately-pinned-invalid.onnx");
        let mut invalid_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&invalid_path)?;
        invalid_file.write_all(b"authored invalid ONNX protobuf, not model weights\n")?;
        invalid_file.sync_all()?;
        drop(invalid_file);
        let invalid_profile = profile(engine, &invalid_path)?;
        ensure!(
            invalid_profile.model == identity(&invalid_path, None)?
                && invalid_profile.model != good_profile.model,
            "invalid model was not accurately captured"
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        let started = generate(
            engine,
            &session,
            &project,
            &source,
            &invalid_profile,
            &invalid_path,
        )?;
        save(output, "invalid-model-start.json", &started)?;
        let failure = wait_terminal(engine, &session, &project, &started, deadline)?;
        save(output, "invalid-model-completed.json", &failure)?;
        let failure_history = page(engine, &session, &project, &failure.job_id)?;
        save(output, "invalid-model-history.json", &failure_history)?;
        let failure_manifest = manifest(engine, &failure)?;
        history_matches(
            &failure_history,
            &failure,
            &invalid_profile,
            &failure_manifest,
            &engine.engine_epoch,
        )?;
        ensure!(
            failure.state == "failed" && failure.candidate_id.is_none(),
            "invalid model did not fail without a candidate: {failure:?}"
        );
        let outcome = failure_history.attempts[0]
            .outcome
            .as_ref()
            .context("failure outcome absent")?;
        let retired = outcome
            .worker_retirement
            .as_ref()
            .context("failure lacks confirmed worker retirement")?;
        let error = retired
            .worker_error
            .as_ref()
            .context("failure lacks an actual worker diagnostic")?;
        ensure!(
            error.code == ErrorCode::Unavailable
                && !error.retryable
                && outcome.validation_error.as_ref() == Some(error)
                && retired.provider_unavailable.is_none(),
            "invalid model was misclassified as provider absence or lacked settled failure"
        );
        ensure!(
            error.message.contains("ONNX adapter failed")
                && error.message.contains("Load model")
                && (error.message.contains("Protobuf") || error.message.contains("protobuf")),
            "failure did not reach actual ONNX model parsing: {}",
            error.message
        );
        let current = snapshot(engine, &session, &project)?;
        ensure!(
            current.revision == original.revision && current.motion == original.motion,
            "generation mutated the original master"
        );
        ensure!(
            success.job_id != failure.job_id && success.attempt_id != failure.attempt_id,
            "separate jobs reused an attempt"
        );
        completion["jobs"] = json!({"success":success.job_id,"invalid_model":failure.job_id});
        completion["initial_engine_epoch"] = json!(engine.engine_epoch);
        Ok(Saved {
            session,
            project,
            original,
            epoch: engine.engine_epoch.clone(),
            success,
            failure,
            success_history,
            failure_history,
        })
    }

    fn custody_clear(engine: &Arc<Engine>) -> Result<()> {
        ensure!(
            engine
                .workers
                .lock()
                .map_err(|_| anyhow::anyhow!("worker registry poisoned"))?
                .is_empty(),
            "worker registry is not empty after shutdown"
        );
        ensure!(
            engine.pool.reserved_cpu_threads()? == 0,
            "CPU custody remains after shutdown"
        );
        let fence = read_json(&engine.config.state_dir.join("worker-fence.json"))?;
        for name in ["groups", "retired", "staging"] {
            ensure!(
                fence[name].as_object().is_some_and(|v| v.is_empty()),
                "worker-fence {name} retains custody"
            );
        }
        ensure!(
            fence["creation"]["records"]
                .as_object()
                .is_some_and(|v| v.is_empty()),
            "worker creation custody remains"
        );
        Ok(())
    }

    fn panic_text(value: Box<dyn std::any::Any + Send>) -> String {
        value
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| value.downcast_ref::<&str>().map(|v| (*v).to_owned()))
            .unwrap_or_else(|| "non-string panic".into())
    }

    fn with_shutdown<T>(
        engine: Arc<Engine>,
        output: &Path,
        name: &str,
        run: impl FnOnce(&Arc<Engine>) -> Result<T>,
    ) -> (Result<T>, Value) {
        let work = catch_unwind(AssertUnwindSafe(|| run(&engine))).unwrap_or_else(|p| {
            Err(anyhow::anyhow!(
                "native campaign panicked: {}",
                panic_text(p)
            ))
        });
        let shutdown = catch_unwind(AssertUnwindSafe(|| {
            engine.request_shutdown();
            engine.finish_shutdown()
        }))
        .unwrap_or_else(|p| Err(anyhow::anyhow!("shutdown panicked: {}", panic_text(p))));
        let finished = shutdown.is_ok();
        let checked = if finished {
            custody_clear(&engine)
        } else {
            Err(anyhow::anyhow!("shutdown did not finish"))
        };
        let clear = checked.is_ok();
        let mut failures = Vec::new();
        if let Err(error) = shutdown {
            failures.push(format!("shutdown: {error:#}"));
        }
        if let Err(error) = checked {
            failures.push(format!("custody: {error:#}"));
        }
        let report = json!({"finished":finished,"custody_clear":clear,"error":if failures.is_empty(){None}else{Some(failures.join("; "))},"qualified":false});
        drop(engine);
        if let Err(error) = save(output, name, &report) {
            failures.push(format!("shutdown receipt: {error:#}"));
        }
        let value = match work {
            Ok(value) => Some(value),
            Err(error) => {
                failures.push(format!("run: {error:#}"));
                None
            }
        };
        let result = if failures.is_empty() {
            value.context("campaign result absent")
        } else {
            Err(anyhow::anyhow!(failures.join("; ")))
        };
        (result, report)
    }

    fn restart_run(
        engine: &Arc<Engine>,
        saved: &Saved,
        output: &Path,
        completion: &mut Value,
    ) -> Result<()> {
        ensure!(
            engine.engine_epoch != saved.epoch,
            "Engine epoch did not change on reopen"
        );
        let before = {
            let db = engine
                .db
                .lock()
                .map_err(|_| anyhow::anyhow!("DB poisoned"))?;
            db.query_row("SELECT COUNT(*) FROM requests", [], |row| {
                row.get::<_, i64>(0)
            })?
        };
        let success = page(
            engine,
            &saved.session,
            &saved.project,
            &saved.success.job_id,
        )?;
        let failure = page(
            engine,
            &saved.session,
            &saved.project,
            &saved.failure.job_id,
        )?;
        save(output, "restart-success-history.json", &success)?;
        save(output, "restart-invalid-model-history.json", &failure)?;
        ensure!(
            serde_json::to_vec(&success)? == serde_json::to_vec(&saved.success_history)?
                && serde_json::to_vec(&failure)? == serde_json::to_vec(&saved.failure_history)?,
            "restart changed original attempt observations"
        );
        let (requests, jobs, attempts, active, candidates) = {
            let db = engine
                .db
                .lock()
                .map_err(|_| anyhow::anyhow!("DB poisoned"))?;
            db.query_row("SELECT (SELECT COUNT(*) FROM requests),(SELECT COUNT(*) FROM jobs),(SELECT COUNT(*) FROM attempts),(SELECT COUNT(*) FROM jobs WHERE state NOT IN ('completed','failed','cancelled','interrupted')),(SELECT COUNT(*) FROM candidates)", [], |row| Ok((row.get::<_, i64>(0)?,row.get::<_, i64>(1)?,row.get::<_, i64>(2)?,row.get::<_, i64>(3)?,row.get::<_, i64>(4)?)))?
        };
        ensure!(
            requests == before && jobs == 2 && attempts == 2 && active == 0 && candidates == 1,
            "history read or restart created work/receipts"
        );
        ensure!(
            engine
                .workers
                .lock()
                .map_err(|_| anyhow::anyhow!("worker registry poisoned"))?
                .is_empty()
                && engine.pool.reserved_cpu_threads()? == 0,
            "restart restored execution custody"
        );
        let current = snapshot(engine, &saved.session, &saved.project)?;
        ensure!(
            current.revision == saved.original.revision && current.motion == saved.original.motion,
            "restart changed original master"
        );
        save(
            output,
            "restart-state.json",
            &json!({"jobs":jobs,"attempts":attempts,"active_jobs":active,"candidates":candidates,"history_only":true,"old_outcome_epoch":saved.epoch,"current_engine_epoch":engine.engine_epoch,"worker_custody_restored":false,"history_grants_execution_authority":false,"qualified":false}),
        )?;
        completion["history_only_restart"] = json!(true);
        completion["reopened_engine_epoch"] = json!(engine.engine_epoch);
        Ok(())
    }

    fn campaign(directory: &Path, output: &Path, completion: &mut Value) -> Result<()> {
        let mut config = EngineConfig::new(
            directory.join("state"),
            env_path("PULSAR_ATTEMPT_HISTORY_WORKER")?,
        );
        config.worker_timeout = Duration::from_secs(60);
        let engine = Engine::open(config.clone())?;
        completion["initial_engine_opened"] = json!(true);
        let (result, shutdown) = with_shutdown(engine, output, "shutdown-initial.json", |engine| {
            first_run(engine, directory, output, completion)
        });
        completion["initial_shutdown"] = shutdown;
        let saved = result?;
        // Only the established transient-lease helper may wait before reopening;
        // execution or model failures are never retried with a new job.
        let engine = reopen_after_transient_leases(config)?;
        completion["reopened_engine_opened"] = json!(true);
        let (result, shutdown) =
            with_shutdown(engine, output, "shutdown-reopened.json", |engine| {
                restart_run(engine, &saved, output, completion)
            });
        completion["reopened_shutdown"] = shutdown;
        result
    }

    #[test]
    #[ignore = "requires exact authored media/model, pinned ORT, current native worker and isolated private state"]
    fn native_cpu_attempt_history_preserves_success_and_invalid_model_failure() {
        let output = env_path("PULSAR_ATTEMPT_HISTORY_OUTPUT").expect("explicit output directory");
        if !output.exists() {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&output)
                .expect("new private output");
        }
        let metadata = fs::symlink_metadata(&output).expect("output metadata");
        assert!(
            metadata.is_dir() && metadata.permissions().mode() & 0o077 == 0,
            "output must be a private directory"
        );
        let directory = tempfile::tempdir().expect("private native scratch");
        let mut completion = json!({"schema_version":1,"passed":false,"qualified":false,"actual_cpu_inference":false,"physical_device":false,"accepted_equivalence":false,"fallback_performed":false,"history_only_restart":false,"initial_engine_opened":false,"reopened_engine_opened":false,"initial_shutdown":null,"reopened_shutdown":null,"error":null,"scope":"Actual CPU generation and settled invalid-model history on authored neutral input; no useful-model, provider-equivalence, physical or release qualification"});
        let result = catch_unwind(AssertUnwindSafe(|| {
            campaign(directory.path(), &output, &mut completion)
        }))
        .unwrap_or_else(|p| Err(anyhow::anyhow!("campaign panicked: {}", panic_text(p))));
        let joined = ["initial", "reopened"].into_iter().all(|name| {
            completion[format!("{name}_engine_opened")] != true
                || (completion[format!("{name}_shutdown")]["finished"] == true
                    && completion[format!("{name}_shutdown")]["custody_clear"] == true)
        });
        let cleanup = if joined {
            directory.close().map_err(anyhow::Error::from)
        } else {
            let retained = directory.keep();
            completion["unjoined_private_scratch_retained"] = json!(retained);
            Err(anyhow::anyhow!(
                "owned scratch retained because worker custody is unresolved"
            ))
        };
        completion["scratch_removed"] = json!(cleanup.is_ok());
        let mut errors = Vec::new();
        if let Err(error) = result {
            errors.push(format!("{error:#}"));
        }
        if let Err(error) = cleanup {
            errors.push(format!("{error:#}"));
        }
        completion["passed"] = json!(errors.is_empty());
        if !errors.is_empty() {
            completion["error"] = json!(errors.join("; "));
        }
        let receipt = save(&output, "completion.json", &completion);
        assert!(
            receipt.is_ok(),
            "completion receipt failed after shutdown: {receipt:?}"
        );
        assert!(
            errors.is_empty(),
            "native attempt history failed after shutdown: {}",
            errors.join("; ")
        );
    }
}
