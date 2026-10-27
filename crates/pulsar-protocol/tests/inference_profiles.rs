use pulsar_protocol::*;

fn profile() -> DeclaredInferenceProfile {
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new("test-artifact").unwrap(),
        sha256: "a".repeat(64),
        byte_len: 12,
    };
    DeclaredInferenceProfile {
        schema_version: 1,
        model: identity.clone(),
        runtime: identity,
        preprocessing: InferencePreprocessing {
            input: ModelInputContract {
                width: 320,
                height: 180,
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
        minimum_memory_bytes: 4096,
        minimum_output_bytes: 1024,
        cpu_threads: 2,
        cuda_resources: None,
    }
}

#[test]
fn validates_explicit_contract_and_exact_budget() {
    let p = profile();
    p.validate().unwrap();
    let mut budget = ResourceBudget {
        memory_bytes: 4096,
        output_bytes: 1024,
        wall_time_ms: 1,
        cpu_threads: 2,
    };
    p.admit(&budget).unwrap();
    budget.memory_bytes -= 1;
    assert!(p.admit(&budget).is_err());
    budget.memory_bytes += 1;
    budget.cpu_threads = 1;
    assert!(p.admit(&budget).is_err());
}

#[test]
fn versioned_cuda_resources_do_not_change_legacy_cpu_serialization() {
    let cpu = profile();
    let bytes = serde_json::to_vec(&cpu).unwrap();
    assert!(!String::from_utf8(bytes.clone())
        .unwrap()
        .contains("cuda_resources"));
    let restored: DeclaredInferenceProfile = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_vec(&restored).unwrap(), bytes);
    let mut cuda = cpu.clone();
    cuda.schema_version = 2;
    cuda.provider = InferenceProvider::Cuda;
    cuda.cuda_resources = Some(CudaResourceLimits {
        device_ordinal: 0,
        device_uuid: "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f4351".into(),
        address_space_bytes: 16 * 1024u64.pow(3),
        device_reservation_bytes: 1024u64.pow(3),
        arena_limit_bytes: 64 * 1024u64.pow(2),
    });
    let budget = ResourceBudget {
        memory_bytes: 2 * 1024u64.pow(3),
        output_bytes: 1024,
        wall_time_ms: 1,
        cpu_threads: 2,
    };
    cuda.admit(&budget).unwrap();
    let encoded = serde_json::to_vec(&cuda).unwrap();
    let decoded: DeclaredInferenceProfile = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
    for (version, provider, resources) in [
        (1, InferenceProvider::Cuda, true),
        (2, InferenceProvider::Cpu, true),
        (2, InferenceProvider::Cuda, false),
        (2, InferenceProvider::TensorRt, true),
        (3, InferenceProvider::Cuda, true),
    ] {
        let mut bad = cuda.clone();
        bad.schema_version = version;
        bad.provider = provider;
        if !resources {
            bad.cuda_resources = None;
        }
        assert!(bad.validate().is_err());
    }
    for field in [
        "address_space_bytes",
        "device_reservation_bytes",
        "arena_limit_bytes",
    ] {
        let mut bad = serde_json::to_value(&cuda).unwrap();
        bad["cuda_resources"][field] = serde_json::json!(0);
        assert!(serde_json::from_value::<DeclaredInferenceProfile>(bad)
            .unwrap()
            .validate()
            .is_err());
    }
    for field in ["address_space_bytes", "device_reservation_bytes"] {
        let mut bad = serde_json::to_value(&cuda).unwrap();
        bad["cuda_resources"][field] = serde_json::json!((1u64 << 40) + 1);
        assert!(serde_json::from_value::<DeclaredInferenceProfile>(bad)
            .unwrap()
            .validate()
            .is_err());
    }
    for uuid in [
        "GPU-",
        "MIG-1790118a-a6d7-4eaf-fcac-dcacac5f4351",
        "GPU-1790118a-a6d7-4eaf-fcac-dcacac5f435z",
    ] {
        let mut bad = cuda.clone();
        bad.cuda_resources.as_mut().unwrap().device_uuid = uuid.into();
        assert!(bad.validate().is_err());
    }
    let mut bad = cuda.clone();
    bad.cuda_resources.as_mut().unwrap().device_ordinal = 64;
    assert!(bad.validate().is_err());
    let mut bad = cuda.clone();
    bad.cuda_resources.as_mut().unwrap().arena_limit_bytes = 2 * 1024u64.pow(3);
    assert!(bad.validate().is_err());
    let mut bad = cuda.clone();
    bad.cuda_resources.as_mut().unwrap().address_space_bytes = cuda.minimum_memory_bytes - 1;
    assert!(bad.validate().is_err());
    let mut bad = cuda;
    bad.cuda_resources.as_mut().unwrap().address_space_bytes = budget.memory_bytes - 1;
    assert!(bad.admit(&budget).is_err());
}

#[test]
fn rejects_ambiguous_preprocessing_and_unidentified_bytes() {
    let mut p = profile();
    p.preprocessing.scale[0] = f32::NAN;
    assert!(p.validate().is_err());
    p = profile();
    p.preprocessing.color = InferenceColor::Gray;
    assert!(p.validate().is_err());
    p = profile();
    p.model.sha256 = "unknown".into();
    assert!(p.validate().is_err());
    p = profile();
    p.preprocessing.bias.pop();
    assert!(p.validate().is_err());
    p = profile();
    p.model.sha256 = "A".repeat(64);
    assert!(p.validate().is_err());
}

#[test]
fn execution_requires_exact_attempt_profile_and_strict_provider() {
    let p = profile();
    let attempt = AttemptId::new("attempt-one").unwrap();
    let hash = "b".repeat(64);
    let mut evidence = InferenceExecutionEvidence {
        attempt_id: attempt.clone(),
        profile_sha256: hash.clone(),
        executed_providers: vec![InferenceProvider::Cpu],
        elapsed_ns: 1,
        peak_memory_bytes: None,
    };
    evidence.validate_for(&p, &attempt, &hash).unwrap();
    assert!(evidence
        .validate_for(&p, &AttemptId::new("attempt-two").unwrap(), &hash)
        .is_err());
    assert!(evidence
        .validate_for(&p, &attempt, &"c".repeat(64))
        .is_err());
    evidence.executed_providers.clear();
    assert!(evidence.validate_for(&p, &attempt, &hash).is_err());
    evidence.executed_providers = vec![InferenceProvider::Cuda, InferenceProvider::Cpu];
    assert!(evidence.validate_for(&p, &attempt, &hash).is_err());
}

#[test]
fn target_seed_contract_is_optional_checked_and_requires_indexed_detector_support() {
    let seed = AnalysisTargetSeed {
        source_frame_index: 2,
        x: 0.25,
        y: 0.5,
        class_id: Some(0),
    };
    let mut settings = GenerationSettings {
        target_seed: Some(seed.clone()),
        source_sample_stride: 2,
        model_input: Some(ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        }),
        ..Default::default()
    };
    settings.validate().unwrap();
    for value in [-1.0, 1.0, f64::NAN, f64::INFINITY] {
        let mut bad = seed.clone();
        bad.x = value;
        assert!(bad.validate().is_err());
    }
    settings.target_seed.as_mut().unwrap().class_id = Some(1);
    assert!(settings.validate().is_err());
    settings.target_seed.as_mut().unwrap().class_id = None;
    settings.target_seed.as_mut().unwrap().source_frame_index = 1;
    assert!(settings.validate().is_err());
    settings.target_seed.as_mut().unwrap().source_frame_index = 2;
    settings.model_input = None;
    assert!(settings.validate().is_err());
    let mut value = serde_json::to_value(GenerationSettings::default()).unwrap();
    assert!(!value.as_object().unwrap().contains_key("target_seed"));
    assert!(serde_json::from_value::<GenerationSettings>(value.clone())
        .unwrap()
        .target_seed
        .is_none());
    value["target_seed"] = serde_json::json!({"source_frame_index":0,"x":0.5,"y":0.5,"class_id":null,"coordinate_guess":"source"});
    assert!(serde_json::from_value::<GenerationSettings>(value).is_err());
}
