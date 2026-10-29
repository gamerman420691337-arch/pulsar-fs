// Private native SOFTWARE SIMULATION. No public acceptance, measured equivalence,
// human review, genuine provider absence or physical qualification is asserted.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod inference_fallback_native_tests {
    use super::*;
    use anyhow::{ensure, Context};
    use serde_json::{json, Value};
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
        time::{Duration, Instant},
    };
    use InferenceEquivalenceEvidenceDetails as D;
    use InferenceEquivalenceEvidenceKind as K;
    mod authored {
        include!("inference_equivalence_fixture.rs");
    }
    include!("engine_inference_fallback_active_tests.rs");
    include!("engine_inference_fallback_pack_native_fixture.rs");

    fn fixture_bytes(value: &impl serde::Serialize) -> Vec<u8> {
        serde_json::to_vec(value).unwrap()
    }
    fn fixture_hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn fixture_artifact(label: &str) -> ArtifactIdentity {
        let sha256 = format!("{:x}", Sha256::digest(label.as_bytes()));
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: label.len() as u64,
        }
    }

    fn seal_simulated_shape(
        subject: InferenceEquivalenceSubject,
        mut details: BTreeMap<
            InferenceEquivalenceEvidenceKind,
            InferenceEquivalenceEvidenceDetails,
        >,
    ) -> (
        InferenceEquivalenceManifest,
        BTreeMap<InferenceEquivalenceEvidenceKind, Vec<u8>>,
    ) {
        use InferenceEquivalenceEvidenceDetails as D;
        use InferenceEquivalenceEvidenceKind as K;
        let subject_sha256 = subject.subject_sha256().unwrap();
        let mut bytes: BTreeMap<K, Vec<u8>> = BTreeMap::new();
        for kind in K::ALL {
            let hashes = |role| fixture_hash(&bytes[&role]);
            let value = details.get_mut(&kind).unwrap();
            match value {
                D::QualityComparison(v) => {
                    v.corpus_sha256 = hashes(K::Corpus);
                    v.protocol_sha256 = hashes(K::Protocol);
                    v.from_execution_sha256 = hashes(K::FromExecution);
                    v.to_execution_sha256 = hashes(K::ToExecution);
                }
                D::ProviderPlacement(v) => {
                    v.from_execution_sha256 = hashes(K::FromExecution);
                    v.to_execution_sha256 = hashes(K::ToExecution);
                }
                D::Resources(v) => {
                    v.from_execution_sha256 = hashes(K::FromExecution);
                    v.to_execution_sha256 = hashes(K::ToExecution);
                }
                D::Review(v) => {
                    v.reviewed_evidence = K::ALL
                        .into_iter()
                        .filter(|role| *role != K::Review)
                        .map(|role| InferenceEquivalenceReviewedEvidence {
                            kind: role,
                            sha256: hashes(role),
                        })
                        .collect();
                }
                _ => {}
            }
            bytes.insert(
                kind,
                fixture_bytes(&InferenceEquivalenceEvidenceDocument {
                    schema_version: 1,
                    origin: subject.origin,
                    subject_sha256: subject_sha256.clone(),
                    kind,
                    details: value.clone(),
                }),
            );
        }
        let evidence = bytes
            .iter()
            .map(|(kind, raw)| {
                let sha256 = fixture_hash(raw);
                InferenceEquivalenceEvidence {
                    kind: *kind,
                    identity: ArtifactIdentity {
                        artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                        sha256,
                        byte_len: raw.len() as u64,
                    },
                }
            })
            .collect();
        let manifest = InferenceEquivalenceManifest { subject, evidence };
        manifest.validate().unwrap();
        (manifest, bytes)
    }

    const MEDIA: &str = "/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-23/hsp-continuity/rendered-fixture/sterile-test-pattern-40s.mp4";
    const MODEL: &str = "/mnt/4tb/artifacts/Funscripts/keep/pulsar-roadmap-m1/roadmap-2026-10-23/hsp-live-source/native-cpu-fixture/authored-temporal.onnx";
    const MEDIA_SHA: &str = "cfaafb105b7db1b5406ddfcaa913d9d635c990ec3f4dacd6818dee0b3c4eef12";
    const MODEL_SHA: &str = "9a10ed332a69f241c880c0ff8b08f8d234371e6702a59f868510fef7550078c8";
    const ORT_SHA: &str = "b5c9d4f124d24707f514dad926dc181820807178855df1c528e3addb2dd0e6f7";

    fn environment_path(name: &str) -> Result<PathBuf> {
        let path =
            PathBuf::from(std::env::var_os(name).with_context(|| format!("{name} required"))?);
        ensure!(path.is_absolute(), "{name} must be absolute");
        Ok(path)
    }
    fn file_identity(path: &Path) -> Result<ArtifactIdentity> {
        let (sha256, byte_len) = artifacts::digest_file(path, 512 * 1024 * 1024)?;
        Ok(ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone())?,
            sha256,
            byte_len,
        })
    }
    fn json_file(path: &Path) -> Result<Value> {
        let file = artifacts::open_regular(path)?;
        ensure!(
            file.metadata()?.len() <= 4 * 1024 * 1024,
            "native receipt exceeds limit"
        );
        Ok(serde_json::from_reader(file.take(4 * 1024 * 1024 + 1))?)
    }
    fn save(path: &Path, value: &Value) -> Result<()> {
        let raw = serde_json::to_vec_pretty(value)?;
        ensure!(
            raw.len() <= 8 * 1024 * 1024,
            "simulation receipt exceeds bound"
        );
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(&raw)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    }
    fn call(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: Option<&ProjectId>,
        revision: Option<u64>,
        command: Command,
    ) -> Result<ResponseBody> {
        Ok(engine
            .handle(request(actor, project, revision, command))
            .result?)
    }
    fn snapshot(
        engine: &Arc<Engine>,
        actor: &SessionId,
        project: &ProjectId,
    ) -> Result<ProjectSnapshot> {
        let ResponseBody::Project(value) =
            call(engine, actor, Some(project), None, Command::GetSnapshot)?
        else {
            anyhow::bail!("project response missing");
        };
        Ok(*value)
    }
    fn selected_profile(
        model: &SourceArtifact,
        runtime: ArtifactIdentity,
        minimum_memory_bytes: u64,
    ) -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model: model.identity.clone(),
            runtime,
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
            minimum_memory_bytes,
            minimum_output_bytes: 1024 * 1024,
            cpu_threads: 2,
            cuda_resources: None,
        }
    }
    fn settings(profile: &DeclaredInferenceProfile) -> GenerationSettings {
        GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            source_sample_stride: 2,
            model_input: Some(profile.preprocessing.input.clone()),
            inference_profile: Some(profile.clone()),
            ..Default::default()
        }
    }
    fn selection(
        reference: InferenceFallbackReference,
        source: &SourceArtifact,
    ) -> InferenceFallbackSelection {
        InferenceFallbackSelection {
            schema_version: 1,
            equivalents: vec![reference],
            maximum_fallbacks: 1,
            source_policy: InferenceFallbackSourcePolicy {
                schema_version: 1,
                kind: InferenceFallbackSourceKind::DecodedMedia,
                sources: Some(vec![source.identity.clone()]),
                independent_audio: InferenceFallbackIndependentAudio::Forbidden,
            },
            view_policy: InferenceFallbackViewPolicy {
                schema_version: 1,
                applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
            },
        }
    }

    /// The only fabricated eligibility is installed inside this disposable test DB.
    /// No public acceptance method is called, and none of these records/documents is
    /// exported in the native receipt. Every textual attribution denotes simulation.
    fn install_simulated_eligibility(
        engine: &Arc<Engine>,
        actor: &SessionId,
        source: &SourceArtifact,
        model: &SourceArtifact,
        from: &DeclaredInferenceProfile,
        to: &DeclaredInferenceProfile,
        fixture_validity_ms: u64,
    ) -> Result<InferenceFallbackSelection> {
        ensure!(
            matches!(fixture_validity_ms, 45_000 | 300_000),
            "private fixture validity bound"
        );
        let from_environment = engine
            .observe_inference_environment(from, Instant::now() + Duration::from_secs(30))?
            .environment()
            .clone();
        let to_environment = engine
            .observe_inference_environment(to, Instant::now() + Duration::from_secs(30))?
            .environment()
            .clone();
        let now = inference_fallback_execution::wall_time()?;
        let (mut manifest, documents) = authored::synthetic_fixture();
        let mut details = authored::fixture_documents(&documents);
        let provisional = selection(
            InferenceFallbackReference {
                dossier_id: ArtifactId::new("0".repeat(64))?,
                expected_revision: 2,
                expected_report_sha256: "0".repeat(64),
            },
            source,
        );
        manifest.subject.origin = InferenceEquivalenceOrigin::Measured;
        manifest.subject.from_profile = from.clone();
        manifest.subject.to_profile = to.clone();
        manifest.subject.from_environment = from_environment.clone();
        manifest.subject.to_environment = to_environment.clone();
        manifest.subject.not_before_unix_ms = now.saturating_sub(1000);
        manifest.subject.expires_unix_ms = now
            .checked_add(fixture_validity_ms)
            .context("fixture validity overflow")?;
        manifest.subject.scope.recipe = InferenceEquivalenceRecipe::TemporalPosition;
        manifest.subject.scope.source_policy = provisional.source_policy.identity()?;
        manifest.subject.scope.view_policy = provisional.view_policy.identity()?;
        manifest.subject.scope.limitations="SOFTWARE SIMULATION ONLY: fabricated eligibility shape, no measured equivalence, human review or qualification".into();
        let scope = manifest.subject.scope_sha256()?;
        if let D::Corpus(corpus) = details.get_mut(&K::Corpus).unwrap() {
            corpus.split = InferenceEquivalenceSplit::Heldout;
            corpus.heldout_freeze = Some(fixture_artifact(
                "SOFTWARE SIMULATION - no real corpus freeze",
            ));
            for case in &mut corpus.cases {
                case.source_sha256 = source.identity.sha256.clone();
                case.independent_annotations = true;
            }
        }
        if let D::Protocol(protocol) = details.get_mut(&K::Protocol).unwrap() {
            protocol.scope_sha256 = scope.clone();
            protocol.approval_record =
                fixture_artifact("SOFTWARE SIMULATION - no human criteria approval");
        }
        for (kind, profile, environment) in [
            (K::FromExecution, from, &from_environment),
            (K::ToExecution, to, &to_environment),
        ] {
            let execution = match details.get_mut(&kind).unwrap() {
                D::FromExecution(v) | D::ToExecution(v) => v,
                _ => unreachable!(),
            };
            execution.scope_sha256 = scope.clone();
            for case in &mut execution.cases {
                case.profile_sha256 = fixture_hash(&fixture_bytes(profile));
                case.environment_sha256 = environment.identity_sha256()?;
                case.manifest.source = source.clone();
                case.manifest.tools = engine
                    .tools
                    .as_ref()
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .clone();
                case.manifest.tools.onnx_runtime = Some(
                    engine
                        .cpu_owner_policy
                        .as_ref()
                        .unwrap()
                        .runtime_reference()?,
                );
                case.manifest.dependencies = vec![profile.model.clone(), profile.runtime.clone()];
                case.manifest.budget = ResourceBudget {
                    memory_bytes: 512 * 1024 * 1024,
                    output_bytes: 16 * 1024 * 1024,
                    wall_time_ms: 1000,
                    cpu_threads: 2,
                };
                let WorkerOperation::Generate {
                    settings,
                    model: declared,
                    ..
                } = &mut case.manifest.operation
                else {
                    unreachable!()
                };
                let interval = settings.project_interval;
                **settings = GenerationSettings {
                    project_interval: interval,
                    ..self::settings(profile)
                };
                *declared = Some(model.clone());
                case.manifest.validate()?;
            }
        }
        if let D::ProviderPlacement(placement) = details.get_mut(&K::ProviderPlacement).unwrap() {
            for case in &mut placement.cases {
                case.from.profile_sha256 = fixture_hash(&fixture_bytes(from));
                case.to.profile_sha256 = fixture_hash(&fixture_bytes(to));
            }
        }
        if let D::Review(review) = details.get_mut(&K::Review).unwrap() {
            review.reviewed_at_unix_ms = now;
            review.reviewer = "SOFTWARE SIMULATION - no reviewer person".into();
            review.conclusion = InferenceEquivalenceConclusion::EquivalentWithinScope;
            review.limitations =
                "Eligibility enum shape only, not real measurement or review".into();
        }
        let (manifest, documents) = seal_simulated_shape(manifest.subject, details);
        validate_evidence(&manifest, &documents)?.require_acceptance(now)?;
        let mut db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
        let tx = db.transaction()?;
        let mut report = inference_equivalence_store::import_pending(
            &tx,
            &manifest,
            &documents.into_iter().collect::<Vec<_>>(),
        )?;
        let request_id = RequestId::new(format!("simulation-record-{}", Uuid::new_v4()))?;
        report.revision = 2;
        report.state = InferenceEquivalenceState::Accepted;
        report.decision = Some(InferenceEquivalenceDecision {
            revision: 2,
            state: InferenceEquivalenceState::Accepted,
            request_id: request_id.clone(),
            actor_session: actor.clone(),
            operator: Some("SOFTWARE SIMULATION - no operator acceptance".into()),
            note: String::new(),
            recorded_at_unix_ms: Some(now),
            review_identity: Some(report.manifest.evidence_identity(K::Review)?.clone()),
        });
        report.validate()?;
        crate::inference_equivalence_reader::arm(&tx)?;
        let raw = encode(&report)?;
        let report_hash = fixture_hash(raw.as_bytes());
        ensure!(tx.execute("UPDATE inference_equivalence_records SET revision=2,report=?2 WHERE id=?1 AND revision=1",params![report.dossier_id.as_str(),raw])?==1,"simulation row changed");
        tx.execute(
            "INSERT INTO inference_equivalence_decisions VALUES(?1,2,?2,?3,?4)",
            params![
                report.dossier_id.as_str(),
                actor.as_str(),
                request_id.as_str(),
                report_hash
            ],
        )?;
        let fingerprint =
            fixture_hash(b"SOFTWARE SIMULATION - not an API acceptance authorization");
        tx.execute(
            "INSERT INTO request_identities(session,request,fingerprint) VALUES(?1,?2,?3)",
            params![actor.as_str(), request_id.as_str(), fingerprint],
        )?;
        let receipt =
            ResponseBody::InferenceEquivalenceSaved(Box::new(InferenceEquivalenceReceipt {
                request_id: request_id.clone(),
                input_sha256: None,
                report: report.clone(),
            }));
        tx.execute(
            "INSERT INTO requests VALUES(?1,?2,?3,?4)",
            params![
                actor.as_str(),
                request_id.as_str(),
                fingerprint,
                encode(&receipt)?
            ],
        )?;
        tx.execute(
            "UPDATE inference_equivalence_schema SET clock_floor_unix_ms=?1",
            [now],
        )?;
        tx.commit()?;
        inference_equivalence_store::validate(&db)?;
        Ok(selection(
            InferenceFallbackReference {
                dossier_id: report.dossier_id,
                expected_revision: 2,
                expected_report_sha256: report_hash,
            },
            source,
        ))
    }

    struct Inputs {
        actor: SessionId,
        project: ProjectId,
        source: SourceArtifact,
        model: SourceArtifact,
        from: DeclaredInferenceProfile,
        to: DeclaredInferenceProfile,
        selection: InferenceFallbackSelection,
        original: ProjectSnapshot,
    }
    fn prepare_inputs(engine: &Arc<Engine>) -> Result<Inputs> {
        prepare_inputs_with_validity(engine, 300_000)
    }
    fn prepare_inputs_with_validity(
        engine: &Arc<Engine>,
        fixture_validity_ms: u64,
    ) -> Result<Inputs> {
        let actor = pair(engine, "SOFTWARE SIMULATION native fallback controller");
        let ResponseBody::Project(project) = call(
            engine,
            &actor,
            None,
            None,
            Command::CreateProject {
                name: "SOFTWARE SIMULATION - neutral CPU fallback".into(),
            },
        )?
        else {
            anyhow::bail!("project missing");
        };
        let project = project.project_id.clone();
        let ResponseBody::Source { source_version } = call(
            engine,
            &actor,
            Some(&project),
            Some(0),
            Command::ImportSource {
                path: PathBuf::from(MEDIA),
            },
        )?
        else {
            anyhow::bail!("source missing");
        };
        let source = {
            let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
            engine.source(&db, &project, &source_version)?
        };
        let model = engine.snapshot_model(Path::new(MODEL))?;
        ensure!(
            source.identity.sha256 == MEDIA_SHA && model.identity.sha256 == MODEL_SHA,
            "authored input bytes changed"
        );
        let runtime = engine
            .cpu_owner_policy
            .as_ref()
            .context("CPU owner policy required")?
            .runtime_identity()?;
        ensure!(runtime.sha256 == ORT_SHA, "actual runtime pin changed");
        let from = selected_profile(&model, runtime.clone(), 256 * 1024 * 1024);
        let to = selected_profile(&model, runtime, 128 * 1024 * 1024);
        let selection = install_simulated_eligibility(
            engine,
            &actor,
            &source,
            &model,
            &from,
            &to,
            fixture_validity_ms,
        )?;
        let original = snapshot(engine, &actor, &project)?;
        Ok(Inputs {
            actor,
            project,
            source,
            model,
            from,
            to,
            selection,
            original,
        })
    }
    fn public_synthetic_refusal(
        engine: &Arc<Engine>,
        input: &Inputs,
        model: InferenceFallbackModel,
    ) -> Result<Value> {
        let model_kind = match &model {
            InferenceFallbackModel::Path { .. } => "path",
            InferenceFallbackModel::ModelPack { .. } => "model_pack",
        };
        let (manifest, documents) = authored::synthetic_fixture();
        let report = {
            let mut db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
            let tx = db.transaction()?;
            let report = inference_equivalence_store::import_pending(
                &tx,
                &manifest,
                &documents.into_iter().collect::<Vec<_>>(),
            )?;
            tx.commit()?;
            report
        };
        let report_sha = fixture_hash(&fixture_bytes(&report));
        let reference = InferenceFallbackReference {
            dossier_id: report.dossier_id,
            expected_revision: report.revision,
            expected_report_sha256: report_sha,
        };
        let request = request(
            &input.actor,
            Some(&input.project),
            Some(0),
            Command::GenerateWithInferenceFallback {
                source_version: input.source.source_version.clone(),
                preset: "default".into(),
                settings: Box::new(settings(&input.from)),
                model,
                selection: Box::new(selection(reference, &input.source)),
            },
        );
        let error = engine
            .handle(request)
            .result
            .expect_err("public Synthetic/Pending must refuse");
        ensure!(
            error.code == ErrorCode::Unavailable
                && error.message.contains("no current measured acceptance"),
            "public Synthetic case failed outside its qualification gate: {error}"
        );
        let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))?;
        ensure!(count == 0, "Synthetic refusal created a generation job");
        Ok(
            json!({"refused":true,"model_kind":model_kind,"code":error.code,"message":error.message,"generation_jobs":count,"public_acceptance_called":false}),
        )
    }
    struct NativeCustodyClear {
        engine_epoch: String,
        workers_checked: bool,
    }
    fn await_custody_clear(
        engine: &Arc<Engine>,
        require_workers_empty: bool,
    ) -> Result<NativeCustodyClear> {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let clear = !engine.worker_fence.has_pending_worker_custody()?
                && engine.pool.reserved_storage() == 0
                && engine.pool.reserved_cpu_threads()? == 0
                && (!require_workers_empty
                    || engine
                        .workers
                        .lock()
                        .map_err(|_| anyhow::anyhow!("worker lock"))?
                        .is_empty());
            if clear {
                return Ok(NativeCustodyClear {
                    engine_epoch: engine.engine_epoch.clone(),
                    workers_checked: require_workers_empty,
                });
            }
            ensure!(
                Instant::now() < end,
                "predecessor or successor custody did not clear"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn job(engine: &Arc<Engine>, input: &Inputs, id: &JobId) -> Result<JobSnapshot> {
        let ResponseBody::Job(value) = call(
            engine,
            &input.actor,
            Some(&input.project),
            None,
            Command::JobStatus { job_id: id.clone() },
        )?
        else {
            anyhow::bail!("job response missing");
        };
        Ok(value)
    }
    fn history(engine: &Arc<Engine>, input: &Inputs, id: &JobId) -> Result<JobAttemptPage> {
        let ResponseBody::JobAttempts(value) = call(
            engine,
            &input.actor,
            Some(&input.project),
            None,
            Command::JobAttempts {
                job_id: id.clone(),
                after_attempt: None,
                limit: 8,
            },
        )?
        else {
            anyhow::bail!("history missing");
        };
        Ok(*value)
    }
    fn retained_manifest(engine: &Arc<Engine>, value: &JobSnapshot) -> Result<WorkerRequest> {
        let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
        let raw:String=db.query_row("SELECT CASE WHEN length(CAST(manifest AS BLOB))<=1048576 THEN manifest ELSE NULL END FROM attempts WHERE id=?1 AND job=?2",params![value.attempt_id.as_str(),value.job_id.as_str()],|r|r.get(0))?;
        let manifest: WorkerRequest = serde_json::from_str(&raw)?;
        manifest.validate()?;
        Ok(manifest)
    }

    struct SimulatedWorkerHandle {
        engine: Arc<Engine>,
        job: JobId,
        flag: Arc<AtomicBool>,
    }
    impl Drop for SimulatedWorkerHandle {
        fn drop(&mut self) {
            self.flag.store(true, Ordering::Release);
            if let Ok(mut workers) = self.engine.workers.lock() {
                if workers
                    .get(self.job.as_str())
                    .is_some_and(|current| Arc::ptr_eq(current, &self.flag))
                {
                    workers.remove(self.job.as_str());
                }
            }
        }
    }
    fn success_mode(mode: &str) -> bool {
        matches!(mode, "success" | "pack_success")
    }
    fn pack_mode(mode: &str) -> bool {
        matches!(
            mode,
            "pack_success" | "pack_withdraw_pending" | "pack_withdraw_active"
        )
    }
    fn run_case(engine: &Arc<Engine>, mode: &str, archive: bool) -> Result<Value> {
        // Short record validity is authored before any admission. Neither the
        // accepted-shaped record nor a live lease is rewritten after dispatch.
        let input = if mode == "active_dossier_expiry" {
            prepare_inputs_with_validity(engine, 45_000)?
        } else {
            prepare_inputs(engine)?
        };
        let mut selected_pack = if pack_mode(mode) {
            Some(prepare_native_selected_pack(engine, &input, mode)?)
        } else {
            None
        };
        let model = selected_pack.as_ref().map_or_else(
            || InferenceFallbackModel::Path {
                path: PathBuf::from(MODEL),
            },
            |pack| InferenceFallbackModel::ModelPack {
                choice: pack.choice.clone(),
            },
        );
        let synthetic_refusal = public_synthetic_refusal(engine, &input, model.clone())?;
        await_custody_clear(engine, true)?;
        let prepared = engine.prepare_initial_fallback(&input.selection, &input.from)?;
        let operation = selected_pack.as_ref().map_or_else(
            || WorkerOperation::Generate {
                preset: "default".into(),
                settings: Box::new(settings(&input.from)),
                model: Some(input.model.clone()),
            },
            |pack| pack.operation.clone(),
        );
        let command = match selected_pack.as_ref() {
            Some(pack) => pack.generation_command(&input)?,
            None => Command::GenerateWithInferenceFallback {
                source_version: input.source.source_version.clone(),
                preset: "default".into(),
                settings: Box::new(settings(&input.from)),
                model,
                selection: Box::new(input.selection.clone()),
            },
        };
        let generation_request = request(
            &input.actor,
            Some(&input.project),
            Some(input.original.revision.value()),
            command,
        );
        let extra = selected_pack
            .as_ref()
            .map_or(&[][..], |pack| pack.dependencies.as_slice());
        let budget = crate::inference_budget::admit(
            ResourceBudget {
                memory_bytes: WORKER_MEMORY,
                output_bytes: OUTPUT_LIMIT,
                wall_time_ms: 60_000,
                cpu_threads: 2,
            },
            Some(&input.from),
            None,
            engine.config.max_ram_bytes,
        )?;
        let preparing = AtomicBool::new(false);
        let cpu = engine.capture_cpu_inference(
            &generation_request,
            &input.from,
            &budget,
            crate::resources::GpuPriority::Offline,
            &preparing,
        )?;
        let (manifest, reservation, cancel, admission) = {
            let mut db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
            let tx = db.transaction()?;
            engine.authorize(&tx, &generation_request, &input.actor)?;
            let (manifest, reservation, cancel) = engine.prepare_operation_with_capture(
                &tx,
                &generation_request,
                &input.source.source_version,
                operation,
                OperationDependencies {
                    origin: AttemptOrigin::New,
                    extra,
                    cuda: None,
                    cpu: Some(&cpu),
                    fallback: Some(&prepared),
                },
            )?;
            let admission = prepared.require_bound(&tx, &manifest)?;
            ensure!(
                tx.execute(
                    "UPDATE jobs SET state='running' WHERE id=?1 AND attempt=?2 AND state='queued'",
                    params![manifest.job_id.as_str(), manifest.attempt_id.as_str()]
                )? == 1,
                "primary state changed"
            );
            tx.execute(
                "UPDATE attempts SET state='running' WHERE id=?1",
                [manifest.attempt_id.as_str()],
            )?;
            tx.commit()?;
            (manifest, reservation, cancel, admission)
        };
        let pack_admission = selected_pack
            .as_ref()
            .map(|pack| verify_native_selected_pack_admission(engine, &input, pack, &manifest))
            .transpose()?;
        let mut pack_withdrawal = None;
        let mut pack_verification = None;
        let mut active_successor = None;
        engine
            .workers
            .lock()
            .map_err(|_| anyhow::anyhow!("worker lock"))?
            .insert(manifest.job_id.to_string(), cancel.clone());
        let _simulator_handle = SimulatedWorkerHandle {
            engine: engine.clone(),
            job: manifest.job_id.clone(),
            flag: cancel.clone(),
        };
        engine.validate_dependencies(&manifest)?;
        let lease =
            engine
                .fallback_lifetimes
                .register(inference_fallback_lifetime::Registration {
                    job: manifest.job_id.clone(),
                    project: manifest.project_id.clone(),
                    owner: input.actor.clone(),
                    attempt: manifest.attempt_id.clone(),
                    reference: admission.reference,
                    admitted_at_unix_ms: admission.admitted_at_unix_ms,
                    expires_at_unix_ms: admission.expires_at_unix_ms,
                    attempt_budget_ms: manifest.budget.wall_time_ms,
                    original_deadline: Instant::now()
                        + Duration::from_secs(if mode == "expiry" { 2 } else { 60 }),
                    cancel: cancel.clone(),
                })?;
        let claim = Arc::new(reservation);
        let mut custody = engine
            .worker_fence
            .admit_group(
                claim.clone(),
                manifest.budget.memory_bytes,
                manifest.budget.cpu_threads,
            )?
            .into_worker();
        let simulator = environment_path("PULSAR_FALLBACK_NATIVE_SIMULATOR")?;
        let mut simulator_config = engine.config.clone();
        simulator_config.worker_executable = simulator.clone();
        let command = crate::confinement::command(
            &simulator_config,
            &manifest.budget,
            manifest.budget.memory_bytes,
            &custody.group_handle(),
            &[],
            &manifest.output_dir,
            "software-simulated-provider-absence",
        )?;
        let bootstrap = json!({"software_simulation":true,"request":manifest,"profile_sha256":fixture_hash(&fixture_bytes(&input.from))});
        let report = launch::supervise_owned_attempt_with_progress(
            &manifest,
            &bootstrap,
            &cancel,
            custody,
            command,
            lease.deadline(),
            true,
        );
        let (outcome, settled) = report.into_parts();
        let settled =
            settled.context("supervised simulator did not produce opaque settled failure")?;
        ensure!(
            outcome.is_err() && settled.provider_unavailable().is_some(),
            "simulated primary not classified by real supervisor"
        );
        settled
            .provider_unavailable()
            .unwrap()
            .validate_for(&input.from, &fixture_hash(&fixture_bytes(&input.from)))?;
        let worker_retirement = Some(WorkerRetirementObservation {
            worker_error: Some(JobAttemptDiagnostic::from_error(settled.error())),
            provider_unavailable: settled.provider_unavailable().cloned(),
        });
        if mode == "expiry" {
            if let Some(remaining) = lease.deadline().checked_duration_since(Instant::now()) {
                std::thread::sleep(remaining + Duration::from_millis(5));
            }
            ensure!(lease.check().is_err(), "attempt lifetime did not expire");
        }
        let pending = engine.finish_job_observed_inner(
            &manifest,
            outcome,
            worker_retirement,
            Some(&lease),
            Some(&settled),
        )?;
        {
            let mut workers = engine
                .workers
                .lock()
                .map_err(|_| anyhow::anyhow!("worker lock"))?;
            ensure!(
                workers
                    .get(manifest.job_id.as_str())
                    .is_some_and(|current| Arc::ptr_eq(current, &cancel)),
                "predecessor cancellation handle replaced"
            );
            workers.remove(manifest.job_id.as_str());
        }
        lease.retire()?;
        drop(prepared);
        drop(claim);
        cpu.image.close()?;
        await_custody_clear(engine, true)?;
        let no_predecessor_custody = true;
        if mode == "expiry" {
            ensure!(!pending, "expired attempt scheduled fallback");
        } else {
            ensure!(pending, "simulated eligibility was not marked pending");
        }
        if mode == "cancel" {
            call(
                engine,
                &input.actor,
                Some(&input.project),
                None,
                Command::CancelJob {
                    job_id: manifest.job_id.clone(),
                },
            )?;
        }
        if mode == "withdraw" {
            call(
                engine,
                &input.actor,
                None,
                None,
                Command::WithdrawInferenceEquivalence {
                    dossier_id: input.selection.equivalents[0].dossier_id.clone(),
                    expected_record_revision: 2,
                    reason: "SOFTWARE SIMULATION fixture retirement; no real approval".into(),
                    owner_proof: engine.model_pack_owner_token.clone(),
                },
            )?;
        }
        if mode == "pack_withdraw_pending" {
            pack_withdrawal = Some(withdraw_native_selected_pack(
                engine,
                &input,
                selected_pack.as_ref().context("selected pack absent")?,
            )?);
        }
        let end = Instant::now() + Duration::from_secs(120);
        let continuation = if pending {
            Some(engine.continue_inference_fallback(&manifest, settled))
        } else {
            None
        };
        let continuation_error = match continuation {
            Some(Ok(())) => {
                ensure!(
                    success_mode(mode) || active_mode(mode),
                    "retired authority admitted a successor"
                );
                None
            }
            Some(Err(error)) => {
                ensure!(
                    !success_mode(mode) && !active_mode(mode),
                    "automatic controller refused target: {error}"
                );
                engine.fail_pending_fallback(&manifest, &error)?;
                Some(error)
            }
            None => None,
        };
        let mut active_consumer = if active_mode(mode) {
            let barrier = hold_active_successor(engine, &input.actor, &manifest, &input.to, end)?;
            ensure!(
                barrier.manifest().attempt_id != manifest.attempt_id
                    && barrier.checkpoint().record.analysed_samples >= 16,
                "active barrier did not bind real successor progress"
            );
            active_successor = Some(barrier.manifest().clone());
            let trigger = match mode {
                "active_cancel" => {
                    call(
                        engine,
                        &input.actor,
                        Some(&input.project),
                        None,
                        Command::CancelJob {
                            job_id: manifest.job_id.clone(),
                        },
                    )?;
                    "cancellation"
                }
                "active_withdraw" => {
                    call(
                        engine,
                        &input.actor,
                        None,
                        None,
                        Command::WithdrawInferenceEquivalence {
                            dossier_id: input.selection.equivalents[0].dossier_id.clone(),
                            expected_record_revision: 2,
                            reason:
                                "SOFTWARE SIMULATION active fixture retirement; no real approval"
                                    .into(),
                            owner_proof: engine.model_pack_owner_token.clone(),
                        },
                    )?;
                    "dossier_withdrawal"
                }
                "pack_withdraw_active" => {
                    pack_withdrawal = Some(withdraw_native_selected_pack(
                        engine,
                        &input,
                        selected_pack.as_ref().context("selected pack absent")?,
                    )?);
                    "pack_withdrawal"
                }
                "active_expiry" => "attempt_budget_expiry",
                "active_dossier_expiry" => "accepted_record_expiry",
                _ => unreachable!(),
            };
            Some(barrier.observe_invalidation_and_release(trigger, end)?)
        } else {
            None
        };
        let terminal = loop {
            let value = job(engine, &input, &manifest.job_id)?;
            if ["completed", "failed", "cancelled"].contains(&value.state.as_str()) {
                break value;
            }
            ensure!(Instant::now() < end, "automatic successor did not settle");
            std::thread::sleep(Duration::from_millis(20));
        };
        let cleared_custody = await_custody_clear(engine, true)?;
        let history = history(engine, &input, &manifest.job_id)?;
        history.validate()?;
        let (attempts, candidates, pending_rows): (i64, i64, i64) = {
            let db = engine.db.lock().map_err(|_| anyhow::anyhow!("DB lock"))?;
            (
                db.query_row(
                    "SELECT COUNT(*) FROM attempts WHERE job=?1",
                    [manifest.job_id.as_str()],
                    |r| r.get(0),
                )?,
                db.query_row("SELECT COUNT(*) FROM candidates", [], |r| r.get(0))?,
                db.query_row("SELECT COUNT(*) FROM inference_fallback_pending", [], |r| {
                    r.get(0)
                })?,
            )
        };
        ensure!(pending_rows == 0, "pending fallback survived settlement");
        if let Some(proof) = active_consumer.as_mut() {
            ensure!(
                attempts == 2 && candidates == 0 && terminal.attempt_id != manifest.attempt_id,
                "active invalidation produced candidate or unexpected attempt count"
            );
            proof["terminal_assertion"] =
                assert_active_terminal(mode, &terminal, &history, &cleared_custody)?;
        }
        let target = if success_mode(mode) {
            ensure!(
                terminal.state == "completed"
                    && terminal.attempt_id != manifest.attempt_id
                    && attempts == 2
                    && candidates == 1,
                "two-attempt automatic target did not complete"
            );
            let next = retained_manifest(engine, &terminal)?;
            let WorkerOperation::Generate { settings, .. } = &next.operation else {
                anyhow::bail!("target generation missing");
            };
            ensure!(
                encode(
                    settings
                        .inference_profile
                        .as_ref()
                        .context("target profile")?
                )? == encode(&input.to)?,
                "target did not use exact equivalent profile"
            );
            let native = json_file(&next.output_dir.join("receipt.json"))?;
            ensure!(
                native["qualification"] == "unqualified"
                    && native["inference_execution"]["provider"] == "cpu"
                    && native["inference_execution"]["successful_runs"]
                        .as_u64()
                        .is_some_and(|n| n > 0),
                "actual CPU target execution receipt absent"
            );
            ensure!(
                native["generation_progress"]["reused_analysed_samples"].as_u64() == Some(0),
                "fallback reused incompatible predecessor progress"
            );
            if let Some(pack) = selected_pack.as_ref() {
                pack_verification = Some(verify_native_selected_pack_success(
                    engine, &input, pack, &manifest, &next, &native, &terminal,
                )?);
            }
            Some(json!({"manifest":next,"native_receipt":native,"actual_cpu_execution":true}))
        } else if active_mode(mode) {
            None
        } else {
            ensure!(
                attempts == 1 && candidates == 0 && terminal.candidate_id.is_none(),
                "retired authority produced another attempt or candidate"
            );
            None
        };
        if matches!(mode, "pack_withdraw_pending" | "pack_withdraw_active") {
            pack_verification = Some(verify_native_selected_pack_withdrawal(
                engine,
                &input,
                selected_pack.as_ref().context("selected pack absent")?,
                &manifest,
                active_successor.as_ref(),
                &terminal,
            )?);
        }
        let selected_pack = if let Some(pack) = selected_pack.take() {
            let result = json!({"metadata":pack.metadata.clone(),"admission":pack_admission.context("selected pack admission absent")?,"withdrawal":pack_withdrawal,"verification":pack_verification.context("selected pack verification absent")?,"supplier_removed":true});
            pack.close()?;
            Some(result)
        } else {
            None
        };
        let after = snapshot(engine, &input.actor, &input.project)?;
        ensure!(
            after.revision == input.original.revision
                && encode(&after.motion)? == encode(&input.original.motion)?,
            "simulation altered master"
        );
        let archival = if archive {
            Some(archive_native_fallback(engine, &input.actor, &input.project)?)
        } else {
            None
        };
        Ok(
            json!({"case":mode,"passed":true,"simulated_eligibility":true,"primary_provider_absence_simulated":true,"real_measured_equivalence":false,"qualified":false,
        "primary_simulator":file_identity(&simulator)?,"primary_attempt":manifest.attempt_id,"primary_profile":input.from,
        "original_custody_dropped_before_successor":no_predecessor_custody,"opaque_failure_from_real_supervisor":true,"pending_marked":pending,
        "continuation_error":continuation_error,"terminal":terminal,"history":history,"target":target,"active_consumer":active_consumer,"selected_pack":selected_pack,
        "attempt_count":attempts,"candidate_count":candidates,"pending_rows":pending_rows,"master_preserved":true,"custody_clear":true,
        "public_synthetic_refusal":synthetic_refusal,"public_acceptance_called":false,"accepted_fixture_records_exported":archive,"archival":archival}),
        )
    }

    fn campaign_case(mode: &str, archive: bool) -> Result<Value> {
        let directory = tempfile::tempdir()?;
        let mut config = EngineConfig::new(
            directory.path().join("state"),
            environment_path("PULSAR_FALLBACK_NATIVE_WORKER")?,
        );
        config.cpu_owner_policy = Some(environment_path("PULSAR_FALLBACK_NATIVE_POLICY")?);
        config.worker_timeout = Duration::from_secs(60);
        let engine = Engine::open(config)?;
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_case(&engine, mode, archive)));
        let shutdown = engine.finish_shutdown();
        let custody = await_custody_clear(&engine, true);
        drop(engine);
        let removed = directory.close();
        shutdown?;
        custody?;
        removed?;
        let mut value = match result {
            Ok(result) => result?,
            Err(error) => anyhow::bail!(
                "native simulation panicked: {}",
                error
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| error.downcast_ref::<&str>().copied())
                    .unwrap_or("nontext panic")
            ),
        };
        value["shutdown_joined"] = json!(true);
        value["private_fixture_database_removed"] = json!(true);
        Ok(value)
    }

    #[test]
    #[ignore = "requires pinned worker, private CPU closure policy, simulator, delegated cgroups and owned host supervisor; SOFTWARE SIMULATION only"]
    fn native_simulated_eligibility_drives_real_cpu_successor_and_refuses_retired_authority() {
        let output =
            environment_path("PULSAR_FALLBACK_NATIVE_OUTPUT").expect("explicit native output");
        let mut receipt = json!({"schema_version":1,"passed":false,"simulated_eligibility":true,"primary_provider_absence_simulated":true,"real_measured_equivalence":false,
        "qualified":false,"physical_device":false,"public_acceptance_called":false,"accepted_fixture_records_exported":false,
        "scope":"SOFTWARE SIMULATION: privately authored accepted-shaped temporary eligibility and framed primary failure; actual target CPU execution/custody only. No actual provider absence, scientific equivalence, reviewer, operator acceptance or release qualification.","cases":[],"error":null});
        let result = (|| -> Result<()> {
            ensure!(
                file_identity(Path::new(MEDIA))?.sha256 == MEDIA_SHA
                    && file_identity(Path::new(MODEL))?.sha256 == MODEL_SHA,
                "authored neutral inputs changed"
            );
            for mode in [
                "success",
                "cancel",
                "withdraw",
                "expiry",
                "active_cancel",
                "active_withdraw",
                "active_expiry",
                "active_dossier_expiry",
                "pack_success",
                "pack_withdraw_pending",
                "pack_withdraw_active",
            ] {
                receipt["cases"]
                    .as_array_mut()
                    .unwrap()
                    .push(campaign_case(mode, false)?);
            }
            Ok(())
        })();
        if let Err(error) = &result {
            receipt["error"] = json!(format!("{error:#}"));
        } else {
            receipt["passed"] = json!(true);
        }
        save(&output.join("receipt.json"), &receipt).expect("save bounded simulation receipt");
        result.expect("native SOFTWARE SIMULATION campaign failed");
    }

    include!("engine_inference_fallback_archive_native_tests.rs");
}
