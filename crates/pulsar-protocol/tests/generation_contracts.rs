use pulsar_protocol::*;

#[test]
fn audio_vibe_has_a_canonical_wire_mode_and_rejects_unknown_modes() {
    let input = GenerationInput::Audio {
        source_version: SourceVersionId::new("audio-source").unwrap(),
        mapping: AudioMapping::new(Axis::Yaw, 0.5, NormalizedPosition::new(0.25).unwrap()).unwrap(),
        mode: AudioMode::Vibe,
        analysis: Some(AudioAnalysisOptions::new(AudioBand::Mid, 0.2).unwrap()),
    };
    let mut wire = serde_json::to_value(input).unwrap();
    assert_eq!(wire["arguments"]["mode"], "vibe");
    serde_json::from_value::<GenerationInput>(wire.clone())
        .unwrap()
        .validate()
        .unwrap();
    for mode in ["osc", "oscillation", "unknown"] {
        wire["arguments"]["mode"] = serde_json::json!(mode);
        assert!(serde_json::from_value::<GenerationInput>(wire.clone()).is_err());
    }
}

#[test]
fn audio_analysis_is_checked_and_legacy_wire_remains_unchanged() {
    let legacy = serde_json::json!({"modality":"audio","arguments":{
        "source_version":"source", "mapping":serde_json::to_value(AudioMapping::new(Axis::Stroke,0.5,NormalizedPosition::new(0.25).unwrap()).unwrap()).unwrap(),
        "mode":"envelope"}});
    let old: GenerationInput = serde_json::from_value(legacy.clone()).unwrap();
    old.validate().unwrap();
    assert_eq!(serde_json::to_value(old).unwrap(), legacy);
    let mut explicit = legacy.clone();
    explicit["arguments"]["analysis"] = serde_json::json!({"band":"bass","threshold":0.2});
    let input: GenerationInput = serde_json::from_value(explicit.clone()).unwrap();
    input.validate().unwrap();
    assert_eq!(serde_json::to_value(input).unwrap(), explicit);
    for bad in [
        serde_json::json!({"band":"unknown","threshold":0.2}),
        serde_json::json!({"band":"full","threshold":0.0}),
        serde_json::json!({"band":"bass","threshold":0.2,"extra":true}),
    ] {
        explicit["arguments"]["analysis"] = bad;
        assert!(serde_json::from_value::<GenerationInput>(explicit.clone()).is_err());
    }
}

fn id() -> RequestId {
    RequestId::new("request").unwrap()
}
fn project() -> ProjectId {
    ProjectId::new("project").unwrap()
}
fn synthesis() -> GenerationInput {
    GenerationInput::Text {
        prompt: "pattern=sine axis=stroke duration=2s frequency=1hz amplitude=0.2 offset=0.5"
            .into(),
    }
}
fn flag() -> ReviewFlag {
    ReviewFlag {
        axis: Some(Axis::Stroke),
        range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000)).unwrap(),
        reason: ReviewReason::new("missing_detection").unwrap(),
        evidence: EvidenceKind::Unavailable,
        confidence: Some(Confidence::new(0.25, None).unwrap()),
    }
}

#[test]
fn additive_mutations_require_project_and_revision() {
    for command in [
        Command::GenerateInput { input: synthesis() },
        Command::ImportFunscript {
            path: std::env::temp_dir().join("input.funscript"),
        },
        Command::MergeCandidate {
            candidate_id: CandidateId::new("candidate").unwrap(),
            axes: vec![Axis::Stroke],
            range: None,
        },
    ] {
        assert!(Request::new(id(), command.clone()).validate().is_err());
        assert!(Request::new(id(), command.clone())
            .in_project(project(), None)
            .validate()
            .is_err());
        let request = Request::new(id(), command).in_project(project(), Some(RevisionId::new(0)));
        assert!(request.validate().is_ok());
        assert!(request.command.is_mutating());
    }
    let diagnostics = Request::new(
        id(),
        Command::Diagnostics {
            candidate_id: None,
            maximum_speed_percent_per_second: None,
        },
    )
    .in_project(project(), None);
    assert!(diagnostics.validate().is_ok());
    assert!(!diagnostics.command.is_mutating());
}

#[test]
fn prompts_and_axes_are_bounded_and_unambiguous() {
    for prompt in [
        "".to_owned(),
        "x".repeat(MAX_PROMPT_BYTES + 1),
        "private\ncommand".to_owned(),
    ] {
        assert!(GenerationInput::Text {
            prompt: prompt.clone()
        }
        .validate()
        .is_err());
        assert!(GenerationInput::Image {
            source_version: SourceVersionId::new("source").unwrap(),
            prompt
        }
        .validate()
        .is_err());
    }
    assert!(validate_merge_axes(&[]).is_err());
    assert!(validate_merge_axes(&[Axis::Stroke, Axis::Stroke]).is_err());
    assert!(validate_merge_axes(&Axis::ALL).is_ok());
    let pattern = parse_pattern_prompt(
        "pattern=sine axis=stroke duration=2s frequency=1hz amplitude=0.2 offset=0.5",
    )
    .unwrap();
    assert!(GenerationInput::Patterns {
        patterns: vec![pattern.clone(), pattern]
    }
    .validate()
    .is_err());
    assert!(GenerationInput::Patterns { patterns: vec![] }
        .validate()
        .is_err());
}

#[test]
fn prompt_bytes_are_redacted_from_debug_and_preserved_on_wire() {
    let input = GenerationInput::Text {
        prompt: "PRIVATE_PATTERN_TEXT".into(),
    };
    assert!(!format!("{input:?}").contains("PRIVATE_PATTERN_TEXT"));
    let request = Request::new(id(), Command::GenerateInput { input });
    assert!(!format!("{request:?}").contains("PRIVATE_PATTERN_TEXT"));
    assert!(serde_json::to_string(&request)
        .unwrap()
        .contains("PRIVATE_PATTERN_TEXT"));
}

#[test]
fn review_spans_reject_invalid_codes_times_and_calibration_claims() {
    for code in ["", "Private prompt", "UPPER", "../secret/path"] {
        assert!(ReviewReason::new(code).is_err());
    }
    assert!(serde_json::from_str::<ReviewReason>("\"not a code\"").is_err());
    assert!(validate_reviews(&[flag()]).is_ok());
    assert!(validate_reviews(&vec![flag(); MAX_REVIEW_FLAGS + 1]).is_err());
    let mut invalid = flag();
    invalid.range =
        TimeRange::new(ProjectTime::from_nanos(-1), ProjectTime::from_nanos(1)).unwrap();
    assert!(invalid.validate().is_err());
    assert!(serde_json::from_value::<ReviewFlag>(serde_json::to_value(&invalid).unwrap()).is_err());
    invalid = flag();
    invalid.confidence = Some(
        Confidence::new(
            0.9,
            Some(ArtifactId::new("unverified-calibration").unwrap()),
        )
        .unwrap(),
    );
    assert!(invalid.validate().is_err());
    assert!(serde_json::from_value::<ReviewFlag>(serde_json::to_value(invalid).unwrap()).is_err());
}

#[test]
fn candidate_descriptor_payload_defaults_review_without_inline_program() {
    let candidate = CandidateSnapshot {
        project_interval: None,
        candidate_id: CandidateId::new("candidate").unwrap(),
        project_id: project(),
        base_revision: RevisionId::new(0),
        motion: MotionDescriptor {
            program: ProgramDescriptor {
                artifact_id: ArtifactId::new("artifact").unwrap(),
                sha256: "0".repeat(64),
                byte_len: 13,
                codec: MotionCodec::MotionProgramJsonV1,
                axes: vec![],
            },
            binding: MotionBinding::Candidate {
                project_id: project(),
                candidate_id: CandidateId::new("candidate").unwrap(),
                base_revision: RevisionId::new(0),
            },
        },
        job_id: None,
        review: vec![],
    };
    let mut wire = serde_json::to_value(candidate).unwrap();
    wire.as_object_mut().unwrap().remove("review");
    let decoded: CandidateSnapshot = serde_json::from_value(wire).unwrap();
    assert!(decoded.review.is_empty());
    assert!(decoded.motion.program.axes.is_empty());
}

#[test]
fn modality_unknown_fields_and_missing_image_prompt_fail_closed() {
    let mut wire = serde_json::to_value(synthesis()).unwrap();
    wire["arguments"]["shell"] = serde_json::json!("arbitrary command");
    assert!(serde_json::from_value::<GenerationInput>(wire).is_err());
    assert!(serde_json::from_str::<GenerationInput>(
        r#"{"modality":"image","arguments":{"source_version":"source"}}"#
    )
    .is_err());
}

#[test]
fn diagnostics_do_not_admit_unbounded_or_ambiguous_reports() {
    let mut report = DiagnosticsReport {
        project_id: project(),
        revision: RevisionId::new(1),
        candidate_id: None,
        base_revision: None,
        protected_regions: vec![],
        segment_speed: None,
        issues: vec![],
    };
    assert!(report.validate().is_ok());
    report.base_revision = Some(RevisionId::new(0));
    assert!(report.validate().is_err());
    report.base_revision = None;
    report.issues.push(DiagnosticIssue {
        code: "gap".into(),
        message: "x".repeat(513),
        axis: None,
        range: None,
    });
    assert!(report.validate().is_err());
}

#[test]
fn diagnostics_total_json_escaping_cannot_overrun_control_budget() {
    let report = DiagnosticsReport {
        project_id: project(),
        revision: RevisionId::new(0),
        candidate_id: None,
        base_revision: None,
        protected_regions: vec![],
        segment_speed: None,
        issues: vec![
            DiagnosticIssue {
                code: "bounded".into(),
                message: "\\".repeat(512),
                axis: None,
                range: None
            };
            MAX_DIAGNOSTIC_ISSUES
        ],
    };
    assert!(report.issues.iter().all(|issue| issue.validate().is_ok()));
    assert!(
        report.validate().is_err(),
        "individually bounded strings still require an aggregate encoded-byte limit"
    );
}

#[test]
fn all_six_checked_pattern_axes_are_admitted_once() {
    let patterns = Axis::ALL
        .iter()
        .map(|axis| {
            parse_pattern_prompt(&format!(
                "pattern=sine axis={} duration=2s frequency=1hz amplitude=0.2 offset=0.5",
                axis.as_str()
            ))
            .unwrap()
        })
        .collect();
    assert!(GenerationInput::Patterns { patterns }.validate().is_ok());
}

#[test]
fn optional_segment_speed_policy_is_bounded_read_only_and_legacy_compatible() {
    let legacy: Command = serde_json::from_value(
        serde_json::json!({"operation":"diagnostics","arguments":{"candidate_id":null}}),
    )
    .unwrap();
    assert!(!serde_json::to_value(&legacy).unwrap()["arguments"]
        .as_object()
        .unwrap()
        .contains_key("maximum_speed_percent_per_second"));
    for limit in [0.0, -1.0, f64::NAN, f64::INFINITY, 100_000_000_001.0] {
        assert!(Request::new(
            id(),
            Command::Diagnostics {
                candidate_id: None,
                maximum_speed_percent_per_second: Some(limit)
            }
        )
        .in_project(project(), None)
        .validate()
        .is_err());
    }
    let request = Request::new(
        id(),
        Command::Diagnostics {
            candidate_id: None,
            maximum_speed_percent_per_second: Some(450.0),
        },
    )
    .in_project(project(), None);
    request.validate().unwrap();
    assert!(!request.command.is_mutating());
    assert!(!request.command.requires_revision());
    let mut scan = SegmentSpeedScan {
        maximum_percent_per_second: 450.0,
        scanned_segments: 1,
        unavailable_segments: 0,
        truncated: false,
        violations: vec![SegmentSpeedViolation {
            axis: Axis::Stroke,
            range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1)).unwrap(),
            average_percent_per_second: 500.0,
        }],
    };
    scan.validate().unwrap();
    scan.unavailable_segments = 1;
    assert!(scan.validate().is_err());
    scan.unavailable_segments = 0;
    scan.violations[0].average_percent_per_second = f64::NAN;
    assert!(scan.validate().is_err());
}

#[test]
fn generation_interval_is_optional_strict_and_original_project_time() {
    let legacy = serde_json::to_value(GenerationSettings::default()).unwrap();
    assert!(legacy.get("project_interval").is_none());
    let mut settings: GenerationSettings = serde_json::from_value(legacy.clone()).unwrap();
    settings.project_interval =
        Some(TimeRange::new(ProjectTime::from_nanos(123), ProjectTime::from_nanos(456)).unwrap());
    settings.validate().unwrap();
    let encoded = serde_json::to_value(&settings).unwrap();
    assert_eq!(
        encoded["project_interval"],
        serde_json::json!({"start":123,"end":456})
    );
    assert_eq!(
        serde_json::from_value::<GenerationSettings>(encoded)
            .unwrap()
            .project_interval,
        settings.project_interval
    );
    settings.project_interval =
        Some(TimeRange::new(ProjectTime::from_nanos(-1), ProjectTime::from_nanos(1)).unwrap());
    assert!(settings.validate().is_err());
    let mut bad = legacy;
    bad["project_interval"] = serde_json::json!({"start":2,"end":1});
    assert!(serde_json::from_value::<GenerationSettings>(bad).is_err());
}

#[test]
fn typed_pattern_admission_preflights_duration_and_combined_allocation() {
    let a=parse_pattern_prompt("pattern=triangle axis=stroke duration=1000s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=200").unwrap();
    let other = parse_pattern_prompt(
        "pattern=triangle axis=yaw duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8",
    )
    .unwrap();
    assert!(GenerationInput::Patterns {
        patterns: vec![a, other]
    }
    .validate()
    .is_err());
    let patterns=["stroke","sway","surge","roll","pitch","yaw"].into_iter().map(|axis|parse_pattern_prompt(&format!("pattern=triangle axis={axis} duration=1000s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=200")).unwrap()).collect();
    let request = Request::new(
        id(),
        Command::GenerateInput {
            input: GenerationInput::Patterns { patterns },
        },
    )
    .in_project(project(), Some(RevisionId::new(0)));
    assert!(request.validate().is_err());
}
