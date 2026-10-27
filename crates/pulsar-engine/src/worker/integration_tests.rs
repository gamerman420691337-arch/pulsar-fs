//! Real decoder/process fixtures. These tests qualify interface behavior only,
//! not a detector's real-media quality or a production sandbox configuration.

use super::*;
use std::path::Path;
use std::process::Command;

fn fixture() -> (tempfile::TempDir, WorkerTools, SourceArtifact) {
    fixture_with_shifts(&[0, 1, 2, 3, 4, 5, 6, 7], 1)
}
fn fixture_with_shifts(
    shifts: &[usize; 8],
    contrast_divisor: u8,
) -> (tempfile::TempDir, WorkerTools, SourceArtifact) {
    let directory = tempfile::tempdir().unwrap();
    let tools =
        discover_tools().expect("architecture media tests require declared ffmpeg/ffprobe tools");
    let raw = directory.path().join("pixels.rgb");
    let mut bytes = Vec::new();
    for &shift in shifts {
        for y in 0..64usize {
            for x in 0..64usize {
                let shifted_y = (y + 64 - shift) % 64;
                let value =
                    ((x * 31 + shifted_y * 17 + (x / 7) * (shifted_y / 5) * 13) % 256) as u8;
                let value = value / contrast_divisor;
                bytes.extend_from_slice(&[value, value, value]);
            }
        }
    }
    std::fs::write(&raw, bytes).unwrap();
    let video = directory.path().join("moving.mkv");
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
        .args(["-frames:v", "8", "-threads", "1", "-c:v", "ffv1"])
        .arg(&video);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    let pinned = pin_artifact(&video).unwrap();
    let source = SourceArtifact {
        source_version: SourceVersionId::new("fixture-source").unwrap(),
        path: pinned.path,
        identity: pinned.identity,
    };
    (directory, tools, source)
}

fn manifest(
    directory: &Path,
    tools: WorkerTools,
    source: SourceArtifact,
    operation: WorkerOperation,
) -> WorkerRequest {
    let output_dir = directory.join("attempt");
    std::fs::create_dir(&output_dir).unwrap();
    WorkerRequest {
        cuda_execution: None,
        independent_audio: None,
        version: PROTOCOL_VERSION,
        attempt_id: AttemptId::new("fixture-attempt").unwrap(),
        job_id: JobId::new("fixture-job").unwrap(),
        project_id: ProjectId::new("fixture-project").unwrap(),
        base_revision: RevisionId::new(0),
        source,
        output_dir: output_dir.canonicalize().unwrap(),
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

fn context(source: &SourceArtifact) -> FrameContext {
    FrameContext {
        source_version: source.source_version.clone(),
        source_placement: SourcePlacementId::new("primary-fixture-source").unwrap(),
        frame: FrameId::new(999),
        transform: TransformId::new("source-identity").unwrap(),
        seek_generation: 7,
        request_generation: 12,
    }
}

#[test]
fn packed_source_decoder_ignores_display_rotation_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().expect("native orientation fixture requires ffmpeg/ffprobe");
    let plain = directory.path().join("plain.mp4");
    let rotated = directory.path().join("rotated.mp4");
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut encode = Command::new(&tools.ffmpeg.path);
    encode
        .args([
            "-v",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=96x64:rate=2:duration=2",
            "-c:v",
            "mpeg4",
            "-pix_fmt",
            "yuv420p",
            "-threads",
            "1",
        ])
        .arg(&plain);
    media::bounded_output(encode, 1024, deadline).unwrap();
    assert_rotation_decode(&tools, &plain, &rotated, deadline, "mpeg4");
}

#[test]
fn packed_h264_source_decoder_ignores_display_rotation_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().expect("native orientation fixture requires ffmpeg/ffprobe");
    let plain = directory.path().join("plain.mp4");
    let rotated = directory.path().join("rotated.mp4");
    std::fs::write(
        &plain,
        include_bytes!("../../tests/fixtures/rotation-h264.mp4"),
    )
    .unwrap();
    assert_rotation_decode(
        &tools,
        &plain,
        &rotated,
        Instant::now() + Duration::from_secs(20),
        "h264",
    );
}

fn assert_rotation_decode(
    tools: &WorkerTools,
    plain: &Path,
    rotated: &Path,
    deadline: Instant,
    codec: &str,
) {
    let mut remux = Command::new(&tools.ffmpeg.path);
    remux
        .args(["-v", "error", "-nostdin", "-display_rotation", "90", "-i"])
        .arg(plain)
        .args(["-c", "copy"])
        .arg(rotated);
    media::bounded_output(remux, 1024, deadline).unwrap();
    let mut probe = Command::new(&tools.ffprobe.path);
    probe
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(rotated);
    let metadata: serde_json::Value =
        serde_json::from_slice(&media::bounded_output(probe, 64 * 1024, deadline).unwrap())
            .unwrap();
    assert_eq!(metadata["streams"][0]["codec_name"], codec);
    assert_eq!(metadata["streams"][0]["width"], 96);
    assert_eq!(metadata["streams"][0]["height"], 64);
    assert!(metadata["streams"][0]["side_data_list"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["rotation"]
            .as_i64()
            .is_some_and(|angle| angle.abs() == 90)));
    let decode = |source: &Path| {
        let mut decoder = media::Decoder::new_filtered(
            &tools.ffmpeg.path,
            source,
            media::FrameTransform {
                width: 64,
                height: 64,
                filter: "scale=64:64:flags=neighbor",
            },
            1,
            1,
            deadline,
        )
        .unwrap();
        let mut frames = Vec::new();
        while let Some(frame) = decoder.next().unwrap() {
            frames.push((frame.source_time, frame.pixels));
        }
        frames
    };
    let expected = decode(plain);
    let actual = decode(rotated);
    assert_eq!(expected.len(), 4);
    for (index, (time, pixels)) in expected.iter().enumerate() {
        // Independent fixture clock: two frames/second starting at zero.
        assert_eq!(
            time.numerator() * 2,
            index as i64 * i64::from(time.denominator())
        );
        assert_eq!(pixels.len(), 64 * 64 * 3);
    }
    assert!(expected.windows(2).any(|pair| pair[0].1 != pair[1].1));
    for ((expected_time, expected_pixels), (actual_time, actual_pixels)) in
        expected.iter().zip(&actual)
    {
        assert_eq!(expected_time, actual_time);
        assert!(
            expected_pixels == actual_pixels,
            "metadata changed packed-source pixels"
        );
    }
    assert_eq!(expected.len(), actual.len());
}

#[test]
fn source_clock_mapping_restores_native_sample_pts_without_rewriting_placed_candidate() {
    let (directory, tools, source) = fixture();
    let placement = SourcePlacement {
        placement_id: SourcePlacementId::new("placement.source-export").unwrap(),
        created_revision: RevisionId::new(0),
        spec: SourcePlacementSpec {
            source_version: source.source_version.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(875_000_001, 1_000_000_000).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(2_000_000_001),
            rate_numerator: 3,
            rate_denominator: 1,
        },
    };
    let request = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Generate {
            preset: "fast".into(),
            settings: (GenerationSettings {
                source_placement: Some(placement.clone()),
                analysis_width: 64,
                analysis_height: 64,
                ..Default::default()
            })
            .into(),
            model: None,
        },
    );
    let WorkerOutput::Generated {
        mut program,
        mut receipt,
        ..
    } = execute_manifest(&request).result.unwrap()
    else {
        panic!("generated")
    };
    // The engine egress broker fixes these names independently of worker paths.
    let broker_program = request.output_dir.join("program.json");
    let broker_receipt = request.output_dir.join("receipt.json");
    std::fs::rename(&program.path, &broker_program).unwrap();
    program.path = broker_program;
    std::fs::rename(&receipt.path, &broker_receipt).unwrap();
    receipt.path = broker_receipt;
    let original_bytes = std::fs::read(&program.path).unwrap();
    let original: MotionProgram = serde_json::from_slice(&original_bytes).unwrap();
    let receipt_bytes = std::fs::read(&receipt.path).unwrap();
    let report: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
    let plan: pulsar_core::PlacedGenerationPlan =
        serde_json::from_value(report["placed_generation_plan"].clone()).unwrap();
    let retained = vec![receipt.identity.clone()];
    let canonical_bytes = serde_json::to_vec(&original).unwrap();
    let canonical_hash = format!("{:x}", Sha256::digest(&canonical_bytes));
    let canonical = ArtifactIdentity {
        artifact_id: ArtifactId::new(format!("motion.{canonical_hash}")).unwrap(),
        sha256: canonical_hash,
        byte_len: canonical_bytes.len() as u64,
    };
    let load = |request: &WorkerRequest, identities: &[ArtifactIdentity]| {
        crate::source_clock_receipts::load(request, identities, &canonical)
    };
    assert_eq!(load(&request, &retained).unwrap(), plan);
    // A reader can grow after its size was observed. Receipt parsing must not
    // consume the general output budget in whitespace beyond that observed size.
    struct CountedReader {
        bytes: std::io::Cursor<Vec<u8>>,
        count: std::rc::Rc<std::cell::Cell<usize>>,
    }
    impl std::io::Read for CountedReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let n = std::io::Read::read(&mut self.bytes, buffer)?;
            self.count.set(self.count.get() + n);
            Ok(n)
        }
    }
    let mut grown = receipt_bytes.clone();
    grown.extend(std::iter::repeat_n(b' ', 64 * 1024));
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    let reader = CountedReader {
        bytes: std::io::Cursor::new(grown),
        count: count.clone(),
    };
    assert!(crate::source_clock_receipts::load_verified_readers(
        &request,
        &retained,
        &canonical,
        reader,
        receipt_bytes.len() as u64,
        |_, _| Ok(Box::new(std::io::Cursor::new(original_bytes.clone())))
    )
    .is_err());
    assert!(
        count.get() <= receipt_bytes.len() + 8192,
        "receipt consumed {} bytes after an observed {} byte size",
        count.get(),
        receipt_bytes.len()
    );
    let count = std::rc::Rc::new(std::cell::Cell::new(0));
    assert!(crate::source_clock_receipts::load_verified_readers(
        &request,
        &retained,
        &canonical,
        std::io::Cursor::new(receipt_bytes.clone()),
        receipt_bytes.len() as u64,
        |_, _| {
            let mut grown = original_bytes.clone();
            grown.extend(std::iter::repeat_n(b' ', 64 * 1024));
            Ok(Box::new(CountedReader {
                bytes: std::io::Cursor::new(grown),
                count: count.clone(),
            }))
        }
    )
    .is_err());
    assert!(
        count.get() <= original_bytes.len() + 8192,
        "program consumed {} bytes after an observed {} byte size",
        count.get(),
        original_bytes.len()
    );
    crate::authority::source_clock_origins::verify_native_fixture(
        directory.path(),
        &request,
        &original,
        &plan,
        &retained,
        &receipt.identity,
        &canonical,
    );
    let mut wrong_canonical = canonical.clone();
    wrong_canonical.byte_len += 1;
    assert!(crate::source_clock_receipts::load(&request, &retained, &wrong_canonical).is_err());
    assert!(load(&request, &[]).is_err());
    assert!(load(
        &request,
        &[receipt.identity.clone(), receipt.identity.clone()]
    )
    .is_err());
    // Even an identity updated to match altered bytes cannot replace the
    // originating attempt/configuration/placement meaning.
    for key in [
        "program",
        "schema_version",
        "project_id",
        "base_revision",
        "job_id",
        "attempt_id",
        "source_version",
        "configuration_sha256",
        "preset",
        "source_placement",
        "configuration",
    ] {
        let mut changed = report.clone();
        changed[key] = match key {
            "schema_version" => serde_json::json!(3),
            "program" => {
                let mut id = program.identity.clone();
                id.byte_len += 1;
                serde_json::to_value(id).unwrap()
            }
            "base_revision" => serde_json::to_value(RevisionId::new(1)).unwrap(),
            "source_placement" => {
                let mut p = placement.clone();
                p.spec.project_anchor = ProjectTime::from_nanos(2_000_000_002);
                serde_json::to_value(p).unwrap()
            }
            "configuration" => {
                let mut s = if let WorkerOperation::Generate { settings, .. } = &request.operation {
                    settings.clone()
                } else {
                    unreachable!()
                };
                s.source_sample_stride = 2;
                serde_json::to_value(s).unwrap()
            }
            _ => serde_json::json!("wrong-origin"),
        };
        std::fs::write(&receipt.path, serde_json::to_vec(&changed).unwrap()).unwrap();
        let changed_identity = pin_artifact(&receipt.path).unwrap().identity;
        assert!(load(&request, &[changed_identity]).is_err(), "{key}");
        assert!(load(&request, &retained).is_err());
    }
    let mut trailing = receipt_bytes.clone();
    trailing.extend_from_slice(b" {}");
    std::fs::write(&receipt.path, &trailing).unwrap();
    assert!(load(&request, &[pin_artifact(&receipt.path).unwrap().identity]).is_err());
    std::fs::write(&receipt.path, &receipt_bytes).unwrap();
    assert_eq!(load(&request, &retained).unwrap(), plan);
    let mut reduced = request.clone();
    reduced.budget.output_bytes = receipt_bytes.len() as u64 - 1;
    assert!(load(&reduced, &retained).is_err());
    reduced = request.clone();
    reduced.budget.memory_bytes = 1024;
    assert!(load(&reduced, &retained).is_err());
    let mut wrong_size = receipt.identity.clone();
    wrong_size.byte_len += 1;
    assert!(load(&request, &[wrong_size]).is_err());
    #[cfg(unix)]
    {
        let backing = receipt.path.with_extension("saved");
        std::fs::rename(&receipt.path, &backing).unwrap();
        std::os::unix::fs::symlink(&backing, &receipt.path).unwrap();
        assert!(load(&request, &retained).is_err());
        std::fs::remove_file(&receipt.path).unwrap();
        std::fs::rename(backing, &receipt.path).unwrap();
    }
    assert!(plan.uncovered_boundaries.is_empty());
    let track = original.track(Axis::Stroke).unwrap();
    assert!(track.gaps().is_empty());
    let mapped =
        pulsar_core::map_track_to_source_clock(track, &placement.spec, Some(&plan), 100).unwrap();
    assert_eq!(mapped.track.actions().len(), 8);
    assert_eq!(mapped.reused_sample_clocks, 8);
    for ((current, source), mapping) in track
        .actions()
        .iter()
        .zip(mapped.track.actions())
        .zip(&mapped.mappings)
    {
        let frame = mapping.original_source_frame.unwrap();
        assert_eq!(
            source.time().as_nanos(),
            i64::try_from(frame).unwrap() * 125_000_000
        );
        assert_eq!(
            mapping.source_time,
            plan.samples[frame as usize].elapsed_source_time
        );
        assert_eq!(current.position(), source.position());
        assert_eq!(current.evidence(), source.evidence());
    }
    assert_ne!(
        placement
            .spec
            .source_at_project(track.actions()[1].time())
            .unwrap(),
        mapped.mappings[1].source_time
    );
    assert_eq!(std::fs::read(&program.path).unwrap(), original_bytes);
    std::fs::write(&program.path, b"{}").unwrap();
    assert!(load(&request, &retained).is_err());
    std::fs::write(&program.path, &original_bytes).unwrap();
    assert_eq!(load(&request, &retained).unwrap(), plan);
    let mut formatted = b" \n".to_vec();
    formatted.extend_from_slice(&original_bytes);
    formatted.extend_from_slice(b"\n ");
    std::fs::write(&program.path, &formatted).unwrap();
    let mut rebound = report.clone();
    rebound["program"] =
        serde_json::to_value(pin_artifact(&program.path).unwrap().identity).unwrap();
    std::fs::write(&receipt.path, serde_json::to_vec(&rebound).unwrap()).unwrap();
    let formatted_receipt = pin_artifact(&receipt.path).unwrap().identity;
    assert_eq!(load(&request, &[formatted_receipt]).unwrap(), plan);
    std::fs::write(&program.path, &original_bytes).unwrap();
    std::fs::write(&receipt.path, &receipt_bytes).unwrap();
    assert_eq!(load(&request, &retained).unwrap(), plan);
}

#[test]
fn placed_generation_preserves_source_observations_and_reverses_project_samples() {
    let mut outputs = Vec::new();
    for rate in [1, -1] {
        let (directory, tools, source) = fixture();
        let placement = SourcePlacement {
            placement_id: SourcePlacementId::new("placement.generation").unwrap(),
            created_revision: RevisionId::new(0),
            spec: SourcePlacementSpec {
                source_version: source.source_version.clone(),
                source_start: SourceTimestamp::new(1, 8).unwrap(),
                source_end: SourceTimestamp::new(7, 8).unwrap(),
                source_anchor: SourceTimestamp::new(1, 8).unwrap(),
                project_anchor: ProjectTime::from_nanos(if rate > 0 {
                    1_000_000_000
                } else {
                    2_000_000_000
                }),
                rate_numerator: rate,
                rate_denominator: 1,
            },
        };
        let mut request = manifest(
            directory.path(),
            tools,
            source,
            WorkerOperation::Generate {
                preset: "fast".into(),
                settings: (GenerationSettings {
                    source_placement: Some(placement.clone()),
                    analysis_width: 64,
                    analysis_height: 64,
                    ..Default::default()
                })
                .into(),
                model: None,
            },
        );
        let original = request.clone();
        if let WorkerOperation::Generate { settings, .. } = &mut request.operation {
            settings.source_placement.as_mut().unwrap().created_revision = RevisionId::new(1);
        }
        assert!(request.validate().is_err());
        let output = execute_manifest(&original).result.unwrap();
        let WorkerOutput::Generated {
            program, receipt, ..
        } = output
        else {
            panic!("generated")
        };
        let program: MotionProgram =
            serde_json::from_slice(&std::fs::read(program.path).unwrap()).unwrap();
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(receipt.path).unwrap()).unwrap();
        assert_eq!(receipt["schema_version"], 2);
        let plan: pulsar_core::PlacedGenerationPlan =
            serde_json::from_value(receipt["placed_generation_plan"].clone()).unwrap();
        assert_eq!(plan.placement, placement);
        assert_eq!(
            plan.samples
                .iter()
                .map(|s| s.source_frame_index)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6]
        );
        let samples = receipt["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 6);
        for (sample, planned) in samples.iter().zip(&plan.samples) {
            assert_eq!(sample["source_frame_index"], planned.source_frame_index);
            assert_eq!(
                sample["project_time_ns"],
                planned.quantization.time.as_nanos()
            );
            assert!(sample.get("quantization_remainder").is_none());
            assert_eq!(
                serde_json::from_value::<pulsar_core::PlacedGenerationSample>(
                    sample["placed"].clone()
                )
                .unwrap(),
                *planned
            );
        }
        let track = program.track(Axis::Stroke).unwrap();
        assert!(!track.actions().is_empty());
        for boundary in &plan.uncovered_boundaries {
            assert!(track
                .gaps()
                .iter()
                .any(|g| g.start() <= boundary.start() && g.end() >= boundary.end()));
        }
        outputs.push(program);
    }
    let forward = outputs[0].track(Axis::Stroke).unwrap().actions();
    let reverse = outputs[1].track(Axis::Stroke).unwrap().actions();
    assert_eq!(forward.len(), reverse.len());
    for (a, b) in forward.iter().zip(reverse.iter().rev()) {
        assert_eq!(a.time().as_nanos() + b.time().as_nanos(), 3_000_000_000);
        assert_eq!(a.position(), b.position());
        assert_eq!(a.evidence(), b.evidence());
    }
}

#[test]
fn actual_decode_preview_binds_frame_pts_and_reports_missing_model() {
    let (directory, tools, source) = fixture();
    let requested = context(&source);
    let manifest = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Preview {
            still_image: false,
            annotated: None,
            placement_seek: None,
            time_basis: PreviewTimeBasis::SourcePts,
            video_view: None,
            context: requested.clone(),
            source_time: SourceTimestamp::new(3, 8).unwrap(),
            max_width: 48,
            max_height: 48,
            model: None,
            model_input: None,
            inference_profile: None,
        },
    );
    let output = execute_manifest(&manifest).result.unwrap();
    let WorkerOutput::Preview(result) = output else {
        panic!("expected preview");
    };
    assert!(result.matches_seek_request(&requested, &SourceTimestamp::new(3, 8).unwrap()));
    assert!(!result.matches_seek_request(&requested, &SourceTimestamp::new(4, 8).unwrap()));
    assert_eq!(result.context.frame, FrameId::new(3));
    assert_eq!(result.source_frame_index, 3);
    assert_eq!(
        result.source_time.numerator() * 8,
        i64::from(result.source_time.denominator()) * 3
    );
    assert!(result.observations.is_empty());
    assert!(matches!(
        result.analysis,
        AnalysisStatus::Unavailable { .. }
    ));
    let frame = result.frame.unwrap();
    frame.validate(manifest.budget.output_bytes).unwrap();
    let pixels = std::fs::read(&frame.artifact.path).unwrap();
    assert_eq!(pixels.len(), 48 * 48 * 3);
    assert!(pixels.windows(2).any(|window| window[0] != window[1]));
    artifacts::verify_artifact(
        &frame.artifact.path,
        &frame.artifact.identity,
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
}

#[cfg(unix)]
#[test]
fn snapshot_preview_refuses_changed_decode_clock_before_artifact_publication() {
    use std::os::unix::fs::PermissionsExt;
    let (directory, tools, source) = fixture();
    let mut control_pixels = None;
    for shifted in [false, true] {
        let root = directory
            .path()
            .join(if shifted { "shifted" } else { "unchanged" });
        std::fs::create_dir(&root).unwrap();
        let proxy = root.join("authored-ffmpeg-proxy");
        let real_tool = serde_json::to_string(tools.ffmpeg.path.to_str().unwrap()).unwrap();
        let filter = if shifted {
            ",setpts=PTS+1/TB"
        } else {
            ",setpts=PTS"
        };
        std::fs::write(&proxy, format!("#!/usr/bin/python3\nimport os,sys\nargs=[{real_tool}]+sys.argv[1:]\ni=args.index('-vf')+1\nargs[i]+={filter:?}\nos.execv(args[0],args)\n")).unwrap();
        std::fs::set_permissions(&proxy, std::fs::Permissions::from_mode(0o500)).unwrap();
        let mut selected_tools = tools.clone();
        selected_tools.ffmpeg = pin_artifact(&proxy).unwrap();
        let request = manifest(
            &root,
            selected_tools,
            source.clone(),
            WorkerOperation::Preview {
                still_image: false,
                annotated: None,
                placement_seek: None,
                time_basis: PreviewTimeBasis::SourcePts,
                video_view: None,
                context: context(&source),
                source_time: SourceTimestamp::new(3, 8).unwrap(),
                max_width: 48,
                max_height: 48,
                model: None,
                model_input: None,
                inference_profile: None,
            },
        );
        if shifted {
            let pixels = media::extract_filtered(
                &request.tools.ffmpeg.path,
                &source.path,
                media::SnapshotFrame {
                    index: 3,
                    source_time: SourceTimestamp::new(11, 8).unwrap(),
                },
                media::FrameTransform {
                    width: 48,
                    height: 48,
                    filter: "scale=48:48",
                },
                1,
                Instant::now() + Duration::from_secs(20),
            )
            .unwrap();
            assert_eq!(
                Some(pixels),
                control_pixels,
                "timestamp fault changed frame pixels"
            );
        }
        let result = execute_manifest(&request).result;
        if shifted {
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("decoded snapshot timestamp differs from indexed source frame"),
                "{error}"
            );
            assert_eq!(std::fs::read_dir(&request.output_dir).unwrap().count(), 0);
        } else {
            let WorkerOutput::Preview(preview) = result.unwrap() else {
                panic!("preview expected")
            };
            assert_eq!(preview.source_frame_index, 3);
            control_pixels = Some(std::fs::read(preview.frame.unwrap().artifact.path).unwrap());
        }
    }
}

#[test]
fn actual_pixel_generation_returns_checked_candidate_and_lineage() {
    let (directory, tools, source) = fixture();
    let manifest = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Generate {
            preset: "fast".into(),
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
    let output = execute_manifest(&manifest).result.unwrap();
    let WorkerOutput::Generated {
        program,
        receipt,
        lineage,
        ..
    } = output
    else {
        panic!("expected candidate");
    };
    let program: MotionProgram =
        serde_json::from_slice(&std::fs::read(&program.path).unwrap()).unwrap();
    let track = program.track(Axis::Stroke).unwrap();
    assert!(
        track.actions().len() >= 2,
        "moving textured source must yield actual motion evidence"
    );
    assert!(track
        .actions()
        .iter()
        .any(|action| action.evidence() == EvidenceKind::Inferred));
    assert!(track
        .actions()
        .iter()
        .all(|action| action.evidence() != EvidenceKind::Observed));
    assert!(track
        .actions()
        .iter()
        .all(|action| action.position().value() > 0.0 && action.position().value() < 1.0));
    assert!(lineage.contains(&manifest.source.identity));
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&receipt.path).unwrap()).unwrap();
    assert_eq!(receipt["samples"].as_array().unwrap().len(), 8);
    assert_eq!(receipt["qualification"], "unqualified");
    assert_eq!(receipt["source_origin_pts"], 0);
}

#[test]
fn source_byte_replacement_rejected_before_native_execution() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.bin");
    std::fs::write(&path, b"original").unwrap();
    let reference = pin_artifact(&path).unwrap();
    let source = SourceArtifact {
        source_version: SourceVersionId::new("source").unwrap(),
        path: reference.path.clone(),
        identity: reference.identity.clone(),
    };
    let tools = WorkerTools {
        ffmpeg: reference.clone(),
        ffprobe: reference,
        onnx_runtime: None,
    };
    let manifest = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Generate {
            preset: "default".into(),
            settings: Default::default(),
            model: None,
        },
    );
    std::fs::write(&path, b"replaced").unwrap();
    assert_eq!(
        execute_manifest(&manifest).result.unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
}

#[test]
fn artifact_budget_failure_is_typed_resource_exhaustion() {
    let (directory, tools, source) = fixture();
    let request_context = context(&source);
    let mut manifest = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Preview {
            still_image: false,
            annotated: None,
            placement_seek: None,
            time_basis: PreviewTimeBasis::SourcePts,
            video_view: None,
            context: request_context,
            source_time: SourceTimestamp::new(0, 1).unwrap(),
            max_width: 48,
            max_height: 48,
            model: None,
            model_input: None,
            inference_profile: None,
        },
    );
    manifest.budget.output_bytes = 8;
    assert_eq!(
        execute_manifest(&manifest).result.unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
}

fn projected_fixture(half: bool) -> (tempfile::TempDir, WorkerTools, SourceArtifact) {
    let directory = tempfile::tempdir().unwrap();
    let tools = discover_tools().unwrap();
    let eye_width = if half { 64 } else { 128 };
    let height = 64;
    let width = eye_width * 2;
    let mut pixels = Vec::new();
    for frame in 0..8 {
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[
                    ((x % eye_width) * 255 / (eye_width - 1)) as u8,
                    (y * 255 / (height - 1)) as u8,
                    ((x / eye_width) * 100 + frame) as u8,
                ]);
            }
        }
    }
    let raw = directory.path().join("projection.rgb");
    std::fs::write(&raw, pixels).unwrap();
    let video = directory.path().join("projection.mkv");
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
            &format!("{width}x{height}"),
            "-framerate",
            "8",
            "-i",
        ])
        .arg(raw)
        .args([
            "-frames:v",
            "8",
            "-threads",
            "1",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "bgr0",
        ])
        .arg(&video);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    let pinned = pin_artifact(&video).unwrap();
    let source = SourceArtifact {
        source_version: SourceVersionId::new("projection-source").unwrap(),
        path: pinned.path,
        identity: pinned.identity,
    };
    (directory, tools, source)
}
fn view(half: bool, eye: StereoEye, yaw: f64) -> VideoView {
    let viewport = PerspectiveView {
        yaw_degrees: yaw,
        pitch_degrees: 0.0,
        roll_degrees: 0.0,
        horizontal_fov_degrees: 90.0,
        vertical_fov_degrees: 90.0,
    };
    VideoView {
        layout: StereoLayout::SideBySideLeftRight,
        eye,
        projection: if half {
            VideoProjection::HalfEquirectangular180(viewport)
        } else {
            VideoProjection::Equirectangular360(viewport)
        },
    }
}
fn projected_preview(
    directory: &Path,
    tools: WorkerTools,
    source: SourceArtifact,
    view: VideoView,
) -> PreviewResult {
    let mut context = context(&source);
    context.transform = video_view_transform(Some(&view)).unwrap();
    let request = manifest(
        directory,
        tools,
        source,
        WorkerOperation::Preview {
            still_image: false,
            annotated: None,
            placement_seek: None,
            time_basis: PreviewTimeBasis::SourcePts,
            video_view: Some(view),
            context,
            source_time: SourceTimestamp::new(3, 8).unwrap(),
            max_width: 64,
            max_height: 64,
            model: None,
            model_input: None,
            inference_profile: None,
        },
    );
    let WorkerOutput::Preview(result) = execute_manifest(&request).result.unwrap() else {
        panic!("projected preview expected")
    };
    *result
}
#[test]
fn projected_360_preview_selects_eye_rotates_longitude_and_preserves_source_time() {
    for (eye, yaw, expected_red, expected_blue) in [
        (StereoEye::Left, 0.0, 128, 3),
        (StereoEye::Right, 90.0, 192, 103),
    ] {
        let (directory, tools, source) = projected_fixture(false);
        let selected = view(false, eye, yaw);
        let transform = video_view_transform(Some(&selected)).unwrap();
        let result = projected_preview(directory.path(), tools, source, selected);
        assert_eq!(result.context.transform, transform);
        assert_eq!(result.source_frame_index, 3);
        assert_eq!(
            result.source_time.numerator() * 8,
            i64::from(result.source_time.denominator()) * 3
        );
        let frame = result.frame.unwrap();
        assert_eq!((frame.width, frame.height), (64, 64));
        let pixels = std::fs::read(frame.artifact.path).unwrap();
        let center = &pixels[(32 * 64 + 32) * 3..][..3];
        assert!(
            (i32::from(center[0]) - expected_red).abs() <= 5,
            "independent longitude ramp: {center:?}"
        );
        assert!((i32::from(center[1]) - 128).abs() <= 5);
        assert!(
            (i32::from(center[2]) - expected_blue).abs() <= 1,
            "the other eye must not leak into the selected view"
        );
        assert!(result.observations.is_empty());
        assert!(matches!(
            result.analysis,
            AnalysisStatus::Unavailable { .. }
        ));
    }
}
#[test]
fn projected_vr180_preview_uses_half_equirectangular_family_and_exact_eye() {
    let (directory, tools, source) = projected_fixture(true);
    let result = projected_preview(
        directory.path(),
        tools,
        source,
        view(true, StereoEye::Right, 0.0),
    );
    let pixels = std::fs::read(result.frame.unwrap().artifact.path).unwrap();
    let center = &pixels[(32 * 64 + 32) * 3..][..3];
    assert!((i32::from(center[0]) - 128).abs() <= 5);
    assert!((i32::from(center[1]) - 128).abs() <= 5);
    assert_eq!(center[2], 103);
}
#[test]
fn projected_generation_records_view_identity_without_inventing_observed_motion() {
    let (directory, tools, source) = projected_fixture(false);
    let selected = view(false, StereoEye::Right, 30.0);
    let transform = video_view_transform(Some(&selected)).unwrap();
    let request = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Generate {
            preset: "fast".into(),
            settings: (GenerationSettings {
                video_view: Some(selected),
                analysis_width: 64,
                analysis_height: 64,
                ..Default::default()
            })
            .into(),
            model: None,
        },
    );
    let WorkerOutput::Generated {
        program,
        receipt: report,
        ..
    } = execute_manifest(&request).result.unwrap()
    else {
        panic!("candidate expected")
    };
    let program: MotionProgram =
        serde_json::from_slice(&std::fs::read(program.path).unwrap()).unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(report.path).unwrap()).unwrap();
    assert_eq!(report["coordinate_projection"], transform.as_str());
    assert_eq!(report["source_dimensions"], serde_json::json!([256, 64]));
    let samples = report["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 8);
    assert_eq!(samples[3]["source_frame_index"], 3);
    assert_eq!(samples[3]["project_time_ns"], 375_000_000);
    assert_eq!(report["qualification"], "unqualified");
    assert!(program
        .track(Axis::Stroke)
        .unwrap()
        .actions()
        .iter()
        .all(|action| action.evidence() != EvidenceKind::Observed));
}
#[test]
fn projected_preview_refuses_mismatched_transform_before_native_execution() {
    let (directory, tools, source) = projected_fixture(false);
    let request = manifest(
        directory.path(),
        tools,
        source.clone(),
        WorkerOperation::Preview {
            still_image: false,
            annotated: None,
            placement_seek: None,
            time_basis: PreviewTimeBasis::SourcePts,
            video_view: Some(view(false, StereoEye::Left, 0.0)),
            context: context(&source),
            source_time: SourceTimestamp::new(0, 1).unwrap(),
            max_width: 64,
            max_height: 64,
            model: None,
            model_input: None,
            inference_profile: None,
        },
    );
    assert_eq!(
        request.validate().unwrap_err().code,
        ErrorCode::InvalidRequest
    );
}

#[test]
fn projected_360_seam_wraps_inside_selected_eye_instead_of_reading_its_neighbor() {
    let (directory, tools, source) = projected_fixture(false);
    let result = projected_preview(
        directory.path(),
        tools,
        source,
        view(false, StereoEye::Right, 180.0),
    );
    let pixels = std::fs::read(result.frame.unwrap().artifact.path).unwrap();
    let left = &pixels[(32 * 64 + 28) * 3..][..3];
    let right = &pixels[(32 * 64 + 35) * 3..][..3];
    assert!(
        left[0] > 230 && right[0] < 25,
        "declared longitude seam: {left:?} / {right:?}"
    );
    assert_eq!(left[2], 103);
    assert_eq!(right[2], 103);
}
#[test]
fn projected_flat_stereo_respects_declared_reversed_eye_order() {
    let (directory, tools, source) = projected_fixture(false);
    let selected = VideoView {
        layout: StereoLayout::SideBySideRightLeft,
        eye: StereoEye::Right,
        projection: VideoProjection::Flat,
    };
    let result = projected_preview(directory.path(), tools, source, selected);
    let frame = result.frame.unwrap();
    assert_eq!((frame.width, frame.height), (64, 32));
    let pixels = std::fs::read(frame.artifact.path).unwrap();
    assert_eq!(
        pixels[(16 * 64 + 32) * 3 + 2],
        3,
        "explicitly reversed eye order selects the first packed eye"
    );
}

#[test]
fn coordinate_maps_match_native_rotated_sampling_centers_and_refuse_hidden_points() {
    for half in [false, true] {
        let (directory, tools, source) = projected_fixture(half);
        let mut selected = view(half, StereoEye::Right, if half { 15.0 } else { 120.0 });
        let (VideoProjection::Equirectangular360(v) | VideoProjection::HalfEquirectangular180(v)) =
            &mut selected.projection
        else {
            unreachable!()
        };
        v.pitch_degrees = -12.0;
        v.roll_degrees = 27.0;
        v.horizontal_fov_degrees = 50.0;
        v.vertical_fov_degrees = 35.0;
        let result = projected_preview(directory.path(), tools, source, selected);
        assert!(result.matches_request(&result.context));
        let map = result.coordinate_map.as_ref().unwrap();
        let pixels = std::fs::read(&result.frame.as_ref().unwrap().artifact.path).unwrap();
        let eye_width = if half { 64.0 } else { 128.0 };
        for (x, y) in [(4, 7), (32, 32), (55, 49)] {
            let p = pulsar_core::ViewPoint {
                x: (f64::from(x) + 0.5) / 64.0,
                y: (f64::from(y) + 0.5) / 64.0,
            };
            let s = map.to_source(p).unwrap();
            let expected_r = ((s.x * eye_width * 2.0 - eye_width - 0.5) / (eye_width - 1.0) * 255.0)
                .round() as i32;
            let expected_g = ((s.y * 64.0 - 0.5) / 63.0 * 255.0).round() as i32;
            let actual = &pixels[((y * 64 + x) * 3) as usize..][..3];
            assert!(
                (i32::from(actual[0]) - expected_r).abs() <= 3
                    && (i32::from(actual[1]) - expected_g).abs() <= 3,
                "native sampling differs at {x},{y}: {actual:?} expected {expected_r},{expected_g}"
            );
            assert_eq!(actual[2], 103);
            let back = map.to_view(s).unwrap().unwrap();
            assert!((back.x - p.x).abs() < 1e-10 && (back.y - p.y).abs() < 1e-10);
        }
        assert_eq!(
            map.to_view(pulsar_core::PackedSourcePoint { x: 0.25, y: 0.5 })
                .unwrap(),
            None
        );
        let mut missing = result.clone();
        missing.coordinate_map = None;
        assert!(!missing.matches_request(&result.context));
        let mut corrupted = result.clone();
        let mut raw = serde_json::to_value(map).unwrap();
        raw["output_width"] = 63.into();
        corrupted.coordinate_map = Some(serde_json::from_value(raw).unwrap());
        assert!(!corrupted.matches_request(&result.context));
    }
}

#[test]
fn projected_offset_variable_pts_preview_uses_explicit_clock_and_actual_frame() {
    let (directory, tools, source) = projected_fixture(false);
    let offset = directory.path().join("offset-vfr.mkv");
    let mut command = Command::new(&tools.ffmpeg.path);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&source.path)
        .args([
            "-vf",
            "setpts=5/TB+(N+gte(N\\,2))*0.125/TB",
            "-fps_mode",
            "passthrough",
            "-threads",
            "1",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "bgr0",
        ])
        .arg(&offset);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    let pinned = pin_artifact(&offset).unwrap();
    let source = SourceArtifact {
        path: pinned.path,
        identity: pinned.identity,
        ..source
    };
    let index = media::probe(
        &tools.ffprobe.path,
        &source.path,
        Instant::now() + Duration::from_secs(20),
    )
    .unwrap();
    assert_eq!(
        index.pts[0] * i64::from(index.time_base_num) * 1000 / i64::from(index.time_base_den),
        5000
    );
    for (case, basis, time, ordinal, elapsed_ms) in [
        (
            0,
            PreviewTimeBasis::ElapsedFromFirstFrame,
            SourceTimestamp::new(300, 1000).unwrap(),
            1,
            125,
        ),
        (
            1,
            PreviewTimeBasis::ElapsedFromFirstFrame,
            SourceTimestamp::new(375, 1000).unwrap(),
            2,
            375,
        ),
        (
            2,
            PreviewTimeBasis::SourcePts,
            SourceTimestamp::new(5300, 1000).unwrap(),
            1,
            125,
        ),
    ] {
        let selected = view(false, StereoEye::Right, 0.0);
        let mut expected = context(&source);
        expected.transform = video_view_transform(Some(&selected)).unwrap();
        let output = directory.path().join(format!("seek-{case}"));
        std::fs::create_dir(&output).unwrap();
        let request = manifest(
            &output,
            tools.clone(),
            source.clone(),
            WorkerOperation::Preview {
                still_image: false,
                annotated: None,
                placement_seek: None,
                time_basis: basis,
                video_view: Some(selected),
                context: expected.clone(),
                source_time: time,
                max_width: 64,
                max_height: 64,
                model: None,
                model_input: None,
                inference_profile: None,
            },
        );
        let WorkerOutput::Preview(result) = execute_manifest(&request).result.unwrap() else {
            panic!("preview expected")
        };
        assert!(result.matches_timed_seek_request(&expected, &time, basis));
        assert_eq!(result.source_frame_index, ordinal);
        let elapsed = result.elapsed_source_time.unwrap();
        assert_eq!(
            elapsed.numerator() * 1000 / i64::from(elapsed.denominator()),
            elapsed_ms
        );
        let origin = result.source_origin.unwrap();
        assert_eq!(
            origin.numerator() * 1000 / i64::from(origin.denominator()),
            5000
        );
        let rgb = std::fs::read(result.frame.unwrap().artifact.path).unwrap();
        assert_eq!(rgb[(32 * 64 + 32) * 3 + 2], 100 + ordinal as u8);
    }
}

#[test]
fn placed_project_preview_uses_exact_subnanosecond_mapping_and_reverse_source_samples() {
    for rate in [2, -2] {
        let (directory, tools, source) = projected_fixture(false);
        let selected = view(false, StereoEye::Right, 0.0);
        let placement = SourcePlacement {
            placement_id: SourcePlacementId::new("placed-source").unwrap(),
            created_revision: RevisionId::new(0),
            spec: SourcePlacementSpec {
                source_version: source.source_version.clone(),
                source_start: SourceTimestamp::new(1, 8).unwrap(),
                source_end: SourceTimestamp::new(1, 1).unwrap(),
                source_anchor: SourceTimestamp::new(3, 8).unwrap(),
                project_anchor: ProjectTime::from_nanos(2_000_000_000),
                rate_numerator: rate,
                rate_denominator: 13,
            },
        };
        let seek = PlacedPreviewRequest {
            placement,
            project_time: ProjectTime::from_nanos(2_000_000_001),
        };
        let exact = seek
            .placement
            .spec
            .source_at_project(seek.project_time)
            .unwrap();
        assert!(exact.denominator() > u64::from(u32::MAX));
        let mut context = context(&source);
        context.source_placement = seek.placement.placement_id.clone();
        context.transform = video_view_transform(Some(&selected)).unwrap();
        let request = manifest(
            directory.path(),
            tools,
            source,
            WorkerOperation::Preview {
                still_image: false,
                annotated: None,
                placement_seek: Some(Into::into(seek.clone())),
                time_basis: PreviewTimeBasis::ElapsedFromFirstFrame,
                video_view: Some(selected),
                context: context.clone(),
                source_time: SourceTimestamp::new(0, 1).unwrap(),
                max_width: 64,
                max_height: 64,
                model: None,
                model_input: None,
                inference_profile: None,
            },
        );
        let WorkerOutput::Preview(result) = execute_manifest(&request).result.unwrap() else {
            panic!("preview expected")
        };
        assert!(result.matches_placed_seek_request(&context, &seek));
        assert!(!result.matches_timed_seek_request(
            &context,
            &SourceTimestamp::new(0, 1).unwrap(),
            PreviewTimeBasis::ElapsedFromFirstFrame
        ));
        let expected = if rate > 0 { 3 } else { 2 };
        assert_eq!(result.source_frame_index, expected);
        let rgb = std::fs::read(result.frame.unwrap().artifact.path).unwrap();
        assert_eq!(rgb[(32 * 64 + 32) * 3 + 2], 100 + expected as u8);
        let receipt = result.placed.unwrap();
        assert_eq!(
            receipt.quantization,
            receipt.actual_project_time.quantize_nanoseconds().unwrap()
        );
    }
}

#[test]
fn interval_generation_analyses_only_selected_original_frames_and_retains_boundary_gaps() {
    let (directory, tools, source) = fixture();
    let interval = TimeRange::new(
        ProjectTime::from_nanos(250_000_000),
        ProjectTime::from_nanos(750_000_000),
    )
    .unwrap();
    let request = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Generate {
            preset: "fast".into(),
            settings: (GenerationSettings {
                project_interval: Some(interval),
                analysis_width: 64,
                analysis_height: 64,
                ..Default::default()
            })
            .into(),
            model: None,
        },
    );
    let WorkerOutput::Generated {
        program, receipt, ..
    } = execute_manifest(&request).result.unwrap()
    else {
        panic!("generated")
    };
    let output: MotionProgram =
        serde_json::from_slice(&std::fs::read(program.path).unwrap()).unwrap();
    let track = output.track(Axis::Stroke).unwrap();
    assert_eq!(
        track
            .actions()
            .iter()
            .map(|a| a.time().as_nanos())
            .collect::<Vec<_>>(),
        vec![250_000_000, 375_000_000, 500_000_000, 625_000_000]
    );
    assert_eq!(track.actions()[0].evidence(), EvidenceKind::Synthesized);
    assert_eq!(track.actions()[0].position().value(), 0.5);
    assert_eq!(
        track.gaps(),
        &[TimeRange::new(ProjectTime::from_nanos(625_000_001), interval.end()).unwrap()]
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt.path).unwrap()).unwrap();
    assert_eq!(report["decoded_sample_count"], 8);
    assert_eq!(report["analysed_sample_count"], 4);
    assert_eq!(
        report["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["source_frame_index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    assert_eq!(
        report["configuration"]["project_interval"],
        serde_json::json!({"start":250_000_000,"end":750_000_000})
    );
    assert!(report["review"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["reason"] == "interval_boundary_without_decoded_support"));
}

#[test]
fn interval_generation_with_reverse_placement_retains_selected_source_and_project_orders() {
    let (directory, tools, source) = fixture();
    let placement = SourcePlacement {
        placement_id: SourcePlacementId::new("reverse-interval").unwrap(),
        created_revision: RevisionId::new(0),
        spec: SourcePlacementSpec {
            source_version: source.source_version.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(1, 1).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(2_000_000_000),
            rate_numerator: -1,
            rate_denominator: 1,
        },
    };
    let interval = TimeRange::new(
        ProjectTime::from_nanos(1_250_000_000),
        ProjectTime::from_nanos(1_750_000_000),
    )
    .unwrap();
    let request = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Generate {
            preset: "fast".into(),
            settings: (GenerationSettings {
                project_interval: Some(interval),
                source_placement: Some(placement),
                analysis_width: 64,
                analysis_height: 64,
                ..Default::default()
            })
            .into(),
            model: None,
        },
    );
    let WorkerOutput::Generated {
        program, receipt, ..
    } = execute_manifest(&request).result.unwrap()
    else {
        panic!("generated")
    };
    let output: MotionProgram =
        serde_json::from_slice(&std::fs::read(program.path).unwrap()).unwrap();
    let track = output.track(Axis::Stroke).unwrap();
    assert_eq!(
        track
            .actions()
            .iter()
            .map(|a| a.time().as_nanos())
            .collect::<Vec<_>>(),
        vec![1_250_000_000, 1_375_000_000, 1_500_000_000, 1_625_000_000]
    );
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt.path).unwrap()).unwrap();
    assert_eq!(
        report["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["source_frame_index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![3, 4, 5, 6]
    );
    assert_eq!(
        report["project_interval_plan"]["project_order"],
        serde_json::json!([3, 2, 1, 0])
    );
    assert_eq!(report["decoded_sample_count"], 8);
    assert_eq!(report["analysed_sample_count"], 4);
}

#[test]
fn projected_interval_preserves_original_nonzero_variable_pts_and_global_stride() {
    let (directory, tools, source) = projected_fixture(false);
    let offset = directory.path().join("interval-offset-vfr.mkv");
    let mut command = Command::new(&tools.ffmpeg.path);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&source.path)
        .args([
            "-vf",
            "setpts=5/TB+(N+gte(N\\,2))*0.125/TB",
            "-fps_mode",
            "passthrough",
            "-threads",
            "1",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "bgr0",
        ])
        .arg(&offset);
    media::bounded_output(command, 1024, Instant::now() + Duration::from_secs(20)).unwrap();
    let pinned = pin_artifact(&offset).unwrap();
    let source = SourceArtifact {
        path: pinned.path,
        identity: pinned.identity,
        ..source
    };
    let request = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Generate {
            preset: "fast".into(),
            settings: (GenerationSettings {
                project_interval: Some(
                    TimeRange::new(
                        ProjectTime::from_nanos(300_000_000),
                        ProjectTime::from_nanos(800_000_000),
                    )
                    .unwrap(),
                ),
                source_sample_stride: 2,
                video_view: Some(view(false, StereoEye::Right, 0.0)),
                analysis_width: 64,
                analysis_height: 64,
                ..Default::default()
            })
            .into(),
            model: None,
        },
    );
    let WorkerOutput::Generated { receipt, .. } = execute_manifest(&request).result.unwrap() else {
        panic!("generated")
    };
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(receipt.path).unwrap()).unwrap();
    assert_eq!(report["source_origin_pts"], 5000);
    assert_eq!(
        report["source_time_base"],
        serde_json::json!({"numerator":1,"denominator":1000})
    );
    assert_eq!(
        report["samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (
                s["source_frame_index"].as_u64().unwrap(),
                s["source_pts"].as_i64().unwrap(),
                s["project_time_ns"].as_i64().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![(2, 5375, 375_000_000), (4, 5625, 625_000_000)]
    );
    assert_eq!(report["decoded_sample_count"], 4);
    assert_eq!(report["analysed_sample_count"], 2);
    assert_eq!(
        report["project_interval_plan"]["uncovered_boundaries"],
        serde_json::json!([{"start":300_000_000,"end":375_000_000},{"start":625_000_001,"end":800_000_000}])
    );
}

#[test]
fn unplaced_generation_receipt_verifies_content_without_a_placement_plan() {
    let (directory, tools, source) = fixture();
    let request = manifest(
        directory.path(),
        tools,
        source,
        WorkerOperation::Generate {
            preset: "fast".into(),
            settings: (GenerationSettings {
                analysis_width: 64,
                analysis_height: 64,
                ..Default::default()
            })
            .into(),
            model: None,
        },
    );
    let WorkerOutput::Generated {
        program, receipt, ..
    } = execute_manifest(&request).result.unwrap()
    else {
        panic!("generated")
    };
    let raw = std::fs::read(&program.path).unwrap();
    let motion: MotionProgram = serde_json::from_slice(&raw).unwrap();
    let canonical_bytes = serde_json::to_vec(&motion).unwrap();
    let hash = format!("{:x}", Sha256::digest(&canonical_bytes));
    let canonical = ArtifactIdentity {
        artifact_id: ArtifactId::new(format!("motion.{hash}")).unwrap(),
        sha256: hash,
        byte_len: canonical_bytes.len() as u64,
    };
    let bytes = std::fs::read(&receipt.path).unwrap();
    let verify = |bytes: Vec<u8>, retained: Vec<ArtifactIdentity>, raw: Vec<u8>| {
        crate::source_clock_receipts::load_origin_readers(
            &request,
            &retained,
            &canonical,
            std::io::Cursor::new(bytes.clone()),
            bytes.len() as u64,
            |_, _| Ok(Box::new(std::io::Cursor::new(raw.clone()))),
        )
    };
    let origin = verify(bytes.clone(), vec![receipt.identity.clone()], raw.clone()).unwrap();
    assert!(origin.plan.is_none());
    assert_eq!(origin.identity, receipt.identity);
    assert!(verify(bytes.clone(), vec![], raw.clone()).is_err());
    assert!(verify(
        bytes.clone(),
        vec![receipt.identity.clone(); 2],
        raw.clone()
    )
    .is_err());
    let mut corrupt = raw.clone();
    corrupt[0] ^= 1;
    assert!(verify(bytes.clone(), vec![receipt.identity.clone()], corrupt).is_err());
    let report: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    // Rehashed malformed headers must still fail semantic producer binding.
    for (key, value) in [
        ("schema_version", serde_json::json!(2)),
        ("attempt_id", serde_json::json!("wrong-attempt")),
        ("configuration_sha256", serde_json::json!("0".repeat(64))),
        ("recipe", serde_json::json!("raw-frame-pair")),
        ("project_time_mapping", serde_json::json!("unknown")),
        ("source_placement", serde_json::json!({})),
        ("placed_generation_plan", serde_json::json!({})),
    ] {
        let mut changed = report.clone();
        changed[key] = value;
        let changed = serde_json::to_vec(&changed).unwrap();
        let hash = format!("{:x}", Sha256::digest(&changed));
        let identity = ArtifactIdentity {
            artifact_id: ArtifactId::new(hash.clone()).unwrap(),
            sha256: hash,
            byte_len: changed.len() as u64,
        };
        assert!(
            verify(changed, vec![identity], raw.clone()).is_err(),
            "accepted {key}"
        );
    }
    assert!(crate::source_clock_receipts::load_verified_readers(
        &request,
        &[receipt.identity],
        &canonical,
        std::io::Cursor::new(bytes.clone()),
        bytes.len() as u64,
        |_, _| Ok(Box::new(std::io::Cursor::new(raw.clone())))
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn mixed_sampling_stash_export_uses_each_actual_producer_clock() {
    let (_directory, tools, source) = fixture_with_shifts(&[0, 1, 4, 6, 4, 1, 2, 4], 2);
    crate::authority::tests::verify_native_mixed_export_fixture(tools, source);
}

#[cfg(unix)]
#[test]
fn archived_mixed_sampling_stash_export_uses_retained_producer_clocks() {
    let (_directory, tools, source) = fixture_with_shifts(&[0, 1, 4, 6, 4, 1, 2, 4], 2);
    crate::authority::tests::verify_archived_mixed_export_fixture(tools, source);
}

#[cfg(unix)]
#[test]
fn archived_mixed_sampling_stash_export_preserves_imported_and_local_retiming_clocks() {
    let (_directory, tools, source) = fixture_with_shifts(&[0, 1, 4, 6, 4, 1, 2, 4], 2);
    crate::authority::tests::verify_archived_retimed_mixed_export_fixture(tools, source);
}

#[test]
#[ignore = "owned authored corrupt-media campaign supplies immutable fixtures"]
fn corrupt_video_packet_refuses_preview_without_publishing_frame() {
    let inputs = std::path::PathBuf::from(
        std::env::var_os("PULSAR_CORRUPT_MEDIA_CASES").expect("owned fixtures"),
    );
    let tools = discover_tools().expect("native media tools");
    for (filename, ordinal, should_fail) in [
        ("clean.mp4", 1, false),
        ("damaged-1.mp4", 1, true),
        ("damaged-3.mp4", 3, true),
        ("damaged-7.mp4", 7, true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(filename);
        std::fs::copy(inputs.join(filename), &path).unwrap();
        let pinned = pin_artifact(&path).unwrap();
        let source = SourceArtifact {
            source_version: SourceVersionId::new("authored-corrupt-fixture").unwrap(),
            path: pinned.path,
            identity: pinned.identity,
        };
        let requested = context(&source);
        let request = manifest(
            directory.path(),
            tools.clone(),
            source,
            WorkerOperation::Preview {
                still_image: false,
                annotated: None,
                placement_seek: None,
                time_basis: PreviewTimeBasis::SourcePts,
                video_view: None,
                context: requested.clone(),
                source_time: SourceTimestamp::new(ordinal, 8).unwrap(),
                max_width: 64,
                max_height: 64,
                model: None,
                model_input: None,
                inference_profile: None,
            },
        );
        let response = execute_manifest(&request).result;
        if should_fail {
            assert!(
                response.is_err(),
                "Corrupt {filename} returned a successful-looking preview"
            );
            assert_eq!(
                std::fs::read_dir(&request.output_dir).unwrap().count(),
                0,
                "Corrupt source published an artifact"
            );
        } else {
            let WorkerOutput::Preview(preview) = response.unwrap() else {
                panic!("expected preview")
            };
            assert!(preview
                .matches_seek_request(&requested, &SourceTimestamp::new(ordinal, 8).unwrap()));
            assert!(preview.frame.is_some());
        }
        let mut decoder = media::Decoder::new_filtered(
            &request.tools.ffmpeg.path,
            &request.source.path,
            media::FrameTransform {
                width: 64,
                height: 64,
                filter: "scale=64:64",
            },
            1,
            1,
            Instant::now() + Duration::from_secs(20),
        )
        .unwrap();
        let mut decoded = 0;
        let mut failure = None;
        loop {
            match decoder.next() {
                Ok(Some(frame)) => {
                    assert_eq!(
                        i128::from(frame.source_time.numerator()) * 8,
                        i128::from(decoded) * i128::from(frame.source_time.denominator())
                    );
                    decoded += 1;
                }
                Ok(None) => break,
                Err(error) => {
                    failure = Some(format!("{error:#}"));
                    break;
                }
            }
        }
        if should_fail {
            assert!(
                failure.is_some(),
                "Corrupt {filename} silently completed its decoded stream"
            );
            assert!(decoded < 16);
        } else {
            assert!(failure.is_none(), "Clean stream failed: {failure:?}");
            assert_eq!(decoded, 16);
        }
        println!("authored media {filename}: preview_failure={should_fail}, stream_frames={decoded}, terminal={failure:?}, qualified=false");
    }
}

#[test]
fn metadata_probe_reports_authored_video_without_motion_or_artifacts() {
    let (directory, mut tools, source) = fixture();
    tools.onnx_runtime = None;
    let mut request = manifest(directory.path(), tools, source, WorkerOperation::ProbeMedia);
    let WorkerOutput::MediaProbe(report) = execute_manifest(&request).result.unwrap() else {
        panic!("metadata");
    };
    report.validate().unwrap();
    assert_eq!(report.project_id, request.project_id);
    assert_eq!(report.revision, request.base_revision);
    assert_eq!(report.source, request.source.identity);
    assert_eq!(report.source_version, request.source.source_version);
    assert!(!report.decoded && !report.qualified);
    assert_eq!(report.streams.len(), 1);
    let stream = &report.streams[0];
    assert_eq!(stream.kind, "video");
    assert_eq!(stream.codec.as_deref(), Some("ffv1"));
    assert_eq!((stream.width, stream.height), (Some(64), Some(64)));
    assert_eq!(stream.frame_rate, Some(8.0));
    assert_eq!(report.declared_duration_seconds, Some(1.0));
    assert_eq!(std::fs::read_dir(&request.output_dir).unwrap().count(), 0);
    request.source.identity.sha256 = "0".repeat(64);
    assert!(execute_manifest(&request).result.is_err());
    assert_eq!(std::fs::read_dir(&request.output_dir).unwrap().count(), 0);
}
