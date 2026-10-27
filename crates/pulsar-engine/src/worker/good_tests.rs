//! Good-phase worker regressions using authored synthetic media.
//! These tests exercise decoding and candidate generation, not real-media
//! accuracy, process confinement, vendor speed, or physical device qualification.

use super::*;
use std::path::Path;
use std::process::Command;

fn source_from(path: &Path, name: &str) -> SourceArtifact {
    let pinned = pin_artifact(path).unwrap();
    SourceArtifact {
        source_version: SourceVersionId::new(name).unwrap(),
        path: pinned.path,
        identity: pinned.identity,
    }
}

fn request(
    directory: &Path,
    name: &str,
    tools: WorkerTools,
    source: SourceArtifact,
    operation: WorkerOperation,
) -> WorkerRequest {
    let output = directory.join(name);
    std::fs::create_dir(&output).unwrap();
    WorkerRequest {
        cuda_execution: None,
        independent_audio: None,
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new(format!("good-{name}")).unwrap(),
        job_id: JobId::new(format!("good-job-{name}")).unwrap(),
        project_id: ProjectId::new("good-project").unwrap(),
        base_revision: RevisionId::new(0),
        source,
        output_dir: output.canonicalize().unwrap(),
        operation,
        budget: ResourceBudget {
            memory_bytes: 512 * 1024 * 1024,
            output_bytes: 16 * 1024 * 1024,
            wall_time_ms: 30_000,
            cpu_threads: 1,
        },
        dependencies: Vec::new(),
        tools,
    }
}

fn generated(request: &WorkerRequest) -> (MotionProgram, serde_json::Value, Vec<ReviewFlag>) {
    let WorkerOutput::Generated {
        program,
        receipt,
        lineage,
        review,
    } = execute_manifest(request)
        .result
        .unwrap_or_else(|error| panic!("{error:?}"))
    else {
        panic!("expected a generated candidate");
    };
    assert!(lineage.contains(&request.source.identity));
    let program = serde_json::from_slice(&std::fs::read(program.path).unwrap()).unwrap();
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt.path).unwrap()).unwrap();
    assert_eq!(receipt["qualification"], "unqualified");
    (program, receipt, review)
}

fn translated_video(
    directory: &Path,
    tools: &WorkerTools,
    name: &str,
    vertical_positions: &[i32],
) -> SourceArtifact {
    let mut pixels = Vec::with_capacity(vertical_positions.len() * 64 * 64 * 3);
    for displacement in vertical_positions {
        for y in 0..64i32 {
            for x in 0..64i32 {
                let source_y = (y - displacement).rem_euclid(64);
                let value = ((x * 31 + source_y * 17 + (x / 7) * (source_y / 5) * 13) % 256) as u8;
                pixels.extend_from_slice(&[value, value, value]);
            }
        }
    }
    let raw = directory.join(format!("{name}.rgb"));
    std::fs::write(&raw, pixels).unwrap();
    let video = directory.join(format!("{name}.mkv"));
    let mut command = Command::new(&tools.ffmpeg.path);
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
            "64x64",
            "-framerate",
            "8",
            "-i",
        ])
        .arg(&raw)
        .args([
            "-frames:v",
            &vertical_positions.len().to_string(),
            "-threads",
            "1",
            "-c:v",
            "ffv1",
        ])
        .arg(&video);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    source_from(&video, name)
}

fn video_candidate(positions: &[i32]) -> MotionProgram {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().expect("synthetic decoder tests require declared ffmpeg/ffprobe");
    let source = translated_video(directory.path(), &tools, "translated", positions);
    let request = request(
        directory.path(),
        "generation",
        tools,
        source,
        WorkerOperation::Generate {
            preset: "default".into(),
            settings: (GenerationSettings {
                inference_profile: None,
                analysis_width: 64,
                analysis_height: 64,
                ..Default::default()
            })
            .into(),
            model: None,
        },
    );
    let (program, _, review) = generated(&request);
    assert!(
        !review.is_empty(),
        "global motion without a target model must retain review uncertainty"
    );
    let track = program.track(Axis::Stroke).unwrap();
    assert!(track
        .actions()
        .iter()
        .all(|a| a.evidence() != EvidenceKind::Observed));
    assert!(track
        .actions()
        .iter()
        .all(|a| a.position().value() > 0.0 && a.position().value() < 1.0));
    program
}

fn position_at(program: &MotionProgram, nanos: i64) -> f64 {
    program
        .track(Axis::Stroke)
        .unwrap()
        .actions()
        .iter()
        .find(|action| action.time().as_nanos() == nanos)
        .unwrap_or_else(|| panic!("missing independent time anchor {nanos}"))
        .position()
        .value()
}

#[test]
fn m1_worker_constant_displacement_never_invents_stroke() {
    let program = video_candidate(&[3, 3, 3, 3, 3]);
    let actions = program.track(Axis::Stroke).unwrap().actions();
    assert!(actions.len() >= 2);
    assert!(actions
        .iter()
        .all(|action| (action.position().value() - 0.5).abs() < 1e-9));
    assert_eq!(actions.first().unwrap().time().as_nanos(), 0);
    assert_eq!(actions.last().unwrap().time().as_nanos(), 500_000_000);
}

#[test]
fn m1_worker_monotonic_decreasing_motion_keeps_direction_and_shallow_travel() {
    let program = video_candidate(&[0, -1, -2, -3]);
    for index in 0..4 {
        let expected = 0.5 - f64::from(index) / 64.0;
        assert!((position_at(&program, i64::from(index) * 125_000_000) - expected).abs() < 1e-9,
            "one source pixel must remain one sixty-fourth neutral travel, not full-range normalization");
    }
}

#[test]
fn m1_worker_trailing_pause_does_not_add_reversal() {
    let program = video_candidate(&[0, 1, 0, 0, 0]);
    assert!((position_at(&program, 125_000_000) - (0.5 + 1.0 / 64.0)).abs() < 1e-9);
    for nanos in [250_000_000, 375_000_000, 500_000_000] {
        assert!(
            (position_at(&program, nanos) - 0.5).abs() < 1e-9,
            "trailing pause must hold instead of manufacturing another endpoint"
        );
    }
}

#[test]
fn m1_worker_preserves_both_pause_boundaries_for_peak_and_trough() {
    for displacement in [-1, 1] {
        let program = video_candidate(&[0, displacement, displacement, displacement, 0]);
        let plateau = 0.5 + f64::from(displacement) / 64.0;
        assert!(
            (position_at(&program, 125_000_000) - plateau).abs() < 1e-9,
            "pause start lost"
        );
        assert!(
            (position_at(&program, 375_000_000) - plateau).abs() < 1e-9,
            "pause end lost"
        );
        assert!((position_at(&program, 500_000_000) - 0.5).abs() < 1e-9);
    }
}

fn fake_tools(directory: &Path) -> WorkerTools {
    let path = directory.join("deliberately-not-an-executable-media-tool");
    std::fs::write(
        &path,
        b"An immutable test dependency, not a media executable.",
    )
    .unwrap();
    let artifact = pin_artifact(&path).unwrap();
    WorkerTools {
        ffmpeg: artifact.clone(),
        ffprobe: artifact,
        onnx_runtime: None,
    }
}

fn text_input() -> GenerationInput {
    GenerationInput::Text {
        prompt: "pattern=hold axis=stroke duration=2s frequency=0hz amplitude=0 offset=0.42".into(),
    }
}

fn declared_source(directory: &Path, input: &GenerationInput, name: &str) -> SourceArtifact {
    let path = directory.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_vec(input).unwrap()).unwrap();
    source_from(&path, name)
}

#[test]
fn typed_text_never_invokes_video_probe_and_retains_synthetic_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let input = text_input();
    let source = declared_source(directory.path(), &input, "text");
    let tools = fake_tools(directory.path());
    let request = request(
        directory.path(),
        "text-attempt",
        tools,
        source,
        WorkerOperation::GenerateInput { input },
    );
    let (program, receipt, review) = generated(&request);
    let actions = program.track(Axis::Stroke).unwrap().actions();
    assert_eq!(actions.len(), 2);
    assert!(actions
        .iter()
        .all(|a| a.position().value() == 0.42 && a.evidence() == EvidenceKind::Synthesized));
    assert_eq!(actions[1].time().as_nanos(), 2_000_000_000);
    assert_eq!(receipt["details"]["natural_language_inference"], false);
    assert!(review
        .iter()
        .any(|flag| flag.reason.as_str() == "constrained_pattern_parser_not_local_ai"));
}

#[test]
fn typed_patterns_generate_six_axes_without_video_probe() {
    let directory = tempfile::tempdir().unwrap();
    let patterns = ["stroke", "sway", "surge", "roll", "pitch", "yaw"]
        .iter()
        .map(|axis| {
            parse_pattern_prompt(&format!(
                "pattern=hold axis={axis} duration=1s frequency=0hz amplitude=0 offset=0.4"
            ))
            .unwrap()
        })
        .collect();
    let input = GenerationInput::Patterns { patterns };
    let source = declared_source(directory.path(), &input, "patterns");
    let request = request(
        directory.path(),
        "patterns-attempt",
        fake_tools(directory.path()),
        source,
        WorkerOperation::GenerateInput { input },
    );
    let (program, receipt, _) = generated(&request);
    assert_eq!(program.tracks().len(), 6);
    for track in program.tracks() {
        assert_eq!(track.actions().len(), 2);
        assert!(track
            .actions()
            .iter()
            .all(|a| a.position().value() == 0.4 && a.evidence() == EvidenceKind::Synthesized));
        assert_eq!(track.actions()[1].time().as_nanos(), 1_000_000_000);
    }
    assert_eq!(receipt["details"]["observed_motion"], false);
}

#[test]
fn synthetic_operation_must_match_exact_immutable_serialized_source() {
    for replace_bytes in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let original = text_input();
        let source = declared_source(directory.path(), &original, "bound-text");
        let different = GenerationInput::Text {
            prompt: "pattern=hold axis=stroke duration=2s frequency=0hz amplitude=0 offset=0.43"
                .into(),
        };
        let operation = if replace_bytes {
            std::fs::write(&source.path, serde_json::to_vec(&different).unwrap()).unwrap();
            WorkerOperation::GenerateInput { input: original }
        } else {
            WorkerOperation::GenerateInput { input: different }
        };
        let request = request(
            directory.path(),
            "mismatch",
            fake_tools(directory.path()),
            source,
            operation,
        );
        assert_eq!(
            execute_manifest(&request).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
    }
}

#[test]
fn semantically_equal_but_reencoded_synthetic_source_is_not_exact_binding() {
    let directory = tempfile::tempdir().unwrap();
    let input = text_input();
    let path = directory.path().join("pretty.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&input).unwrap()).unwrap();
    let source = source_from(&path, "pretty");
    let request = request(
        directory.path(),
        "pretty-attempt",
        fake_tools(directory.path()),
        source,
        WorkerOperation::GenerateInput { input },
    );
    assert_eq!(
        execute_manifest(&request).result.unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
}

fn wav_fixture(directory: &Path, tools: &WorkerTools) -> SourceArtifact {
    // Twelve 10 ms windows: silence, four exact 400 Hz periods/window at
    // amplitude 0.1, silence. Oracle RMS is 0.1 / sqrt(2), never peak-normalized.
    let mut pcm = Vec::new();
    for sample in 0..1920usize {
        let value = if (640..1280).contains(&sample) {
            (0.1 * (std::f64::consts::TAU * 400.0 * sample as f64 / 16_000.0).sin()) as f32
        } else {
            0.0
        };
        pcm.extend_from_slice(&value.to_le_bytes());
    }
    let raw = directory.join("authored-audio.f32");
    std::fs::write(&raw, pcm).unwrap();
    let wav = directory.join("authored-audio.wav");
    let mut command = Command::new(&tools.ffmpeg.path);
    command
        .args([
            "-v", "error", "-nostdin", "-f", "f32le", "-ar", "16000", "-ac", "1", "-i",
        ])
        .arg(raw)
        .args(["-threads", "1", "-c:a", "pcm_f32le"])
        .arg(&wav);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    source_from(&wav, "authored-audio")
}

#[test]
fn audio_only_decoder_preserves_absolute_rms_and_silent_boundaries() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let source = wav_fixture(directory.path(), &tools);
    let input = GenerationInput::Audio {
        source_version: source.source_version.clone(),
        mapping: AudioMapping::new(Axis::Stroke, 0.2, NormalizedPosition::new(0.4).unwrap())
            .unwrap(),
        mode: AudioMode::Envelope,
        analysis: None,
    };
    let request = request(
        directory.path(),
        "audio-attempt",
        tools,
        source,
        WorkerOperation::GenerateInput { input },
    );
    let (program, receipt, review) = generated(&request);
    assert!((position_at(&program, 0) - 0.4).abs() < 1e-9);
    assert!((position_at(&program, 40_000_000) - (0.4 + 0.2 * 0.1 / 2.0f64.sqrt())).abs() < 1e-8);
    assert!((position_at(&program, 80_000_000) - 0.4).abs() < 1e-9);
    assert!((position_at(&program, 120_000_000) - 0.4).abs() < 1e-9);
    assert!(program
        .track(Axis::Stroke)
        .unwrap()
        .actions()
        .iter()
        .all(|action| action.position().value() >= 0.4
            && action.position().value() < 0.415
            && action.evidence() == EvidenceKind::Synthesized));
    assert_eq!(receipt["details"]["resampled_sample_count"], 1920);
    let source_timeline = &receipt["details"]["source_timeline"];
    assert_eq!(source_timeline["first_pts"], 0);
    assert_eq!(source_timeline["time_base_numerator"], 1);
    assert_eq!(source_timeline["time_base_denominator"], 16_000);
    assert_eq!(source_timeline["source_sample_rate"], 16_000);
    assert_eq!(source_timeline["source_samples"], 1920);
    assert_eq!(source_timeline["resampled_rate"], 16_000);
    assert_eq!(source_timeline["continuity_tolerance_ticks"], 1);
    assert!(source_timeline["origin_mapping"]
        .as_str()
        .unwrap()
        .contains("subtract first decoded frame PTS"));
    assert_eq!(receipt["details"]["normalization"], "none");
    assert!(review
        .iter()
        .any(|flag| flag.reason.as_str() == "audio_mapping_uncalibrated"));
}

#[test]
fn explicit_audio_analysis_retains_decoder_clock_and_reports_gating() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let source = wav_fixture(directory.path(), &tools);
    for (name, band, threshold) in [
        ("mid-pass", AudioBand::Mid, 0.05),
        ("mid-gated", AudioBand::Mid, 0.2),
        ("bass-reject", AudioBand::Bass, 0.05),
    ] {
        let input = GenerationInput::Audio {
            source_version: source.source_version.clone(),
            mapping: AudioMapping::new(Axis::Stroke, 0.5, NormalizedPosition::new(0.25).unwrap())
                .unwrap(),
            mode: AudioMode::Envelope,
            analysis: Some(AudioAnalysisOptions::new(band, threshold).unwrap()),
        };
        let request = request(
            directory.path(),
            name,
            tools.clone(),
            source.clone(),
            WorkerOperation::GenerateInput { input },
        );
        let (program, receipt, _) = generated(&request);
        let actions = program.track(Axis::Stroke).unwrap().actions();
        assert_eq!(actions.last().unwrap().time().as_nanos(), 120_000_000);
        if name == "mid-pass" {
            assert!(actions.iter().any(|a| a.position().value() > 0.275));
        } else {
            assert!(actions.iter().all(|a| a.position().value() == 0.25));
        }
        assert_eq!(receipt["recipe"], "spectral-rms-envelope-v1");
        assert_eq!(
            receipt["details"]["source_timeline"]["source_samples"],
            1920
        );
        assert_eq!(receipt["details"]["analysis"]["envelope_gated"], true);
        assert_eq!(
            receipt["details"]["analysis"]["options"]["threshold"],
            threshold
        );
        assert_eq!(
            receipt["details"]["analysis"]["filter_delay"],
            "frequency-dependent-uncompensated"
        );
        assert_eq!(receipt["details"]["normalization"], "none");
    }
}

#[test]
fn audio_vibe_worker_preserves_quiet_energy_clock_and_center() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let source = wav_fixture(directory.path(), &tools);
    let input = GenerationInput::Audio {
        source_version: source.source_version.clone(),
        mapping: AudioMapping::new(Axis::Stroke, 0.5, NormalizedPosition::new(0.25).unwrap())
            .unwrap(),
        mode: AudioMode::Vibe,
        analysis: None,
    };
    let request = request(
        directory.path(),
        "vibe",
        tools,
        source,
        WorkerOperation::GenerateInput { input },
    );
    let (program, receipt, review) = generated(&request);
    for time in [0, 80_000_000, 120_000_000] {
        assert!((position_at(&program, time) - 0.5).abs() < 1e-12);
    }
    let expected = 0.5 + 0.25 * 0.1 / 2.0_f64.sqrt() * (std::f64::consts::TAU * 8.0 * 0.04).sin();
    assert!((position_at(&program, 40_000_000) - expected).abs() < 1e-8);
    assert_eq!(
        program
            .track(Axis::Stroke)
            .unwrap()
            .actions()
            .last()
            .unwrap()
            .time()
            .as_nanos(),
        120_000_000
    );
    assert_eq!(receipt["recipe"], "audio-rms-modulated-oscillation-8hz-v1");
    assert_eq!(receipt["details"]["carrier_hz"], 8);
    assert_eq!(receipt["details"]["carrier_period_ns"], 125_000_000);
    assert_eq!(
        receipt["details"]["phase_arithmetic"],
        "integer-nanosecond-modulo"
    );
    assert_eq!(receipt["details"]["position_center"], 0.5);
    assert_eq!(receipt["details"]["position_half_span"], 0.25);
    assert_eq!(
        receipt["details"]["source_timeline"]["source_samples"],
        1920
    );
    assert!(review
        .iter()
        .any(|flag| flag.reason.as_str() == "audio_oscillation_not_device_qualified"));
}

fn still_fixture(directory: &Path, name: &str, invert: bool) -> SourceArtifact {
    let mut bytes = b"P6\n64 64\n255\n".to_vec();
    for y in 0..64usize {
        for x in 0..64usize {
            let value = ((x * 17 + y * 31) % 256) as u8;
            let value = if invert { 255 - value } else { value };
            bytes.extend_from_slice(&[value, value, value]);
        }
    }
    let path = directory.join(format!("{name}.ppm"));
    std::fs::write(&path, bytes).unwrap();
    source_from(&path, name)
}

#[test]
fn still_plus_prompt_decodes_image_but_does_not_claim_semantic_conditioning() {
    let directory = tempfile::tempdir().unwrap();
    let mut programs = Vec::new();
    for (name, invert) in [("image-a", false), ("image-b", true)] {
        let source = still_fixture(directory.path(), name, invert);
        let input = GenerationInput::Image {
            source_version: source.source_version.clone(),
            prompt:
                "pattern=triangle axis=stroke duration=1s frequency=1hz amplitude=0.1 offset=0.5"
                    .into(),
        };
        let request = request(
            directory.path(),
            &format!("{name}-attempt"),
            discover_tools().unwrap(),
            source,
            WorkerOperation::GenerateInput { input },
        );
        let (program, receipt, review) = generated(&request);
        assert_eq!(
            receipt["details"]["image_dimensions"],
            serde_json::json!([64, 64])
        );
        assert_eq!(receipt["details"]["image_semantic_inference"], false);
        assert!(review
            .iter()
            .any(|flag| flag.reason.as_str() == "image_not_semantically_conditioning_motion"));
        assert!(program
            .track(Axis::Stroke)
            .unwrap()
            .actions()
            .iter()
            .all(|a| a.evidence() == EvidenceKind::Synthesized
                && (0.4..=0.6).contains(&a.position().value())));
        programs.push(serde_json::to_value(program).unwrap());
    }
    assert_eq!(
        programs[0], programs[1],
        "this adapter must disclose that only the explicit prompt conditions motion"
    );
}

#[test]
fn still_without_prompt_is_rejected_before_native_tool_execution() {
    let directory = tempfile::tempdir().unwrap();
    let source = still_fixture(directory.path(), "missing-prompt", false);
    let input = GenerationInput::Image {
        source_version: source.source_version.clone(),
        prompt: String::new(),
    };
    let request = request(
        directory.path(),
        "missing-prompt-attempt",
        fake_tools(directory.path()),
        source,
        WorkerOperation::GenerateInput { input },
    );
    assert_eq!(
        execute_manifest(&request).result.unwrap_err().code,
        ProtocolError::invalid("").code
    );
}

#[test]
fn image_modality_cannot_relabel_a_multiframe_video_as_one_still() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let source = translated_video(directory.path(), &tools, "not-a-still", &[0, 1, 2]);
    let input = GenerationInput::Image {
        source_version: source.source_version.clone(),
        prompt: "pattern=hold axis=stroke duration=1s frequency=0hz amplitude=0 offset=0.5".into(),
    };
    let request = request(
        directory.path(),
        "not-a-still-attempt",
        tools,
        source,
        WorkerOperation::GenerateInput { input },
    );
    assert_eq!(
        execute_manifest(&request).result.unwrap_err().code,
        ProtocolError::invalid("").code
    );
}

#[test]
fn discontinuous_audio_pts_are_rejected_not_collapsed_into_earlier_motion() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let original = wav_fixture(directory.path(), &tools);
    let gapped = directory.path().join("gapped-audio.mka");
    let mut command = Command::new(&tools.ffmpeg.path);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&original.path)
        .args([
            "-af",
            r"asetnsamples=n=160:p=0,asetpts=PTS+if(gte(T\,0.05)\,1/TB\,0)",
            "-threads",
            "1",
            "-c:a",
            "pcm_f32le",
            "-f",
            "matroska",
        ])
        .arg(&gapped);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    let mut probe = Command::new(&tools.ffprobe.path);
    probe
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_frames",
            "-show_entries",
            "frame=pts_time,nb_samples",
            "-of",
            "json",
        ])
        .arg(&gapped);
    let frame_metadata =
        media::bounded_output(probe, 32 * 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    let metadata: serde_json::Value = serde_json::from_slice(&frame_metadata).unwrap();
    let times: Vec<f64> = metadata["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["pts_time"].as_str().unwrap().parse::<f64>().unwrap())
        .collect();
    assert!(
        times.windows(2).any(|pair| pair[1] - pair[0] > 0.5),
        "independent fixture oracle requires an actual source timestamp gap"
    );
    let source = source_from(&gapped, "gapped-audio");
    let input = GenerationInput::Audio {
        source_version: source.source_version.clone(),
        mapping: AudioMapping::new(Axis::Stroke, 0.2, NormalizedPosition::new(0.4).unwrap())
            .unwrap(),
        mode: AudioMode::Envelope,
        analysis: None,
    };
    let request = request(
        directory.path(),
        "gapped-attempt",
        tools,
        source,
        WorkerOperation::GenerateInput { input },
    );
    let error = execute_manifest(&request).result.unwrap_err();
    assert_eq!(
        error.code,
        ProtocolError::unsupported("").code,
        "unsupported discontinuous input must not silently collapse physical source timing"
    );
    assert!(
        !request.output_dir.join("candidate.json").exists(),
        "rejected audio must not publish a candidate artifact"
    );
}

#[test]
fn mixed_video_audio_preserves_faithful_motion_and_original_stream_offsets() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let video = translated_video(
        directory.path(),
        &tools,
        "mixed-visual",
        &[0, 1, 2, 3, 4, 5, 6, 7],
    );
    let raw = directory.path().join("mixed-audio.f32");
    std::fs::write(
        &raw,
        (0..16000)
            .flat_map(|_| 0.4f32.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mux = directory.path().join("mixed.mkv");
    let mut command = Command::new(&tools.ffmpeg.path);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&video.path)
        .args(["-f", "f32le", "-ar", "16000", "-ac", "1", "-i"])
        .arg(&raw)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-af",
            "asetpts=PTS+0.125/TB",
            "-c:v",
            "copy",
            "-c:a",
            "pcm_f32le",
            "-threads",
            "1",
        ])
        .arg(&mux);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    let source = source_from(&mux, "mixed-container");
    let operation = |intent: Option<MultimodalIntent>| WorkerOperation::Generate {
        preset: "fast".into(),
        model: None,
        settings: (GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            mixed_audio: intent.map(|intent| MixedAudioSettings {
                independent: None,
                intent,
                mapping: AudioMapping::new(
                    Axis::Stroke,
                    0.2,
                    NormalizedPosition::new(0.4).unwrap(),
                )
                .unwrap(),
            }),
            ..Default::default()
        })
        .into(),
    };
    let (visual, _, _) = generated(&request(
        directory.path(),
        "visual-only",
        tools.clone(),
        source.clone(),
        operation(None),
    ));
    let (faithful, receipt, review) = generated(&request(
        directory.path(),
        "mixed-faithful",
        tools.clone(),
        source.clone(),
        operation(Some(MultimodalIntent::Faithful)),
    ));
    assert_eq!(
        faithful, visual,
        "conflicting audio must not alter available visual samples or times"
    );
    assert_eq!(receipt["mixed_audio"]["audio_timeline"]["first_pts"], 125);
    assert_eq!(
        receipt["mixed_audio"]["audio_timeline"]["time_base_denominator"],
        1000
    );
    assert!(review
        .iter()
        .any(|r| r.reason.as_str() == "mixed_audio_outside_decoded_support"));
    let (creative, receipt, review) = generated(&request(
        directory.path(),
        "mixed-creative",
        tools,
        source,
        operation(Some(MultimodalIntent::CreativeAudio)),
    ));
    let track = creative.track(Axis::Stroke).unwrap();
    assert_eq!(
        track.gaps(),
        &[TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(125_000_000)).unwrap()]
    );
    assert_eq!(
        track.actions().first().unwrap().time().as_nanos(),
        125_000_000
    );
    assert!(track
        .actions()
        .iter()
        .all(|a| (a.position().value() - 0.48).abs() < 1e-8
            && a.evidence() == EvidenceKind::Synthesized));
    let samples = receipt["samples"].as_array().unwrap();
    assert_eq!(
        samples[0]["mixed_audio"]["audio_window_index"],
        serde_json::Value::Null
    );
    assert_eq!(samples[1]["mixed_audio"]["audio_window_index"], 0);
    assert_eq!(samples[2]["mixed_audio"]["audio_window_index"], 12);
    assert_eq!(samples[2]["project_time_ns"], 250_000_000);
    assert_eq!(samples[2]["mixed_audio"]["visual_evidence"], "inferred");
    assert_eq!(samples[2]["mixed_audio"]["contribution"], "audio");
    assert!(review
        .iter()
        .any(|r| r.reason.as_str() == "mixed_audio_synthesized_uncalibrated"));
}

#[test]
fn independent_audio_alignment_retains_exact_support_and_captured_identity() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let video = translated_video(
        directory.path(),
        &tools,
        "independent-visual",
        &[0, 1, 2, 3, 4, 5, 6, 7],
    );
    let pcm = directory.path().join("independent.f32");
    std::fs::write(
        &pcm,
        (0..16000)
            .flat_map(|_| 0.4f32.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let audio_path = directory.path().join("independent.mka");
    let mut command = Command::new(&tools.ffmpeg.path);
    command
        .args([
            "-v", "error", "-nostdin", "-f", "f32le", "-ar", "16000", "-ac", "1", "-i",
        ])
        .arg(&pcm)
        .args([
            "-af",
            "asetpts=PTS+0.125/TB",
            "-c:a",
            "pcm_f32le",
            "-threads",
            "1",
        ])
        .arg(&audio_path);
    assert!(command.status().unwrap().success());
    let audio = source_from(&audio_path, "independent-audio");
    let visual_op = WorkerOperation::Generate {
        preset: "fast".into(),
        model: None,
        settings: (GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            ..Default::default()
        })
        .into(),
    };
    let (visual, _, _) = generated(&request(
        directory.path(),
        "independent-base",
        tools.clone(),
        video.clone(),
        visual_op,
    ));
    let operation = |intent, anchor| WorkerOperation::Generate {
        preset: "fast".into(),
        model: None,
        settings: (GenerationSettings {
            analysis_width: 64,
            analysis_height: 64,
            mixed_audio: Some(MixedAudioSettings {
                intent,
                mapping: AudioMapping::new(
                    Axis::Stroke,
                    0.2,
                    NormalizedPosition::new(0.4).unwrap(),
                )
                .unwrap(),
                independent: Some(IndependentAudio {
                    source_version: audio.source_version.clone(),
                    alignment: AudioAlignment {
                        rate: None,
                        video_anchor: SourceTimestamp::new(0, 1).unwrap(),
                        audio_anchor: anchor,
                    },
                }),
            }),
            ..Default::default()
        })
        .into(),
    };
    let captured = |name, op| {
        let mut r = request(directory.path(), name, tools.clone(), video.clone(), op);
        r.independent_audio = Some(audio.clone());
        r.dependencies.push(audio.identity.clone());
        r
    };
    let r = captured(
        "independent-faithful",
        operation(
            MultimodalIntent::Faithful,
            SourceTimestamp::new(1, 8).unwrap(),
        ),
    );
    let (faithful, receipt, _) = generated(&r);
    assert_eq!(faithful, visual);
    assert_eq!(
        receipt["mixed_audio"]["independent_source"]["identity"]["sha256"],
        audio.identity.sha256
    );
    assert_eq!(
        receipt["samples"][0]["mixed_audio"]["audio_window_index"],
        0
    );
    assert_eq!(
        receipt["samples"][1]["mixed_audio"]["audio_window_index"],
        12
    );
    let r = captured(
        "independent-creative",
        operation(
            MultimodalIntent::CreativeAudio,
            SourceTimestamp::new(1, 8).unwrap(),
        ),
    );
    let (creative, receipt, _) = generated(&r);
    assert_eq!(
        creative
            .track(Axis::Stroke)
            .unwrap()
            .actions()
            .iter()
            .map(|a| a.time())
            .collect::<Vec<_>>(),
        visual
            .track(Axis::Stroke)
            .unwrap()
            .actions()
            .iter()
            .map(|a| a.time())
            .collect::<Vec<_>>()
    );
    assert!(creative
        .track(Axis::Stroke)
        .unwrap()
        .actions()
        .iter()
        .all(|a| (a.position().value() - 0.48).abs() < 1e-8));
    assert!(receipt["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["sha256"] == audio.identity.sha256));
    let r = captured(
        "independent-gap",
        operation(
            MultimodalIntent::CreativeAudio,
            SourceTimestamp::new(0, 1).unwrap(),
        ),
    );
    let (gap, receipt, _) = generated(&r);
    assert_eq!(
        receipt["samples"][0]["mixed_audio"]["audio_window_index"],
        serde_json::Value::Null
    );
    assert!(!gap.track(Axis::Stroke).unwrap().gaps().is_empty());
    // A changed correspondence selects different audio windows while preserving
    // the original video lattice and faithful visual samples.
    for (name, numerator, denominator, second_window, last_window) in [
        ("independent-fast-clock", 2, 1, 25, None),
        ("independent-slow-clock", 1, 2, 6, Some(43)),
    ] {
        let mut op = operation(
            MultimodalIntent::Faithful,
            SourceTimestamp::new(1, 8).unwrap(),
        );
        if let WorkerOperation::Generate { settings, .. } = &mut op {
            settings
                .mixed_audio
                .as_mut()
                .unwrap()
                .independent
                .as_mut()
                .unwrap()
                .alignment
                .rate = Some(AudioClockRate::new(numerator, denominator).unwrap());
        }
        let (motion, receipt, _) = generated(&captured(name, op));
        assert_eq!(motion, visual);
        assert_eq!(
            receipt["mixed_audio"]["settings"]["independent"]["alignment"]["rate"],
            serde_json::json!({"numerator":numerator,"denominator":denominator})
        );
        assert_eq!(
            receipt["samples"][1]["mixed_audio"]["audio_window_index"],
            second_window
        );
        assert_eq!(
            receipt["samples"][7]["mixed_audio"]["audio_window_index"],
            serde_json::to_value(last_window).unwrap()
        );
    }
    let mut r = captured(
        "independent-wrong-identity",
        operation(
            MultimodalIntent::Faithful,
            SourceTimestamp::new(1, 8).unwrap(),
        ),
    );
    r.independent_audio.as_mut().unwrap().identity.sha256 = "0".repeat(64);
    r.dependencies = vec![r.independent_audio.as_ref().unwrap().identity.clone()];
    assert!(execute_manifest(&r).result.is_err());
    assert!(!r.output_dir.join("candidate.json").exists());
    let mut r = captured(
        "independent-foreign-declaration",
        operation(
            MultimodalIntent::Faithful,
            SourceTimestamp::new(1, 8).unwrap(),
        ),
    );
    r.independent_audio.as_mut().unwrap().source_version = SourceVersionId::new("foreign").unwrap();
    assert!(r.validate().is_err());
}

#[test]
#[ignore = "owned authored audio corruption campaign supplies immutable fixtures"]
fn corrupt_audio_refuses_generation_without_publishing_candidate() {
    let inputs = std::path::PathBuf::from(
        std::env::var_os("PULSAR_CORRUPT_AUDIO_CASES").expect("owned fixtures"),
    );
    let tools = discover_tools().expect("native media tools");
    for (filename, should_fail) in [
        ("clean.m4a", false),
        ("clean.flac", false),
        ("clean.mp2", false),
        ("damaged-1.m4a", true),
        ("damaged-3.m4a", true),
        ("damaged-7.m4a", true),
        ("damaged-1.flac", true),
        ("damaged-3.flac", true),
        ("damaged-7.flac", true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(filename);
        std::fs::copy(inputs.join(filename), &path).unwrap();
        let source = source_from(&path, "authored-audio-corruption");
        let input = GenerationInput::Audio {
            source_version: source.source_version.clone(),
            mapping: AudioMapping::new(Axis::Stroke, 0.2, NormalizedPosition::new(0.4).unwrap())
                .unwrap(),
            mode: AudioMode::Envelope,
            analysis: None,
        };
        let request = request(
            directory.path(),
            "audio-corruption-attempt",
            tools.clone(),
            source,
            WorkerOperation::GenerateInput { input },
        );
        let response = execute_manifest(&request).result;
        if should_fail {
            assert!(
                response.is_err(),
                "Corrupt {filename} returned a successful-looking candidate"
            );
            assert_eq!(
                std::fs::read_dir(&request.output_dir).unwrap().count(),
                0,
                "Corrupt audio published an artifact"
            );
            eprintln!(
                "authored audio {filename}: generation_failure=true, error={:?}, qualified=false",
                response.err()
            );
        } else {
            let WorkerOutput::Generated {
                program, receipt, ..
            } = response.unwrap()
            else {
                panic!("expected generated audio candidate")
            };
            let program: MotionProgram =
                serde_json::from_slice(&std::fs::read(program.path).unwrap()).unwrap();
            assert!(!program.track(Axis::Stroke).unwrap().actions().is_empty());
            let receipt: serde_json::Value =
                serde_json::from_slice(&std::fs::read(receipt.path).unwrap()).unwrap();
            assert!(
                receipt["details"]["resampled_sample_count"]
                    .as_u64()
                    .unwrap()
                    >= 32000
            );
            eprintln!(
                "authored audio {filename}: generation_failure=false, samples={}, qualified=false",
                receipt["details"]["resampled_sample_count"]
            );
        }
    }
}

#[test]
fn audio_bundle_keeps_one_source_clock_and_explicit_per_axis_analysis() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let source = wav_fixture(directory.path(), &tools);
    let channels = Axis::ALL
        .iter()
        .enumerate()
        .map(|(i, axis)| AudioChannelRecipe {
            mapping: AudioMapping::new(*axis, 0.2, NormalizedPosition::new(0.4).unwrap()).unwrap(),
            mode: if i == 1 {
                AudioMode::Vibe
            } else if i == 2 {
                AudioMode::Beats
            } else {
                AudioMode::Envelope
            },
            analysis: AudioAnalysisOptions::new(
                if i == 0 {
                    AudioBand::High
                } else {
                    AudioBand::Full
                },
                0.05,
            )
            .unwrap(),
        })
        .collect();
    let input = GenerationInput::AudioBundle {
        source_version: source.source_version.clone(),
        recipe: AudioBundleRecipe::new(channels).unwrap(),
    };
    let request = request(
        directory.path(),
        "audio-bundle-attempt",
        tools,
        source,
        WorkerOperation::GenerateInput { input },
    );
    let WorkerOutput::Generated {
        program, receipt, ..
    } = execute_manifest(&request).result.unwrap()
    else {
        panic!("expected audio bundle")
    };
    let program: MotionProgram =
        serde_json::from_slice(&std::fs::read(program.path).unwrap()).unwrap();
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt.path).unwrap()).unwrap();
    assert_eq!(program.tracks().len(), 6);
    assert_eq!(receipt["recipe"], "explicit-six-axis-audio-v1");
    assert_eq!(receipt["details"]["decode_count"], 1);
    assert_eq!(receipt["details"]["channels"].as_array().unwrap().len(), 6);
    assert_eq!(receipt["qualification"], "unqualified");
    for track in program.tracks() {
        assert!(!track.actions().is_empty());
        assert_eq!(
            track.actions().last().unwrap().time().as_nanos(),
            120_000_000
        );
        assert!(track
            .actions()
            .iter()
            .all(|action| action.evidence() == EvidenceKind::Synthesized));
    }
}
