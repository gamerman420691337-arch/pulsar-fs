//! Authored native file path; no Engine admission or hardware qualification.
use super::*;

#[test]
#[ignore = "requires retained authored RGB/model and fresh pinned CPU runtime"]
fn authored_stereo_region_file_generation_matches_python_and_retains_plan() {
    authored_generation(false, false);
}
#[test]
#[ignore = "requires authored native model/runtime and separate fresh process; exercises portable clones"]
fn authored_annotated_placed_generation_and_portable_clones() {
    authored_generation(true, false);
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires a current worker executable, native authored fixture/runtime and delegated confinement"]
fn authored_annotated_public_engine_generation() {
    authored_generation(true, true);
}
fn authored_generation(placed: bool, public_engine: bool) {
    let root = std::path::PathBuf::from(
        std::env::var_os("PULSAR_ANNOTATED_MODEL_FIXTURE_DIR").expect("fixture directory"),
    );
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("expected.json")).unwrap()).unwrap();
    let pairs = std::fs::read(root.join("pairs.rgb")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&pairs)),
        expected["rgb_sha256"].as_str().unwrap()
    );
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let raw = directory.path().join("stereo.rgb");
    let mut packed = Vec::new();
    // Left eye and the unselected right-eye border deliberately differ.
    // Only the declared right-eye integer region contains the Python inputs.
    for frame in pairs.as_chunks::<{ 64 * 64 * 3 }>().0.iter() {
        for row in frame.as_chunks::<{ 64 * 3 }>().0.iter() {
            packed.extend_from_slice(&[17; 96 * 3]);
            packed.extend_from_slice(&[231; 16 * 3]);
            packed.extend_from_slice(row);
            packed.extend_from_slice(&[231; 16 * 3]);
        }
    }
    std::fs::write(&raw, &packed).unwrap();
    let video = directory.path().join("stereo.mkv");
    let mut command = std::process::Command::new(&tools.ffmpeg.path);
    command
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgb24",
            "-video_size",
            "192x64",
            "-framerate",
            "8",
            "-i",
        ])
        .arg(&raw)
        .args(["-threads", "1", "-c:v", "ffv1"])
        .arg(&video);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    let source = pin_artifact(&video).unwrap();
    let source = SourceArtifact {
        source_version: SourceVersionId::new(source.identity.sha256.clone()).unwrap(),
        path: source.path,
        identity: source.identity,
    };
    std::fs::copy(root.join("model.onnx"), directory.path().join("model.onnx")).unwrap();
    let model = pin_artifact(&directory.path().join("model.onnx")).unwrap();
    assert_eq!(
        model.identity.sha256,
        expected["model_sha256"].as_str().unwrap()
    );
    let model = SourceArtifact {
        source_version: SourceVersionId::new("authored-model").unwrap(),
        path: model.path,
        identity: model.identity,
    };
    let input = ModelInputContract {
        width: 64,
        height: 64,
        channels: 6,
        class_count: 1,
        layout: TensorLayout::Nchw,
        decoder: DetectorDecoder::TemporalScriptPosition,
    };
    let binding:AnnotatedGenerationView=serde_json::from_value(serde_json::json!({
        "source_sha256":source.identity.sha256,"declaration":{"schema_version":1,
            "source_dimensions":[192,64],"analysis_dimensions":[96,64],"target_region":[16,0,64,64],"target_id":"authored",
            "video_view":{"layout":"side_by_side_left_right","eye":"right","projection":{"kind":"flat"}}}
    })).unwrap();
    let profile = DeclaredInferenceProfile {
        schema_version: 1,
        model: model.identity.clone(),
        runtime: tools
            .onnx_runtime
            .as_ref()
            .expect("pinned runtime")
            .identity
            .clone(),
        preprocessing: InferencePreprocessing {
            input: input.clone(),
            resize: InferenceResize::AnnotatedTemporalView,
            color: InferenceColor::RgbFramePair,
            scale: vec![1.0 / 255.0; 6],
            bias: vec![0.0; 6],
            padding_byte: 0,
        },
        provider: InferenceProvider::Cpu,
        minimum_memory_bytes: 128 * 1024 * 1024,
        minimum_output_bytes: 1024 * 1024,
        cpu_threads: 1,
        cuda_resources: None,
    };
    let output = directory.path().join("attempt");
    std::fs::create_dir(&output).unwrap();
    let mut request = WorkerRequest {
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new("authored-attempt").unwrap(),
        job_id: JobId::new("authored-job").unwrap(),
        project_id: ProjectId::new("authored-project").unwrap(),
        base_revision: RevisionId::new(0),
        source: source.clone(),
        tools,
        operation: WorkerOperation::Generate {
            preset: "fast".into(),
            settings: (GenerationSettings {
                annotated_view: Some(binding.clone()),
                analysis_width: 64,
                analysis_height: 64,
                model_input: Some(input),
                inference_profile: Some(profile),
                ..Default::default()
            })
            .into(),
            model: Some(model),
        },
        output_dir: output.canonicalize().unwrap(),
        budget: ResourceBudget {
            memory_bytes: 512 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            wall_time_ms: 30_000,
            cpu_threads: 1,
        },
        dependencies: vec![],
        cuda_execution: None,
        independent_audio: None,
    };
    let WorkerOperation::Generate { settings, .. } = &mut request.operation else {
        panic!()
    };
    if placed {
        settings.source_placement = Some(SourcePlacement {
            placement_id: SourcePlacementId::new("authored-placement").unwrap(),
            created_revision: RevisionId::new(0),
            spec: SourcePlacementSpec {
                source_version: request.source.source_version.clone(),
                source_start: SourceTimestamp::new(0, 1).unwrap(),
                source_end: SourceTimestamp::new(4, 1).unwrap(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::from_nanos(0),
                rate_numerator: 1,
                rate_denominator: 1,
            },
        });
    }
    request.dependencies.push(
        crate::inference_profiles::profile_identity(settings.inference_profile.as_ref().unwrap())
            .unwrap(),
    );
    request.validate().unwrap();
    // Invalid geometry must fail before constructing any session or artifact.
    let mut bad = request.clone();
    if let WorkerOperation::Generate { settings, .. } = &mut bad.operation {
        settings
            .annotated_view
            .as_mut()
            .unwrap()
            .declaration
            .source_dimensions = [194, 64];
    }
    assert!(execute_manifest(&bad)
        .result
        .unwrap_err()
        .to_string()
        .contains("encoded dimensions differ"));
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);
    // Independently observe every transformed decoder frame before inference.
    let plan =
        annotated_input::plan(&binding, &source, [192, 64], request.budget.memory_bytes).unwrap();
    let mut decoder = media::Decoder::new_filtered(
        &request.tools.ffmpeg.path,
        &source.path,
        media::FrameTransform {
            width: 64,
            height: 64,
            filter: &plan.native_plan.spatial_filter,
        },
        1,
        1,
        Instant::now() + Duration::from_secs(20),
    )
    .unwrap();
    for (ordinal, frame) in pairs.as_chunks::<{ 64 * 64 * 3 }>().0.iter().enumerate() {
        let decoded = decoder.next().unwrap().unwrap();
        assert_eq!(decoded.ordinal, ordinal as u64);
        assert_eq!(decoded.pixels, frame);
    }
    assert!(decoder.next().unwrap().is_none());
    let WorkerOutput::Generated {
        program,
        receipt,
        lineage,
        review,
    } = execute_manifest(&request).result.unwrap()
    else {
        panic!("generated output");
    };
    annotated_receipt::verify_file(&request, &program.identity, &receipt, &lineage, &review)
        .unwrap();
    let receipt_bytes = std::fs::read(&receipt.path).unwrap();
    let model_bytes = std::fs::read(root.join("model.onnx")).unwrap();
    let mut inert = request.clone();
    inert.source.path = "/inert/archived-source-never-opened".into();
    let WorkerOperation::Generate {
        model: Some(model), ..
    } = &mut inert.operation
    else {
        panic!()
    };
    model.path = "/inert/archived-model-never-opened".into();
    annotated_receipt::verify_readers(
        &inert,
        &program.identity,
        &receipt.identity,
        &lineage,
        &review,
        std::io::Cursor::new(&receipt_bytes),
        std::io::Cursor::new(&model_bytes),
    )
    .unwrap();
    let mut corrupt = model_bytes.clone();
    corrupt[0] ^= 1;
    assert!(annotated_receipt::verify_readers(
        &inert,
        &program.identity,
        &receipt.identity,
        &lineage,
        &review,
        std::io::Cursor::new(&receipt_bytes),
        std::io::Cursor::new(corrupt)
    )
    .is_err());
    for pointer in [
        "/annotated_model/training_intake_sha256",
        "/annotated_input/native_plan/spatial_filter",
        "/configuration_sha256",
    ] {
        let mut bad: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
        *bad.pointer_mut(pointer).unwrap() = serde_json::json!("changed");
        let bad = serde_json::to_vec(&bad).unwrap();
        let sha256 = format!("{:x}", Sha256::digest(&bad));
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: bad.len() as u64,
        };
        assert!(annotated_receipt::verify_readers(
            &inert,
            &program.identity,
            &identity,
            &lineage,
            &review,
            std::io::Cursor::new(bad),
            std::io::Cursor::new(&model_bytes)
        )
        .is_err());
    }
    let raw_program_reference = program.clone();
    let raw_receipt_reference = receipt.clone();
    let program: MotionProgram =
        serde_json::from_slice(&std::fs::read(program.path).unwrap()).unwrap();
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt.path).unwrap()).unwrap();
    assert_eq!(receipt["schema_version"], 3);
    assert_eq!(receipt["recipe"], ANNOTATED_TEMPORAL_RECIPE);
    assert_eq!(
        receipt["annotated_input"],
        serde_json::to_value(&plan).unwrap()
    );
    assert_eq!(
        receipt["configuration"]["annotated_view"],
        serde_json::to_value(binding).unwrap()
    );
    assert_eq!(
        receipt["annotated_model"]["contract"],
        serde_json::to_value(AnnotatedTemporalModelContract::expected()).unwrap()
    );
    assert_eq!(
        receipt["annotated_model"]["training_intake_sha256"],
        expected["training_intake_sha256"]
    );
    let actions = program.track(Axis::Stroke).unwrap().actions();
    let positions = expected["positions"].as_array().unwrap();
    for (i, position) in positions.iter().enumerate() {
        let time = ((i * 2 + 1) as i64) * 125_000_000;
        let action = actions
            .iter()
            .find(|action| action.time().as_nanos() == time)
            .expect("authored pair has continuous support");
        assert_eq!(action.evidence(), EvidenceKind::Inferred);
        assert!((action.position().value() - position.as_f64().unwrap()).abs() <= 1e-6);
        assert_eq!(
            receipt["samples"][i * 2 + 1]["source_frame_index"],
            i * 2 + 1
        );
        assert_eq!(receipt["samples"][i * 2 + 1]["project_time_ns"], time);
    }
    #[cfg(target_os = "linux")]
    if public_engine {
        crate::authority::tests::verify_public_annotated_generation(&request);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = public_engine;
    if placed {
        super::annotated_portable_tests::verify_native_portable(
            &request,
            &raw_program_reference,
            &raw_receipt_reference,
            &lineage,
            &review,
        );
    }
}
