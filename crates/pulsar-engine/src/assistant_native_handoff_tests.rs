//! Native model contention and tracking tiers. No physical playback or model quality gate.
use super::*;
use std::{collections::BTreeSet, io::Read, time::Instant};

fn identity(path: &Path) -> ArtifactIdentity {
    let mut file = std::fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut block = [0u8; 65536];
    loop {
        let n = file.read(&mut block).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&block[..n]);
    }
    ArtifactIdentity {
        artifact_id: ArtifactId::new("pinned-native-handoff").unwrap(),
        sha256: format!("{:x}", hash.finalize()),
        byte_len: file.metadata().unwrap().len(),
    }
}
fn env_path(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("explicit native asset required"));
    assert!(path.is_absolute());
    path
}

#[test]
#[ignore = "owned delegated cgroups, real YOLO CPU generation and pinned learned assistant; run explicitly serially"]
fn native_generation_and_assistant_handoff_resumes_same_job_without_status_poll(
) -> anyhow::Result<()> {
    native_generation_assistant_handoff(false)
}
#[test]
#[ignore = "actual pinned YOLO/assistant and delegated cgroups; CPU-only contention with ample RAM; serial"]
fn native_cpu_quota_handoff_resumes_generation_with_ample_memory() -> anyhow::Result<()> {
    native_generation_assistant_handoff(true)
}
fn native_generation_assistant_handoff(cpu_only: bool) -> anyhow::Result<()> {
    let worker = env_path("PULSAR_TEST_ASSISTANT_WORKER");
    let runtime = env_path("PULSAR_ASSISTANT_RUNTIME");
    let model = env_path("PULSAR_ASSISTANT_MODEL");
    let pack = env_path("PULSAR_NATIVE_HANDOFF_PACK");
    let video = env_path("PULSAR_NATIVE_HANDOFF_VIDEO");
    let runtime_identity = identity(&runtime);
    let model_identity = identity(&model);
    assert_eq!(
        runtime_identity.sha256,
        "f332257812cf358954a1701c9fb205e5ac50b4fbdf0e2c09944cc25ea58459ae"
    );
    assert_eq!(
        model_identity.sha256,
        "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031"
    );
    let generation_profile: DeclaredInferenceProfile =
        serde_json::from_slice(&std::fs::read(pack.join("profile.json"))?)?;
    assert_eq!(
        generation_profile.model.sha256,
        "36849cda776713a29472569fc5955b6015994f1b2f0063ac53427c9b8340fb74"
    );
    let generation_model = pack.join("model.onnx");
    assert_eq!(
        identity(&generation_model).sha256,
        generation_profile.model.sha256
    );
    let f = Fixture::with_worker_config(worker, |config| {
        config.max_ram_bytes = if cpu_only { 8 << 30 } else { 3 << 30 };
        config.max_cpu_threads = 4;
    });
    let assistant_claim = (2u64 << 30)
        + runtime_identity.byte_len
        + model_identity.byte_len
        + f.engine.binary_identity.byte_len
        + (2 << 20);
    let mut generation_job = None;
    let mut assistant_attempt = None;
    let result = (|| -> anyhow::Result<()> {
        let profile = f.engine.register_assistant_profile(
            crate::AssistantLaunchAssets {
                vision: None,
                runtime_path: runtime,
                model_path: model,
                worker_memory_bytes: 2 * 1024 * 1024 * 1024,
            },
            runtime_identity,
            model_identity,
            4,
            45000,
        )?;
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(f.request(
                Command::ImportSource {
                    path: video.clone(),
                },
                None,
            ))
            .result?
        else {
            anyhow::bail!("source required");
        };
        let before = project_snapshot(&f.engine.db.lock().unwrap(), &f.project)?;
        let settings = GenerationSettings {
            analysis_width: 640,
            analysis_height: 640,
            model_input: Some(generation_profile.preprocessing.input.clone()),
            inference_profile: Some(generation_profile),
            ..Default::default()
        };
        let ResponseBody::Job(original) = f
            .engine
            .handle(f.request(
                Command::Generate {
                    source_version,
                    preset: "fast".into(),
                    settings: Some(settings),
                    model_path: Some(generation_model),
                },
                Some(0),
            ))
            .result?
        else {
            anyhow::bail!("job required");
        };
        generation_job = Some(original.job_id.clone());
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let current = job_snapshot(&f.engine.db.lock().unwrap(), &original.job_id, &f.project)?;
            anyhow::ensure!(
                current.error.is_none() && current.candidate_id.is_none(),
                "generation completed or failed before handoff: {current:?}"
            );
            if current.state == "running" && f.engine.worker_fence.has_pending_worker_custody()? {
                break;
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "native generation custody deadline"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let original_manifest: WorkerRequest = {
            let db = f.engine.db.lock().unwrap();
            let raw: String = db.query_row(
                "SELECT manifest FROM jobs WHERE id=?1",
                [original.job_id.as_str()],
                |r| r.get(0),
            )?;
            decode(&raw)?
        };
        let progress_deadline = Instant::now() + Duration::from_secs(60);
        let completed_progress = loop {
            if let Some(binding) = crate::generation_progress::latest(&original_manifest)? {
                if binding.record.analysed_samples >= 32 {
                    break binding;
                }
            }
            anyhow::ensure!(
                Instant::now() < progress_deadline,
                "completed generation inference checkpoint deadline"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        anyhow::ensure!(
            (WORKER_MEMORY + assistant_claim > f.engine.config.max_ram_bytes) != cpu_only,
            "RAM claims do not match the intended contention scope"
        );
        anyhow::ensure!(
            original_manifest.budget.cpu_threads + 4 > f.engine.config.max_cpu_threads,
            "CPU claims unexpectedly fit"
        );
        let mut observed_cpu_claims = std::collections::BTreeSet::new();
        let ResponseBody::Assistant(accepted) = f
            .engine
            .handle(f.request(
                Command::StartAssistant {
                    spec: AssistantStart {
                        image: None,
                        creative_patterns: None,
                        profile: profile.clone(),
                        user_request: "Please set generation sample stride to 8.".into(),
                        review_focus: vec![],
                        target_focus: vec![],
                        edit_focus: vec![],
                    },
                },
                Some(0),
            ))
            .result?
        else {
            anyhow::bail!("assistant acceptance required");
        };
        assistant_attempt = Some(accepted.attempt_id.clone());
        let deadline = Instant::now() + Duration::from_secs(240);
        let (completed, resumed) = loop {
            // Deliberately no AssistantStatus/List/ResumeJob call: completion must
            // arrange recovery itself. These are passive private journal reads.
            let cpu_claim = f.engine.pool.reserved_cpu_threads()?;
            anyhow::ensure!(
                cpu_claim <= u32::from(f.engine.config.max_cpu_threads),
                "aggregate CPU quota exceeded"
            );
            observed_cpu_claims.insert(cpu_claim);
            let db = f.engine.db.lock().unwrap();
            let report = super::read(&db, &accepted.attempt_id)?;
            let current = job_snapshot(&db, &original.job_id, &f.project)?;
            drop(db);
            if report.state != AssistantState::Running {
                anyhow::ensure!(
                    report.state == AssistantState::Completed,
                    "assistant failed: {report:?}"
                );
                if current.attempt_id != original.attempt_id && current.candidate_id.is_some() {
                    break (report, current);
                }
            }
            anyhow::ensure!(
                current.error.is_none() || current.state == "interrupted",
                "generation failed: {current:?}"
            );
            anyhow::ensure!(
                Instant::now() < deadline,
                "automatic native resume/completion deadline"
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        anyhow::ensure!(
            completed.proposal == Some(AssistantProposal::SetSampleStride { stride: 8 }),
            "learned proposal differs"
        );
        anyhow::ensure!(
            observed_cpu_claims.contains(&4),
            "assistant CPU claim not observed"
        );
        let db = f.engine.db.lock().unwrap();
        let paused: String = db.query_row(
            "SELECT state FROM attempts WHERE id=?1",
            [original.attempt_id.as_str()],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            paused == "assistant_paused",
            "original attempt was not intentionally paused: {paused}"
        );
        let raw: String = db.query_row(
            "SELECT manifest FROM jobs WHERE id=?1",
            [original.job_id.as_str()],
            |r| r.get(0),
        )?;
        let resumed_manifest: WorkerRequest = decode(&raw)?;
        f.engine.validate_checkpoint(&original_manifest)?;
        f.engine.validate_checkpoint(&resumed_manifest)?;
        let basis = |manifest: &WorkerRequest| -> anyhow::Result<serde_json::Value> {
            let mut value = serde_json::to_value(manifest)?;
            let object = value
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("manifest object required"))?;
            object.remove("attempt_id");
            object.remove("output_dir");
            Ok(value)
        };
        anyhow::ensure!(
            basis(&resumed_manifest)? == basis(&original_manifest)?,
            "immutable checkpoint input basis changed"
        );
        anyhow::ensure!(
            encode(&resumed_manifest.operation)? == encode(&original_manifest.operation)?,
            "model or operation changed"
        );
        let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(
            resumed_manifest.output_dir.join("receipt.json"),
        )?)?;
        anyhow::ensure!(
            receipt["generation_progress"]["reused_analysed_samples"]
                .as_u64()
                .is_some_and(|n| n >= completed_progress.record.analysed_samples),
            "completed inference was not reused"
        );
        let after = project_snapshot(&db, &f.project)?;
        anyhow::ensure!(
            after.revision == before.revision && after.motion == before.motion,
            "handoff changed master motion"
        );
        drop(db);
        let reference_dir = tempfile::tempdir_in(&f.engine.config.state_dir)?;
        let mut reference_manifest = original_manifest.clone();
        reference_manifest.attempt_id = AttemptId::new("uninterrupted-native-progress-reference")?;
        reference_manifest.output_dir = reference_dir.path().to_path_buf();
        let reference = crate::launch::execute_with_metrics(
            &f.engine.config,
            &reference_manifest,
            &AtomicBool::new(false),
        )?;
        let WorkerOutput::Generated {
            program: reference_program,
            receipt: reference_receipt,
            ..
        } = reference.output
        else {
            anyhow::bail!("uninterrupted generation output required");
        };
        let resumed_program: MotionProgram = serde_json::from_slice(&std::fs::read(
            resumed_manifest.output_dir.join("program.json"),
        )?)?;
        let reference_program: MotionProgram =
            serde_json::from_slice(&std::fs::read(reference_program.path)?)?;
        anyhow::ensure!(
            resumed_program == reference_program,
            "resumed motion differs from uninterrupted generation"
        );
        let reference_receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(reference_receipt.path)?)?;
        let resumed_runs = receipt["inference_execution"]["successful_runs"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("resumed CPU inference counter required"))?;
        let reference_runs = reference_receipt["inference_execution"]["successful_runs"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("uninterrupted CPU inference counter required"))?;
        let reused = receipt["generation_progress"]["reused_analysed_samples"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("retained analysis counter required"))?;
        anyhow::ensure!(
            resumed_runs < reference_runs
                && resumed_runs.checked_add(reused) == Some(reference_runs),
            "inference counts do not confirm retained computation reuse"
        );
        let normalize = |samples: &serde_json::Value,
                         attempt: &AttemptId|
         -> anyhow::Result<serde_json::Value> {
            let prefix = format!("attempt.{:x}", Sha256::digest(attempt.as_str().as_bytes()));
            let mut samples = samples.clone();
            for sample in samples
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("sample array required"))?
            {
                if let Some(id) = sample["selected_track"].as_str() {
                    anyhow::ensure!(
                        id.starts_with(&prefix),
                        "sample identity has wrong namespace"
                    );
                    sample["selected_track"] =
                        serde_json::json!(id.replacen(&prefix, "attempt", 1));
                }
            }
            Ok(samples)
        };
        anyhow::ensure!(
            normalize(&receipt["samples"], &resumed_manifest.attempt_id)?
                == normalize(
                    &reference_receipt["samples"],
                    &reference_manifest.attempt_id
                )?,
            "resumed lineage differs from uninterrupted generation"
        );
        anyhow::ensure!(
            receipt["review"] == reference_receipt["review"],
            "resumed reviews differ from uninterrupted generation"
        );
        println!(
            "PULSAR_NATIVE_HANDOFF_RESULT={}",
            serde_json::json!({"qualified":false,"job":resumed,"original_attempt":original.attempt_id,"paused_attempt_state":paused,"assistant":completed,"completed_progress_before_pause":completed_progress.record,"generation_progress":receipt["generation_progress"],"same_job":true,"same_input_basis":true,"both_exact_checkpoints_valid":true,"same_generation_operation":true,"no_assistant_status_or_manual_resume":true,"master_unchanged":true,"resumed_successful_inferences":resumed_runs,"uninterrupted_successful_inferences":reference_runs,"uninterrupted_motion_reviews_normalized_lineage_equal":true,"generation_profile":original_manifest.operation,"cpu_only_contention":cpu_only,"observed_cpu_claims":observed_cpu_claims,"cpu_ceiling":f.engine.config.max_cpu_threads,"assistant_aggregate_memory_claim":assistant_claim,"limits":{"pool_ram_bytes":f.engine.config.max_ram_bytes,"generation_worker_claim":WORKER_MEMORY,"assistant_worker_claim":2u64*1024*1024*1024},"limitations":["Authored media and generic learned detector; no motion quality acceptance","Decoder prefix is replayed and discarded; no codec seek checkpoint","No actual live device, GPU or physical playback continuity"]})
        );
        Ok(())
    })();
    if let Some(attempt) = assistant_attempt {
        let _ = f.engine.handle(f.request(
            Command::CancelAssistant {
                attempt_id: attempt,
            },
            None,
        ));
    }
    if let Some(job) = generation_job {
        let _ = f
            .engine
            .handle(f.request(Command::CancelJob { job_id: job }, None));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f.engine.workers.lock().unwrap().is_empty()
        || f.engine.worker_fence.has_pending_worker_custody()?
    {
        if Instant::now() >= deadline {
            let Fixture { _dir, .. } = f;
            let retained = _dir.keep();
            anyhow::bail!(
                "native custody retirement unconfirmed; retained owned state {}",
                retained.display()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    anyhow::ensure!(
        f.engine.pool.reserved_cpu_threads()? == 0,
        "CPU claims retained after retirement"
    );
    result
}

#[test]
#[ignore = "requires actual pinned detector/runtime and owned delegated cgroups; explicitly serial"]
fn native_visual_tiers_execute_pinned_detector_and_record_effective_plans() -> anyhow::Result<()> {
    native_visual_tiers(InferenceProvider::Cpu)
}

#[test]
#[cfg(feature = "cuda")]
#[ignore = "requires pinned learned CUDA detector/runtime, owner policy and delegated cgroups; explicitly serial"]
fn native_cuda_visual_tiers_execute_pinned_detector_and_bind_original_trace() -> anyhow::Result<()>
{
    native_visual_tiers(InferenceProvider::Cuda)
}

fn native_visual_tiers(provider: InferenceProvider) -> anyhow::Result<()> {
    let selected_tier = if provider == InferenceProvider::Cuda {
        std::env::var("PULSAR_NATIVE_CUDA_TIER").ok()
    } else {
        None
    };
    anyhow::ensure!(
        selected_tier
            .as_deref()
            .is_none_or(|tier| matches!(tier, "fast" | "default" | "maximum")),
        "unknown native CUDA tier"
    );
    let pack = env_path("PULSAR_NATIVE_HANDOFF_PACK");
    let profile: DeclaredInferenceProfile =
        serde_json::from_slice(&std::fs::read(pack.join("profile.json"))?)?;
    let model = pack.join("model.onnx");
    anyhow::ensure!(
        identity(&model).sha256
            == "36849cda776713a29472569fc5955b6015994f1b2f0063ac53427c9b8340fb74"
    );
    anyhow::ensure!(
        profile.provider == provider,
        "native profile provider differs"
    );
    let f = Fixture::with_worker_config(env_path("PULSAR_TEST_ASSISTANT_WORKER"), |config| {
        config.max_ram_bytes = 4 * 1024 * 1024 * 1024;
        if provider == InferenceProvider::Cuda {
            config.cuda_owner_policy = Some(env_path("PULSAR_CUDA_OWNER_POLICY"));
            config.worker_timeout = Duration::from_secs(600);
        }
    });
    let mut jobs = Vec::new();
    let outcome = (|| -> anyhow::Result<()> {
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(f.request(
                Command::ImportSource {
                    path: env_path("PULSAR_NATIVE_HANDOFF_VIDEO"),
                },
                None,
            ))
            .result?
        else {
            anyhow::bail!("source required")
        };
        let before = project_snapshot(&f.engine.db.lock().unwrap(), &f.project)?;
        let mut reports = Vec::new();
        for (preset, plan_id) in [
            ("fast", "visual-flow-fast-v1"),
            ("default", "visual-flow-default-v4"),
            ("maximum", "visual-flow-maximum-v2"),
        ] {
            if selected_tier
                .as_deref()
                .is_some_and(|selected| selected != preset)
            {
                continue;
            }
            let settings = GenerationSettings {
                analysis_width: 640,
                analysis_height: 640,
                model_input: Some(profile.preprocessing.input.clone()),
                inference_profile: Some(profile.clone()),
                ..Default::default()
            };
            let ResponseBody::Job(job) = f
                .engine
                .handle(f.request(
                    Command::Generate {
                        source_version: source_version.clone(),
                        preset: preset.into(),
                        settings: Some(settings),
                        model_path: Some(model.clone()),
                    },
                    Some(0),
                ))
                .result?
            else {
                anyhow::bail!("job required")
            };
            jobs.push(job.job_id.clone());
            let start = Instant::now();
            let done = loop {
                let current = job_snapshot(&f.engine.db.lock().unwrap(), &job.job_id, &f.project)?;
                anyhow::ensure!(current.error.is_none(), "native tier failed: {current:?}");
                if current.candidate_id.is_some() {
                    break current;
                }
                anyhow::ensure!(
                    start.elapsed() < Duration::from_secs(180),
                    "native tier deadline"
                );
                std::thread::sleep(Duration::from_millis(50));
            };
            let manifest: WorkerRequest = {
                let db = f.engine.db.lock().unwrap();
                let raw: String = db.query_row(
                    "SELECT manifest FROM jobs WHERE id=?1",
                    [job.job_id.as_str()],
                    |r| r.get(0),
                )?;
                decode(&raw)?
            };
            anyhow::ensure!(
                manifest.budget.memory_bytes == (1536_u64 << 20).max(profile.minimum_memory_bytes),
                "native manifest RAM differs from the declared profile claim"
            );
            let receipt: serde_json::Value =
                serde_json::from_slice(&std::fs::read(manifest.output_dir.join("receipt.json"))?)?;
            let receipt_identity = identity(&manifest.output_dir.join("receipt.json"));
            let lineage: Vec<ArtifactIdentity> = {
                let db = f.engine.db.lock().unwrap();
                let raw: String = db.query_row(
                    "SELECT lineage FROM candidates WHERE id=?1",
                    [done.candidate_id.as_ref().unwrap().as_str()],
                    |r| r.get(0),
                )?;
                decode(&raw)?
            };
            anyhow::ensure!(
                lineage
                    .iter()
                    .filter(|id| id.sha256 == receipt_identity.sha256
                        && id.byte_len == receipt_identity.byte_len)
                    .count()
                    == 1,
                "receipt not bound to published candidate"
            );
            anyhow::ensure!(receipt["visual_tracking_plan"]["id"] == plan_id);
            anyhow::ensure!(receipt["preset"] == preset);
            anyhow::ensure!(
                receipt["inference_execution"]["successful_runs"]
                    .as_u64()
                    .is_some_and(|n| n > 0),
                "no executed inference"
            );
            anyhow::ensure!(
                receipt["inference_execution"]["provider"] == serde_json::to_value(provider)?
            );
            let cuda_evidence = if provider == InferenceProvider::Cuda {
                let raw: String = f.engine.db.lock().unwrap().query_row(
                    "SELECT artifacts FROM cuda_candidate_evidence WHERE attempt=?1",
                    [manifest.attempt_id.as_str()],
                    |r| r.get(0),
                )?;
                let evidence: WorkerCudaArtifacts = decode(&raw)?;
                crate::cuda_result::verify_files(&manifest, &evidence)?;
                if let Some(root) = std::env::var_os("PULSAR_NATIVE_TIER_EVIDENCE") {
                    let root = PathBuf::from(root).join(preset);
                    anyhow::ensure!(!root.exists(), "evidence destination already exists");
                    crate::artifacts::private_directory(&root)?;
                    for (source, name) in [
                        (&evidence.receipt.path, "cuda-inference.json"),
                        (&evidence.trace.path, "cuda-trace.json"),
                    ] {
                        std::fs::copy(source, root.join(name))?;
                    }
                    std::fs::write(
                        root.join("request.json"),
                        serde_json::to_vec_pretty(&manifest)?,
                    )?;
                    std::fs::copy(
                        manifest.output_dir.join("receipt.json"),
                        root.join("receipt.json"),
                    )?;
                    std::fs::write(
                        root.join("candidate-binding.json"),
                        serde_json::to_vec_pretty(&serde_json::json!({
                            "attempt": manifest.attempt_id,
                            "candidate": done.candidate_id,
                            "state": done.state,
                            "cuda_evidence": evidence,
                            "candidate_lineage": lineage,
                        }))?,
                    )?;
                }
                Some(evidence)
            } else {
                None
            };
            anyhow::ensure!(receipt["qualification"] == "unqualified");
            let samples = receipt["samples"]
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("sample lineage required"))?;
            anyhow::ensure!(
                samples
                    .iter()
                    .any(|sample| sample["observed_boxes"].as_u64().is_some_and(|n| n > 0)),
                "source does not exercise nonempty detector observations"
            );
            let (points, level, bound) = match preset {
                "fast" => (8, 0, 8.5),
                "default" => (16, 2, 26.0),
                _ => (64, 2, 34.0),
            };
            anyhow::ensure!(
                samples.iter().any(|sample| sample["target_sampling_points"]
                    .as_u64()
                    .is_some_and(|n| n >= points && n <= 64)
                    && sample["flow_search"][0]["maximum_level"] == level
                    && sample["flow_search"][0]["component_displacement_bound"] == bound),
                "source does not exercise the effective flow search/grid"
            );

            reports.push(serde_json::json!({"preset":preset,"job":done,"elapsed_seconds":start.elapsed().as_secs_f64(),"receipt":receipt,"cuda_evidence":cuda_evidence}));
        }
        let after = project_snapshot(&f.engine.db.lock().unwrap(), &f.project)?;
        anyhow::ensure!(
            before.motion == after.motion && before.revision == after.revision,
            "tiers changed master"
        );
        println!(
            "PULSAR_NATIVE_VISUAL_TIERS_RESULT={}",
            serde_json::json!({"qualified":false,"provider":provider,"selected_tier":selected_tier,"reports":reports,"master_unchanged":true,"limitations":["Authored media does not qualify target motion or quality","Reported plans describe bounded work; not necessarily every search branch executed","No default-model selection, speed, whole-application memory, other vendor or release acceptance"]})
        );
        Ok(())
    })();
    for job_id in jobs {
        let _ = f
            .engine
            .handle(f.request(Command::CancelJob { job_id }, None));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f.engine.workers.lock().unwrap().is_empty()
        || f.engine.worker_fence.has_pending_worker_custody()?
    {
        if Instant::now() >= deadline {
            let Fixture { _dir, .. } = f;
            let kept = _dir.keep();
            anyhow::bail!(
                "tier custody retirement unconfirmed; retained {}",
                kept.display()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    outcome
}

#[test]
#[ignore = "actual pinned media/vision runtimes and delegated cgroups; authored still controls, not useful model quality"]
fn native_authenticated_prepared_image_start_retains_source_and_rejects_stale_pixels(
) -> anyhow::Result<()> {
    let worker = env_path("PULSAR_TEST_ASSISTANT_WORKER");
    let runtime = env_path("PULSAR_VISION_RUNTIME");
    let model = env_path("PULSAR_VISION_MODEL");
    let projector = env_path("PULSAR_VISION_PROJECTOR");
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
    let owner = tempfile::tempdir()?;
    let policy_path = owner.path().join("vision-policy.json");
    let mut setup_config =
        EngineConfig::new(owner.path().join("never-opened-state"), worker.clone());
    setup_config.worker_timeout = std::time::Duration::from_secs(150);
    crate::create_assistant_policy(
        &setup_config,
        crate::AssistantPolicySetup {
            output: policy_path.clone(),
            runtime,
            model,
            projector: Some(projector),
            cpu_threads: 2,
            wall_time_ms: 120000,
            worker_memory_bytes: 2u64 << 30,
            absolute_assets: true,
            approve_host_inference: true,
        },
    )?;
    assert!(!setup_config.state_dir.exists());
    let f = Fixture::with_worker_config(worker, |config| {
        config.max_ram_bytes = 4u64 << 30;
        config.worker_timeout = std::time::Duration::from_secs(150);
        config.assistant_owner_policy = Some(policy_path);
    });
    let ResponseBody::AssistantProfiles(mut profiles) = f
        .engine
        .handle(f.request(Command::AssistantProfiles, None))
        .result?
    else {
        anyhow::bail!("profiles required");
    };
    assert_eq!(profiles.len(), 1);
    let profile = profiles.remove(0);
    assert_eq!(
        profile.vision_projector.as_ref().unwrap().sha256,
        "0802360aca1748f112ea510b8ff277c65b1361c8ef30ed89b83c9c7a60d08e96"
    );
    let mut pixels = Vec::with_capacity(384 * 384 * 3);
    for y in 0i32..384 {
        for x in 0i32..384 {
            pixels.extend_from_slice(if (x - 192).pow(2) + (y - 192).pow(2) <= 112 * 112 {
                &[255, 0, 0]
            } else {
                &[255, 255, 255]
            });
        }
    }
    let mut ppm = b"P6\n384 384\n255\n".to_vec();
    ppm.extend_from_slice(&pixels);
    let path = f._dir.path().join("authored-circle.ppm");
    std::fs::write(&path, &ppm)?;
    let source_sha = identity(&path).sha256;
    let ResponseBody::Source { source_version } = f
        .engine
        .handle(f.request(Command::ImportSource { path }, None))
        .result?
    else {
        anyhow::bail!("source response");
    };
    let before = project_snapshot(&f.engine.db.lock().unwrap(), &f.project)?;
    let request = f.request(Command::PrepareAssistantImage { source_version }, Some(0));
    let ResponseBody::AssistantImage(image) = f.engine.handle(request.clone()).result? else {
        anyhow::bail!("prepared image response");
    };
    image.validate_for(&request)?;
    assert_eq!(image.source.sha256, source_sha);
    assert_eq!((image.width, image.height), (384, 384));
    let pixels_path =
        f.engine
            .require_assistant_image(&f.engine.db.lock().unwrap(), &request, &image)?;
    assert_eq!(std::fs::read(&pixels_path)?, pixels);
    let creative = CreativePatternScope {
        axes: vec![Axis::Stroke],
        duration: ProjectTime::from_nanos(2_000_000_000),
        sample_hz: 8,
        max_amplitude: 0.1,
    };
    let spec = AssistantStart {
        image: Some((*image).clone()),
        creative_patterns: Some(creative),
        profile,
        edit_focus: vec![],
        target_focus: vec![],
        review_focus: vec![],
        user_request: "Suggest a sine stroke theme for this red circle.".into(),
    };
    let mut forged = spec.clone();
    forged.image.as_mut().unwrap().rgb.sha256 = "d".repeat(64);
    assert_eq!(
        f.engine
            .handle(f.request(Command::StartAssistant { spec: forged }, Some(0)))
            .result
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    for case in 0..3 {
        let mut wrong = request.clone();
        match case {
            0 => wrong.project = Some(ProjectId::new("other").unwrap()),
            1 => wrong.session = Some(SessionId::new("other").unwrap()),
            _ => wrong.expected_revision = Some(RevisionId::new(1)),
        }
        assert!(f
            .engine
            .require_assistant_image(&f.engine.db.lock().unwrap(), &wrong, &image)
            .is_err());
    }
    let ResponseBody::Assistant(accepted) = f
        .engine
        .handle(f.request(Command::StartAssistant { spec: spec.clone() }, Some(0)))
        .result?
    else {
        anyhow::bail!("assistant response");
    };
    let deadline = Instant::now() + std::time::Duration::from_secs(140);
    let completed = loop {
        let ResponseBody::Assistant(report) = f
            .engine
            .handle(f.request(
                Command::AssistantStatus {
                    attempt_id: accepted.attempt_id.clone(),
                },
                None,
            ))
            .result?
        else {
            anyhow::bail!("status response");
        };
        if report.state != AssistantState::Running {
            break report;
        }
        if Instant::now() >= deadline {
            f.engine
                .handle(f.request(
                    Command::CancelAssistant {
                        attempt_id: accepted.attempt_id.clone(),
                    },
                    None,
                ))
                .result?;
            anyhow::bail!("assistant completion deadline");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert_eq!(
        completed.state,
        AssistantState::Completed,
        "{:?}",
        completed.error
    );
    assert_eq!(completed.image.as_ref(), Some(image.as_ref()));
    completed.validate()?;
    assert_eq!(
        serde_json::to_value(project_snapshot(&f.engine.db.lock().unwrap(), &f.project)?)?,
        serde_json::to_value(before)?
    );
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get::<_, i64>(0))?,
        0
    );
    // The actual proposal is retained without promoting it into quality acceptance.
    eprintln!(
        "authenticated vision result: {}",
        serde_json::to_string(&completed.proposal)?
    );
    // A separate authored report fixture exercises approval independently of
    // the learned response. The actual learned report above remains unchanged.
    let mut approval_fixture = (*completed).clone();
    approval_fixture.attempt_id = id!(AttemptId);
    approval_fixture.request_id = id!(RequestId);
    approval_fixture.proposal = Some(AssistantProposal::SynthesizePatterns {
        patterns: vec![parse_pattern_prompt("pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8").unwrap()],
    });
    approval_fixture.validate()?;
    {
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "INSERT INTO assistant_attempts VALUES(?1,?2,?3,?4,?5)",
            params![
                approval_fixture.attempt_id.as_str(),
                approval_fixture.project_id.as_str(),
                approval_fixture.session_id.as_str(),
                approval_fixture.request_id.as_str(),
                encoded_report(&approval_fixture)?
            ],
        )?;
    }
    let approval_request = creative_generation(&f, &approval_fixture);
    let approved = f
        .engine
        .approved_assistant_generation(&f.engine.db.lock().unwrap(), &approval_request)?
        .expect("derived fixture patterns");
    assert!(matches!(
        approved.command,
        Command::GenerateInput {
            input: GenerationInput::Patterns { .. }
        }
    ));
    assert_eq!(approved.request_id, approval_request.request_id);
    // Separately authored proposal: exercise actual deterministic generation and
    // portable provenance without substituting it for learned quality evidence.
    let ResponseBody::Job(job) = f.engine.handle(approval_request.clone()).result? else {
        anyhow::bail!("image-conditioned generation job required");
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let completed_job = loop {
        let ResponseBody::Job(current) = f
            .engine
            .handle(f.request(
                Command::JobStatus {
                    job_id: job.job_id.clone(),
                },
                None,
            ))
            .result?
        else {
            anyhow::bail!("job status required")
        };
        if current.state == "completed" {
            break current;
        }
        anyhow::ensure!(
            matches!(current.state.as_str(), "queued" | "running"),
            "image generation failed: {:?}",
            current
        );
        anyhow::ensure!(Instant::now() < deadline, "image generation deadline");
        std::thread::sleep(Duration::from_millis(20));
    };
    let raw: String = f.engine.db.lock().unwrap().query_row(
        "SELECT manifest FROM jobs WHERE id=?1",
        [job.job_id.as_str()],
        |r| r.get(0),
    )?;
    let manifest: WorkerRequest = serde_json::from_str(&raw)?;
    let (origin, origin_bytes, retained_image) =
        assistant_image_origin::retained(&f.engine.config.state_dir, &manifest)?
            .expect("image origin retained");
    assert_eq!(retained_image, *image);
    let record: serde_json::Value = serde_json::from_slice(&origin_bytes)?;
    assert_eq!(
        record["approval_request"],
        approval_request.request_id.as_str()
    );
    assert_eq!(
        record["report"]["attempt_id"],
        approval_fixture.attempt_id.as_str()
    );
    let candidate = candidate_snapshot(
        &f.engine.db.lock().unwrap(),
        completed_job.candidate_id.as_ref().expect("candidate"),
        &f.project,
    )?;
    let inspect = || {
        f.engine.handle(f.request(
            Command::CandidateImageOriginStatus {
                candidate_id: candidate.candidate_id.clone(),
            },
            None,
        ))
    };
    let response = inspect();
    let mut frame = Vec::new();
    write_message(&mut frame, &response)?;
    let decoded: Response = read_message(&mut std::io::Cursor::new(&frame))?;
    let ResponseBody::CandidateImageOrigin(inspection) = decoded.result? else {
        anyhow::bail!("image inspection required");
    };
    inspection.validate()?;
    assert_eq!(inspection.origins.len(), 1);
    assert_eq!(inspection.origins[0].record, origin);
    assert_eq!(
        inspection.origins[0].approval.approval_request,
        approval_request.request_id
    );
    assert_eq!(
        inspection.origins[0].approval.report.attempt_id,
        approval_fixture.attempt_id
    );
    assert_eq!(inspection.motion, candidate.motion);
    assert!(
        !inspection.execution_authority
            && !inspection.observed_temporal_motion
            && !inspection.qualified
    );
    let mut changed = (*inspection).clone();
    changed.qualified = true;
    assert!(changed.validate().is_err());
    let mut changed = (*inspection).clone();
    changed.origins[0].approval.approval.proposal_sha256 = "0".repeat(64);
    assert!(changed.validate().is_err());
    let mut changed = (*inspection).clone();
    changed.origins[0].record.sha256 = "0".repeat(64);
    assert!(changed.validate().is_err());
    let mut changed = (*inspection).clone();
    changed.motion.program.sha256 = "0".repeat(64);
    assert!(changed.validate().is_err());
    let before_receipt = std::fs::read(manifest.output_dir.join("receipt.json"))?;
    let receipt_path = manifest.output_dir.join("receipt.json");
    let receipt_permissions = std::fs::metadata(&receipt_path)?.permissions();
    std::fs::set_permissions(&receipt_path, std::fs::Permissions::from_mode(0o600))?;
    std::fs::write(&receipt_path, b"[]")?;
    assert!(
        inspect().result.is_err(),
        "changed producer receipt must refuse inspection"
    );
    std::fs::write(&receipt_path, before_receipt)?;
    std::fs::set_permissions(&receipt_path, receipt_permissions)?;
    assert!(inspect().result.is_ok());
    let unknown = f.engine.handle(f.request(
        Command::CandidateImageOriginStatus {
            candidate_id: CandidateId::new("missing-image-candidate")?,
        },
        None,
    ));
    assert_eq!(unknown.result.unwrap_err().code, ErrorCode::NotFound);
    let ResponseBody::CandidateImageOriginPixels(retained_pixels) = f
        .engine
        .handle(f.request(
            Command::CandidateImageOriginPixels {
                candidate_id: candidate.candidate_id.clone(),
                record_sha256: origin.sha256.clone(),
            },
            None,
        ))
        .result?
    else {
        anyhow::bail!("retained image pixels");
    };
    retained_pixels.validate()?;
    assert_eq!(retained_pixels.frame.artifact.identity, image.rgb);
    assert_eq!(
        std::fs::read(&retained_pixels.frame.artifact.path)?,
        std::fs::read(
            f.engine
                .config
                .state_dir
                .join("snapshots")
                .join(&image.rgb.sha256)
        )?
    );
    let mut changed = (*retained_pixels).clone();
    changed.frame.width += 1;
    assert!(changed.validate().is_err());
    let mut changed = (*retained_pixels).clone();
    changed.frame.format = PixelFormat::Gray8;
    assert!(changed.validate().is_err());
    let mut changed = (*retained_pixels).clone();
    changed.record.sha256 = "0".repeat(64);
    assert!(changed.validate().is_err());
    assert!(f
        .engine
        .handle(f.request(
            Command::CandidateImageOriginPixels {
                candidate_id: candidate.candidate_id.clone(),
                record_sha256: "0".repeat(64)
            },
            None
        ))
        .result
        .is_err());
    candidate_image_origin::retain_pixel_observation(&retained_pixels);
    eprintln!(
        "local retained conditioning pixels: exact RGB and record/candidate/layout binding PASS"
    );
    eprintln!("local image origin inspection: authenticated scoped response/frame PASS; approval/record/motion/qualification mutation refusals PASS; changed producer receipt refusal PASS");
    let lineage: String = f.engine.db.lock().unwrap().query_row(
        "SELECT lineage FROM candidates WHERE id=?1",
        [candidate.candidate_id.as_str()],
        |r| r.get(0),
    )?;
    assert!(serde_json::from_str::<Vec<ArtifactIdentity>>(&lineage)?.contains(&origin));
    let plan = project_packages::capture(
        &f.engine,
        &f.session,
        &f.token,
        &f.project,
        RevisionId::new(0),
    )?;
    for identity in [&origin, &image.rgb] {
        assert!(plan
            .manifest
            .objects
            .iter()
            .any(|o| o.sha256 == identity.sha256
                && o.byte_len == identity.byte_len
                && o.roles.contains(&PackageObjectRole::DependencyEvidence)));
    }
    let archived_image = assistant_image_origin::archived(&manifest, |identity| {
        let descriptor = plan
            .manifest
            .objects
            .iter()
            .find(|o| o.sha256 == identity.sha256 && o.byte_len == identity.byte_len)
            .ok_or_else(|| ProtocolError::invalid("missing image object"))?;
        let mut bytes = vec![];
        plan.open_object(descriptor)?
            .read_to_end(&mut bytes)
            .map_err(internal)?;
        Ok(bytes)
    })?
    .expect("archived image origin");
    assert_eq!(archived_image, *image);
    let portable_image = plan.manifest.generated_origins[0]
        .image_origin
        .clone()
        .expect("typed image origin");
    assert_eq!(plan.manifest.format_version, 13);
    let mut portable_bytes = Vec::new();
    crate::project_package_format::write_package(
        &plan.manifest,
        &mut portable_bytes,
        |object| plan.open_object(object),
        crate::project_package_format::PackageCodecLimits {
            max_package_bytes: DEFAULT_MAX_PROJECT_PACKAGE_BYTES,
        },
    )?;
    drop(plan);
    eprintln!(
        "image provenance candidate: {}",
        serde_json::to_string(
            &serde_json::json!({"candidate":candidate.candidate_id,"origin":origin,"rgb":image.rgb,"qualified":false,"authored_approval_fixture":true})
        )?
    );
    use std::os::unix::fs::PermissionsExt;
    let source_path = f
        .engine
        .source(
            &f.engine.db.lock().unwrap(),
            &f.project,
            &image.source_version,
        )?
        .path;
    std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o600))?;
    std::fs::write(&source_path, vec![0u8; ppm.len()])?;
    assert!(f
        .engine
        .require_assistant_image(&f.engine.db.lock().unwrap(), &request, &image)
        .is_err());
    assert!(f
        .engine
        .approved_assistant_generation(&f.engine.db.lock().unwrap(), &approval_request)
        .is_err());
    std::fs::write(&source_path, &ppm)?;
    std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o400))?;
    std::fs::set_permissions(&pixels_path, std::fs::Permissions::from_mode(0o600))?;
    std::fs::write(&pixels_path, vec![0u8; pixels.len()])?;
    assert_eq!(
        f.engine
            .handle(f.request(Command::StartAssistant { spec }, Some(0)))
            .result
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    assert!(f
        .engine
        .approved_assistant_generation(&f.engine.db.lock().unwrap(), &approval_request)
        .is_err());
    assert!(!f.engine.worker_fence.has_pending_worker_custody()?);
    assert_eq!(f.engine.pool.reserved_cpu_threads()?, 0);
    // Own RAM-backed fixture suppliers are unavailable: clone import must use
    // exact exported evidence, never historical WorkerRequest host paths.
    let suppliers = BTreeSet::from([
        source_path,
        manifest.source.path.clone(),
        f.engine
            .config
            .state_dir
            .join("snapshots")
            .join(&image.rgb.sha256),
        f.engine
            .config
            .state_dir
            .join("snapshots")
            .join(&origin.sha256),
    ]);
    for path in suppliers {
        std::fs::rename(&path, path.with_extension("unavailable"))?;
        assert!(!path.exists());
    }
    project_package_imports::exercise_authored_image_portable_lifecycle(
        &portable_bytes,
        &portable_image,
    );
    eprintln!("image portable lifecycle: actual export/import/restart/re-export/second-clone PASS; independent record/RGB tamper startup refusals preserve DB/WAL");
    Ok(())
}

#[test]
#[ignore = "requires pinned actual pre-image-origin application and package probe; run explicitly serially"]
fn native_image_origin_preceding_reader_refuses_packages_and_state_without_changes(
) -> anyhow::Result<()> {
    for name in [
        "PULSAR_IMAGE_PRECEDING_READER",
        "PULSAR_IMAGE_PRECEDING_PACKAGE_PROBE",
    ] {
        let path = std::env::var_os(name).ok_or_else(|| anyhow::anyhow!("pin {name}"))?;
        let path = PathBuf::from(path);
        anyhow::ensure!(
            path.is_absolute() && std::fs::symlink_metadata(&path)?.is_file(),
            "preceding witness must be an actual regular executable"
        );
    }
    native_authenticated_prepared_image_start_retains_source_and_rejects_stale_pixels()
}
