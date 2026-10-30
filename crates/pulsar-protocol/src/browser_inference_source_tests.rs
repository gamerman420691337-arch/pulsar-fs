use super::*;

fn fixture() -> (AttemptId, LiveCaptureOptions, LiveInferenceManifest) {
    // Authored metadata only: neither these fixtures nor matching hashes mint
    // native media/timing ownership, accepted equivalence or worker authority.
    let mut stream: LiveInferenceManifest =
        serde_json::from_str(include_str!("../tests/fixtures/live-cpu-manifest.json")).unwrap();
    let report: LiveCaptureReport = serde_json::from_str(include_str!(
        "../tests/fixtures/live-cpu-capture-report.json"
    ))
    .unwrap();
    let mut options = report.options;
    let mut media = crate::browser_media::test_binding();
    media.project_id = stream.identity.project.clone();
    media.revision = stream.identity.revision;
    media.selection.source_version = stream.identity.source.clone();
    media.selection.placement.spec.source_version = stream.identity.source.clone();
    media.selection.source.artifact_id =
        ArtifactId::new(media.selection.source.sha256.clone()).unwrap();
    options.source_time_basis = LiveSourceTimeBasis::SourceMediaPresentation;
    options.source_anchor = media.clock.presentation_anchor;
    let exact = media
        .clock
        .project_at(options.source_anchor, &media.selection.placement.spec)
        .unwrap();
    options.project_anchor = exact.quantize_nanoseconds().unwrap().time;
    options.exact_project_anchor = Some(exact);
    options.inference = Some(LiveCaptureInference {
        model_path: stream.model.path.clone(),
        profile: stream.profile.clone(),
    });
    options.browser_analysis = Some(BrowserAnalysisCaptureDeclaration {
        feed_id: ArtifactId::new("authored-feed").unwrap(),
        player: HandyPlayerIdentity {
            launch_id: AttemptId::new("launch").unwrap(),
            page_id: AttemptId::new("page").unwrap(),
            player_id: AttemptId::new("player").unwrap(),
            source_id: AttemptId::new("dom-source").unwrap(),
            generation: 1,
            time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
        },
        timing_binding_id: ArtifactId::new("timing").unwrap(),
        policy: BrowserAnalysisLookaheadPolicy {
            minimum_lead_us: 1_000_000,
            target_lead_us: 2_000_000,
            maximum_lead_us: 3_000_000,
        },
    });
    let epoch = media.engine_epoch.clone();
    options.browser_media = Some(media);
    stream.clock = options.clock(stream.identity.epoch.clone());
    stream.budget.wall_time_ms = u64::from(options.lease_ms);
    options.validate().unwrap();
    stream.validate().unwrap();
    (epoch, options, stream)
}

#[test]
fn browser_source_policy_has_distinct_wire_domain_and_preserves_file_policy_bytes() {
    let browser = BrowserInferenceFallbackSourcePolicy::any_bound_media();
    let file = InferenceFallbackSourcePolicy {
        schema_version: 1,
        kind: InferenceFallbackSourceKind::DecodedMedia,
        sources: None,
        independent_audio: InferenceFallbackIndependentAudio::Forbidden,
    };
    assert_eq!(
        serde_json::to_string(&file).unwrap(),
        r#"{"schema_version":1,"kind":"decoded_media","sources":null,"independent_audio":{"kind":"forbidden"}}"#
    );
    assert_ne!(browser.identity().unwrap(), file.identity().unwrap());
    assert!(serde_json::from_value::<InferenceFallbackSourcePolicy>(
        serde_json::to_value(&browser).unwrap()
    )
    .is_err());
    assert!(
        serde_json::from_value::<BrowserInferenceFallbackSourcePolicy>(
            serde_json::to_value(&file).unwrap()
        )
        .is_err()
    );
}

#[test]
fn browser_source_policy_requires_bound_analysis_and_exact_boot_project_revision_source_clock() {
    let (epoch, options, stream) = fixture();
    let policy = BrowserInferenceFallbackSourcePolicy::any_bound_media();
    assert!(policy.matches_manifest(&epoch, &options, &stream).unwrap());
    stream.validate_browser_source(&epoch, &options).unwrap();
    assert!(!policy
        .matches_manifest(&AttemptId::new("other-boot").unwrap(), &options, &stream)
        .unwrap());
    for field in ["project", "revision", "source", "clock"] {
        let mut changed = stream.clone();
        match field {
            "project" => changed.identity.project = ProjectId::new("other-project").unwrap(),
            "revision" => {
                changed.identity.revision = RevisionId::new(changed.identity.revision.value() + 1)
            }
            "source" => changed.identity.source = SourceVersionId::new("other-source").unwrap(),
            "clock" => {
                changed.clock.presentation_anchor =
                    LiveClockTick(changed.clock.presentation_anchor.0 + 1)
            }
            _ => unreachable!(),
        }
        assert!(
            !policy.matches_manifest(&epoch, &options, &changed).unwrap(),
            "{field}"
        );
        assert!(
            changed.validate_browser_source(&epoch, &options).is_err(),
            "{field}"
        );
    }
    let mut unbound = options.clone();
    unbound.browser_analysis = None;
    assert!(!policy.matches_manifest(&epoch, &unbound, &stream).unwrap());
    unbound.browser_media = None;
    unbound.source_time_basis = LiveSourceTimeBasis::ProducerDeclared;
    unbound.exact_project_anchor = None;
    assert!(!policy.matches_manifest(&epoch, &unbound, &stream).unwrap());
}

#[test]
fn browser_source_policy_refuses_profile_model_path_and_lease_substitution() {
    let (epoch, options, stream) = fixture();
    let policy = BrowserInferenceFallbackSourcePolicy::any_bound_media();
    let mut changed = options.clone();
    changed.inference.as_mut().unwrap().profile.cpu_threads += 1;
    assert!(!policy.matches_manifest(&epoch, &changed, &stream).unwrap());
    let mut changed = options.clone();
    changed.inference.as_mut().unwrap().model_path = "/other/model".into();
    assert!(!policy.matches_manifest(&epoch, &changed, &stream).unwrap());
    let mut changed = stream.clone();
    changed.budget.wall_time_ms -= 1;
    assert!(!policy.matches_manifest(&epoch, &options, &changed).unwrap());
}

#[test]
fn browser_source_policy_checks_exact_content_allowlist_and_bounded_canonical_identity() {
    let (epoch, options, stream) = fixture();
    let source = options
        .browser_media
        .as_ref()
        .unwrap()
        .selection
        .source
        .clone();
    let mut policy = BrowserInferenceFallbackSourcePolicy {
        sources: Some(vec![source.clone()]),
        ..BrowserInferenceFallbackSourcePolicy::any_bound_media()
    };
    assert!(policy.matches_manifest(&epoch, &options, &stream).unwrap());
    let before = policy.identity().unwrap();
    let source = policy.sources.as_mut().unwrap().first_mut().unwrap();
    source.sha256 = "b".repeat(64);
    source.artifact_id = ArtifactId::new(source.sha256.clone()).unwrap();
    assert!(!policy.matches_manifest(&epoch, &options, &stream).unwrap());
    assert_ne!(before, policy.identity().unwrap());
    policy.sources = Some(vec![]);
    assert!(policy.validate().is_err());
    policy.sources = None;
    policy.schema_version = 2;
    assert!(policy.identity().is_err());
    let mut value =
        serde_json::to_value(BrowserInferenceFallbackSourcePolicy::any_bound_media()).unwrap();
    value["namespace"] = serde_json::json!("decoded_media");
    assert!(serde_json::from_value::<BrowserInferenceFallbackSourcePolicy>(value).is_err());
}

fn reanchor(options: &mut LiveCaptureOptions, at: SourceTimestamp) {
    let media = options.browser_media.as_ref().unwrap();
    let exact = media
        .clock
        .project_at(at, &media.selection.placement.spec)
        .unwrap();
    options.source_anchor = at;
    options.project_anchor = exact.quantize_nanoseconds().unwrap().time;
    options.exact_project_anchor = Some(exact);
}

#[test]
fn browser_semantics_preserve_fresh_epoch_coordinates_without_granting_equivalence() {
    let (_, options, _) = fixture();
    let expected = browser_inference_semantic_identity(&options).unwrap();
    let mut next = options.clone();
    let media = next.browser_media.as_mut().unwrap();
    media.preparation_request_id = RequestId::new("fresh-media-request").unwrap();
    media.lease_id = ArtifactId::new("fresh-media-lease").unwrap();
    media.lease_ms += 1;
    let analysis = next.browser_analysis.as_mut().unwrap();
    analysis.feed_id = ArtifactId::new("fresh-feed").unwrap();
    analysis.timing_binding_id = ArtifactId::new("fresh-timing").unwrap();
    next.continuation_mode = LiveContinuationMode::WhenReady;
    next.lease_ms -= 1;
    next.presentation_anchor = LiveClockTick(100);
    reanchor(&mut next, SourceTimestamp::new(6, 1).unwrap());
    let selected = next.inference.as_mut().unwrap();
    selected.model_path = "/another/model-alias".into();
    selected.profile.cpu_threads += 1;
    selected.profile.runtime.sha256 = "c".repeat(64);
    selected.profile.runtime.artifact_id = ArtifactId::new("c".repeat(64)).unwrap();
    assert_eq!(
        browser_inference_semantic_identity(&next).unwrap(),
        expected
    );
    validate_browser_inference_semantic_successor(&options, &next).unwrap();
    // This metadata test neither approves this alternate runtime nor owns any
    // native source, lease, worker settlement or equivalent-profile decision.
}

#[test]
fn browser_semantics_refuse_source_model_player_clock_and_policy_changes() {
    let (_, options, _) = fixture();
    let expected = browser_inference_semantic_identity(&options).unwrap();
    for field in [
        "boot",
        "project",
        "revision",
        "source",
        "placement",
        "trim",
        "clock",
        "player",
        "lookahead",
        "age",
        "buffer",
        "model",
        "pack",
    ] {
        let mut changed = options.clone();
        let media = changed.browser_media.as_mut().unwrap();
        match field {
            "boot" => media.engine_epoch = AttemptId::new("different-boot").unwrap(),
            "project" => media.project_id = ProjectId::new("different-project").unwrap(),
            "revision" => media.revision = RevisionId::new(media.revision.value() + 1),
            "source" => {
                media.selection.source.sha256 = "d".repeat(64);
                media.selection.source.artifact_id = ArtifactId::new("d".repeat(64)).unwrap();
            }
            "placement" => {
                media.selection.placement.placement_id =
                    SourcePlacementId::new("other-placement").unwrap()
            }
            "trim" => {
                media.selection.placement.spec.source_end = SourceTimestamp::new(9, 1).unwrap()
            }
            "clock" => media.selection.placement.spec.project_anchor = ProjectTime::from_nanos(1),
            "player" => changed.browser_analysis.as_mut().unwrap().player.generation += 1,
            "lookahead" => {
                changed
                    .browser_analysis
                    .as_mut()
                    .unwrap()
                    .policy
                    .target_lead_us += 1
            }
            "age" => changed.policy.maximum_analysis_age_ns += 1,
            "buffer" => changed.policy.added_buffer_ns += 1,
            "model" => {
                let model = &mut changed.inference.as_mut().unwrap().profile.model;
                model.sha256 = "e".repeat(64);
                model.artifact_id = ArtifactId::new("e".repeat(64)).unwrap();
            }
            "pack" => {
                changed.model_pack = Some(ModelPackChoice {
                    manifest_sha256: "f".repeat(64),
                    selection_revision: 1,
                })
            }
            _ => unreachable!(),
        }
        let at = changed.source_anchor;
        reanchor(&mut changed, at);
        changed.validate().unwrap();
        assert_ne!(
            browser_inference_semantic_identity(&changed).unwrap(),
            expected,
            "{field}"
        );
        assert!(
            validate_browser_inference_semantic_successor(&options, &changed).is_err(),
            "{field}"
        );
    }
}

#[test]
fn browser_view_policy_is_separate_bounded_and_matches_actual_semantics() {
    let (_, options, _) = fixture();
    let basis = browser_inference_semantic_identity(&options).unwrap();
    let file = InferenceFallbackViewPolicy {
        schema_version: 1,
        applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
    };
    let mut browser = BrowserInferenceFallbackViewPolicy {
        schema_version: 1,
        namespace: BrowserInferenceFallbackSourceNamespace::BrowserMediaCapture,
        applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
    };
    assert_eq!(
        serde_json::to_string(&file).unwrap(),
        r#"{"schema_version":1,"applicability":{"kind":"preserve_requested_semantics"}}"#
    );
    assert_ne!(file.identity().unwrap(), browser.identity().unwrap());
    assert!(serde_json::from_value::<InferenceFallbackViewPolicy>(
        serde_json::to_value(&browser).unwrap()
    )
    .is_err());
    assert!(
        serde_json::from_value::<BrowserInferenceFallbackViewPolicy>(
            serde_json::to_value(&file).unwrap()
        )
        .is_err()
    );
    assert!(browser.matches_capture(&options).unwrap());
    browser.applicability = InferenceFallbackViewApplicability::ExactSettings {
        sha256: vec![basis.sha256.clone()],
    };
    assert!(browser.matches_capture(&options).unwrap());
    let mut changed = options.clone();
    changed.policy.maximum_analysis_age_ns += 1;
    assert!(!browser.matches_capture(&changed).unwrap());
    for hashes in [
        vec![],
        vec![basis.sha256.clone(); 2],
        vec!["invalid".into()],
        vec![basis.sha256; INFERENCE_EQUIVALENCE_MAX_CASES + 1],
    ] {
        browser.applicability =
            InferenceFallbackViewApplicability::ExactSettings { sha256: hashes };
        assert!(browser.validate().is_err());
    }
}

#[test]
fn browser_semantic_projection_refuses_unbound_or_unsupported_inputs() {
    let (_, options, _) = fixture();
    let mut changed = options.clone();
    changed.browser_analysis = None;
    assert!(browser_inference_semantic_identity(&changed).is_err());
    let mut changed = options.clone();
    changed.inference = None;
    assert!(browser_inference_semantic_identity(&changed).is_err());
    let mut changed = options.clone();
    changed
        .inference
        .as_mut()
        .unwrap()
        .profile
        .preprocessing
        .scale[0] = 0.5;
    assert!(browser_inference_semantic_identity(&changed).is_err());
    let mut changed = options;
    changed.exact_project_anchor = None;
    assert!(browser_inference_semantic_identity(&changed).is_err());
}
