use pulsar_protocol::*;

fn settings() -> GenerationSettings {
    let identity = ArtifactIdentity {
        artifact_id: ArtifactId::new("authored").unwrap(),
        sha256: "a".repeat(64),
        byte_len: 1,
    };
    let input = ModelInputContract {
        width: 64,
        height: 64,
        channels: 6,
        class_count: 1,
        layout: TensorLayout::Nchw,
        decoder: DetectorDecoder::TemporalScriptPosition,
    };
    GenerationSettings {
        analysis_width:64,analysis_height:64,model_input:Some(input.clone()),
        annotated_view:Some(serde_json::from_value(serde_json::json!({
            "source_sha256":"b".repeat(64),"declaration":{"schema_version":1,
                "source_dimensions":[192,64],"analysis_dimensions":[96,64],
                "video_view":{"layout":"side_by_side_left_right","eye":"right","projection":{"kind":"flat"}},
                "target_region":[16,0,64,64],"target_id":"authored"}
        })).unwrap()),
        inference_profile:Some(DeclaredInferenceProfile {
            schema_version:1,model:identity.clone(),runtime:identity,
            preprocessing:InferencePreprocessing {input,resize:InferenceResize::AnnotatedTemporalView,
                color:InferenceColor::RgbFramePair,scale:vec![1.0/255.0;6],bias:vec![0.0;6],padding_byte:0},
            provider:InferenceProvider::Cpu,minimum_memory_bytes:128*1024*1024,
            minimum_output_bytes:1024*1024,cpu_threads:1,cuda_resources:None,
        }),..Default::default()
    }
}

#[test]
fn annotated_recipe_is_explicit_and_cannot_fall_back_to_whole_frames() {
    let valid = settings();
    valid.validate().unwrap();
    let bytes = serde_json::to_vec(&valid).unwrap();
    assert_eq!(
        serde_json::to_vec(&serde_json::from_slice::<GenerationSettings>(&bytes).unwrap()).unwrap(),
        bytes
    );
    for case in [
        "no_profile",
        "no_view",
        "legacy_resize",
        "wrong_color",
        "cuda",
        "padding",
        "bias",
        "dimensions",
        "view",
        "seed",
        "raw",
        "vr",
    ] {
        let mut bad = valid.clone();
        match case {
            "no_profile" => bad.inference_profile = None,
            "no_view" => bad.annotated_view = None,
            "legacy_resize" => {
                bad.inference_profile.as_mut().unwrap().preprocessing.resize =
                    InferenceResize::NearestStretch
            }
            "wrong_color" => {
                bad.inference_profile.as_mut().unwrap().preprocessing.color = InferenceColor::Rgb
            }
            "cuda" => bad.inference_profile.as_mut().unwrap().provider = InferenceProvider::Cuda,
            "padding" => {
                bad.inference_profile
                    .as_mut()
                    .unwrap()
                    .preprocessing
                    .padding_byte = 1
            }
            "bias" => bad.inference_profile.as_mut().unwrap().preprocessing.bias[0] = 0.1,
            "dimensions" => bad.analysis_width = 320,
            "view" => {
                bad.video_view = Some(
                    valid
                        .annotated_view
                        .as_ref()
                        .unwrap()
                        .declaration
                        .video_view
                        .clone(),
                )
            }
            "vr" => bad.vr_mode = true,
            "raw" => {
                bad.raw_frame_pair = Some(RawFramePair {
                    source_epoch: AttemptId::new("authored-epoch").unwrap(),
                    previous_frame: FrameId::new(0),
                    current_frame: FrameId::new(1),
                    previous_time: SourceTimestamp::new(0, 8).unwrap(),
                    current_time: SourceTimestamp::new(1, 8).unwrap(),
                    presentation_clock: None,
                })
            }
            _ => {
                bad.target_seed = Some(AnalysisTargetSeed {
                    source_frame_index: 0,
                    x: 0.5,
                    y: 0.5,
                    class_id: None,
                })
            }
        }
        assert!(bad.validate().is_err(), "{case}");
    }
    let legacy = serde_json::to_value(GenerationSettings::default()).unwrap();
    assert!(legacy.get("annotated_view").is_none());
    let mut unknown = serde_json::to_value(valid).unwrap();
    unknown["annotated_view"]["declaration"]["guess_target"] = true.into();
    assert!(serde_json::from_value::<GenerationSettings>(unknown).is_err());
}

#[test]
fn captured_source_must_match_the_annotation_content() {
    let mut request: WorkerRequest = serde_json::from_str(include_str!(
        "../../pulsar-engine/tests/fixtures/cuda-intent-request.json"
    ))
    .unwrap();
    let root = std::env::temp_dir().join("pulsar-annotated-contract-fixture");
    request.source.path = root.join("source");
    request.output_dir = root.join("attempt");
    request.tools.ffmpeg.path = root.join("ffmpeg");
    request.tools.ffprobe.path = root.join("ffprobe");
    request.tools.onnx_runtime.as_mut().unwrap().path = root.join("onnx_runtime");
    if let WorkerOperation::Generate {
        model: Some(model), ..
    } = &mut request.operation
    {
        model.path = root.join("model");
    }
    let mut settings = settings();
    let WorkerOperation::Generate {
        model: Some(model), ..
    } = &request.operation
    else {
        panic!("model fixture");
    };
    let profile = settings.inference_profile.as_mut().unwrap();
    profile.model = model.identity.clone();
    profile.runtime = request
        .tools
        .onnx_runtime
        .as_ref()
        .unwrap()
        .identity
        .clone();
    profile.cpu_threads = request.budget.cpu_threads;
    settings.annotated_view.as_mut().unwrap().source_sha256 =
        request.source.identity.sha256.clone();
    request.cuda_execution = None;
    request.operation = WorkerOperation::Generate {
        model: Some(model.clone()),
        preset: "fast".into(),
        settings: (settings).into(),
    };
    request.validate().unwrap();
    let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
        unreachable!()
    };
    settings.annotated_view.as_mut().unwrap().source_sha256 = "c".repeat(64);
    assert!(request
        .validate()
        .unwrap_err()
        .to_string()
        .contains("captured source content"));
}
