use pulsar_engine::benchmark::admit_generation_profile;
use pulsar_protocol::*;

fn fixture() -> (DeclaredInferenceProfile, WorkerRequest) {
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new("model").unwrap(),
        sha256: "a".repeat(64),
        byte_len: 10,
    };
    let artifact = SourceArtifact {
        source_version: SourceVersionId::new("version").unwrap(),
        path: "/model".into(),
        identity: identity.clone(),
    };
    let input = ModelInputContract {
        width: 64,
        height: 64,
        channels: 3,
        layout: TensorLayout::Nchw,
        class_count: 1,
        decoder: DetectorDecoder::YoloChannelMajor,
    };
    let profile = DeclaredInferenceProfile {
        schema_version: 1,
        model: identity.clone(),
        runtime: identity.clone(),
        preprocessing: InferencePreprocessing {
            input: input.clone(),
            resize: InferenceResize::Stretch,
            color: InferenceColor::Rgb,
            scale: vec![1.0 / 255.0; 3],
            bias: vec![0.0; 3],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cpu,
        minimum_memory_bytes: 1024,
        minimum_output_bytes: 1024,
        cpu_threads: 1,
        cuda_resources: None,
    };
    let tool = ArtifactReference {
        identity,
        path: "/runtime".into(),
    };
    let request = WorkerRequest {
        cuda_execution: None,
        independent_audio: None,
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new("attempt").unwrap(),
        job_id: JobId::new("job").unwrap(),
        project_id: ProjectId::new("project").unwrap(),
        base_revision: RevisionId::new(0),
        source: artifact.clone(),
        output_dir: "/output".into(),
        operation: WorkerOperation::Generate {
            preset: "fast".into(),
            model: Some(artifact),
            settings: (GenerationSettings {
                inference_profile: Some(profile.clone()),
                model_input: Some(input),
                ..Default::default()
            })
            .into(),
        },
        budget: ResourceBudget {
            memory_bytes: 2048,
            output_bytes: 2048,
            wall_time_ms: 1000,
            cpu_threads: 1,
        },
        dependencies: vec![],
        tools: WorkerTools {
            ffmpeg: tool.clone(),
            ffprobe: tool.clone(),
            onnx_runtime: Some(tool),
        },
    };
    (profile, request)
}

#[test]
fn accepts_exact_cpu_contract_and_binds_serialized_profile() {
    let (p, r) = fixture();
    let admitted = admit_generation_profile(&p, &r).unwrap();
    assert_eq!(admitted.sha256.len(), 64);
    assert_eq!(
        admitted.sha256,
        admit_generation_profile(&p, &r).unwrap().sha256
    );
}

#[test]
fn rejects_backend_substitution_and_preprocessing_drift() {
    let (mut p, r) = fixture();
    p.provider = InferenceProvider::Cuda;
    assert!(admit_generation_profile(&p, &r).is_err());
    p.provider = InferenceProvider::Cpu;
    p.preprocessing.resize = InferenceResize::Letterbox;
    assert!(admit_generation_profile(&p, &r).is_err());
    p.preprocessing.resize = InferenceResize::Stretch;
    p.preprocessing.scale[0] = 1.0;
    assert!(admit_generation_profile(&p, &r).is_err());
}

#[test]
fn rejects_changed_bytes_thread_budget_and_missing_model() {
    let (p, mut r) = fixture();
    r.tools.onnx_runtime.as_mut().unwrap().identity.sha256 = "b".repeat(64);
    assert!(admit_generation_profile(&p, &r).is_err());
    let (p, mut r) = fixture();
    r.budget.cpu_threads = 2;
    assert!(admit_generation_profile(&p, &r).is_err());
    let (p, mut r) = fixture();
    if let WorkerOperation::Generate { model, .. } = &mut r.operation {
        *model = None;
    }
    assert!(admit_generation_profile(&p, &r).is_err());
}

#[test]
fn selected_profile_survives_manifest_serialization_and_is_not_replaced_by_default() {
    let (p, mut r) = fixture();
    if let WorkerOperation::Generate { settings, .. } = &mut r.operation {
        settings.inference_profile = Some(p.clone());
    }
    let encoded = serde_json::to_vec(&r).unwrap();
    let decoded: WorkerRequest = serde_json::from_slice(&encoded).unwrap();
    let selected = pulsar_engine::inference_profiles::resolve_profile(&decoded)
        .unwrap()
        .unwrap();
    assert_eq!(selected.minimum_memory_bytes, p.minimum_memory_bytes);
    let mut pinned = decoded;
    if let WorkerOperation::Generate { settings, .. } = &mut pinned.operation {
        settings.inference_profile.as_mut().unwrap().provider = InferenceProvider::Cuda;
    }
    assert!(pulsar_engine::inference_profiles::resolve_profile(&pinned).is_err());
}

#[test]
fn preview_enforces_selected_profile_and_rejects_profiles_without_model() {
    let (p, mut r) = fixture();
    let model = match &r.operation {
        WorkerOperation::Generate { model, .. } => model.clone(),
        _ => unreachable!(),
    };
    r.operation = WorkerOperation::Preview {
        still_image: false,
        annotated: None,
        placement_seek: None,
        time_basis: PreviewTimeBasis::SourcePts,
        video_view: None,
        context: FrameContext {
            source_version: r.source.source_version.clone(),
            source_placement: SourcePlacementId::new("source").unwrap(),
            frame: FrameId::new(0),
            transform: TransformId::new("source-identity").unwrap(),
            seek_generation: 0,
            request_generation: 0,
        },
        source_time: SourceTimestamp::new(0, 1).unwrap(),
        max_width: 64,
        max_height: 64,
        model,
        model_input: Some(p.preprocessing.input.clone()),
        inference_profile: Some(Into::into(p.clone())),
    };
    let decoded: WorkerRequest = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
    let selected = pulsar_engine::inference_profiles::resolve_profile(&decoded)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::to_value(selected).unwrap(),
        serde_json::to_value(p).unwrap()
    );
    if let WorkerOperation::Preview {
        inference_profile, ..
    } = &mut r.operation
    {
        inference_profile.as_mut().unwrap().provider = InferenceProvider::Cuda;
    }
    assert!(pulsar_engine::inference_profiles::resolve_profile(&r).is_err());
    if let WorkerOperation::Preview {
        inference_profile,
        model,
        model_input,
        ..
    } = &mut r.operation
    {
        inference_profile.as_mut().unwrap().provider = InferenceProvider::Cpu;
        *model = None;
        *model_input = None;
    }
    assert!(r.validate().is_err());
    assert!(pulsar_engine::inference_profiles::resolve_profile(&r).is_err());
}

#[test]
fn admission_matches_selected_letterbox_and_the_actual_implicit_default() {
    let (mut profile, mut request) = fixture();
    profile.preprocessing.resize = InferenceResize::Letterbox;
    profile.preprocessing.padding_byte = 114;
    assert!(admit_generation_profile(&profile, &request).is_err());
    if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
        settings.inference_profile = Some(profile.clone());
    }
    admit_generation_profile(&profile, &request).unwrap();
    if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
        settings.inference_profile = None;
    }
    request.budget.memory_bytes = 256 * 1024 * 1024;
    request.budget.output_bytes = 2 * 1024 * 1024;
    let implicit = pulsar_engine::inference_profiles::resolve_profile(&request)
        .unwrap()
        .unwrap();
    admit_generation_profile(&implicit, &request).unwrap();
    assert!(admit_generation_profile(&profile, &request).is_err());
    let mut drift = implicit.clone();
    drift.minimum_memory_bytes += 1;
    assert!(admit_generation_profile(&drift, &request).is_err());
}
