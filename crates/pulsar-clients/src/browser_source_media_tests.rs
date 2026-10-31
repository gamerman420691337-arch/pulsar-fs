use super::*;

#[test]
fn shutdown_reports_sanitized_source_cleanup_failure_and_preserves_it() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("private-missing.sock");
    let client = SessionClient::from_credentials(
        &socket,
        SessionId::new("source-test-actor").unwrap(),
        "synthetic-private-authentication".into(),
    )
    .unwrap();
    let b = binding();
    let mut media =
        MediaController::new(&client, b.project_id.clone(), b.revision, 30, None).unwrap();
    media.prepared = Some(Prepared {
        intent: intent(&b, Arc::new(AtomicBool::new(false))),
        report: None,
        ready: None,
        pending: 0,
        release_pending: false,
        cleanup_attempted: false,
        error: None,
    });
    let first = media.shutdown().unwrap_err().to_string();
    assert!(first.contains("source cleanup unconfirmed"));
    assert!(first.contains("transport_io"));
    assert!(first.contains("exchange"));
    assert!(!first.contains("private-missing.sock"));
    assert!(!first.contains("synthetic-private-authentication"));
    assert_eq!(media.shutdown().unwrap_err().to_string(), first);
}

#[test]
fn source_cleanup_preserves_first_failed_original_when_multiple_releases_fail() {
    let directory = tempfile::tempdir().unwrap();
    let client = SessionClient::from_credentials(
        &directory.path().join("missing.sock"),
        SessionId::new("source-test-actor").unwrap(),
        "synthetic-authentication".into(),
    )
    .unwrap();
    let b = binding();
    let mut media =
        MediaController::new(&client, b.project_id.clone(), b.revision, 30, None).unwrap();
    let prepared = |request: &str, selection: &str| {
        let mut original = intent(&b, Arc::new(AtomicBool::new(false)));
        original.request_id = RequestId::new(request).unwrap();
        original.selection_id = AttemptId::new(selection).unwrap();
        Prepared {
            intent: original,
            report: None,
            ready: None,
            pending: 0,
            release_pending: false,
            cleanup_attempted: false,
            error: None,
        }
    };
    media.prepared = Some(prepared("first-request", "first-selection"));
    media
        .retired
        .push(prepared("second-request", "second-selection"));
    let failure = media.shutdown().unwrap_err().to_string();
    assert!(failure.contains("first-request"), "{failure}");
    assert!(failure.contains("first-selection"), "{failure}");
    assert!(!failure.contains("second-request"), "{failure}");
    assert!(media.retired[0].intent.cancelled.load(Ordering::Acquire));
    assert_eq!(media.shutdown().unwrap_err().to_string(), failure);
}

#[test]
fn source_cleanup_diagnostics_classify_without_disclosing_error_text() {
    let cases = [
        (
            anyhow::Error::new(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "private token and path",
            ))
            .context(SourceFailureStage::Exchange),
            "exchange",
            "transport_timeout",
            None,
        ),
        (
            anyhow::Error::new(ProtocolError::new(
                ErrorCode::Forbidden,
                "private token and path",
            ))
            .context(SourceFailureStage::Exchange),
            "exchange",
            "protocol_rejected",
            Some(ErrorCode::Forbidden),
        ),
        (
            anyhow::anyhow!("private token and path").context(SourceFailureStage::Reply),
            "reply",
            "reply_validation",
            None,
        ),
        (
            anyhow::Error::new(TransportError::ResponseMismatch)
                .context(SourceFailureStage::Exchange),
            "exchange",
            "transport_framing",
            None,
        ),
    ];
    for (error, stage, category, code) in cases {
        let value = source_cleanup_diagnostic(&error);
        assert_eq!(
            value,
            serde_json::json!({"stage":stage,"category":category,"protocol_code":code})
        );
        assert!(!value.to_string().contains("private"));
    }
}

#[test]
fn replacement_preparation_detaches_prior_successor_without_losing_cleanup() {
    for dispatched in [true, false] {
        // Drive the actual controller routes with an authored native response and
        // an inspectable worker queue. No Engine, media pixels or device is used.
        let directory = tempfile::tempdir().unwrap();
        let original = binding();
        let page_cancelled = Arc::new(AtomicBool::new(false));
        let prior = intent(&original, page_cancelled.clone());
        prior.released.store(true, Ordering::Release);
        prior.cancelled.store(true, Ordering::Release);
        let mut child = intent(&original, page_cancelled.clone());
        child.dispatched.store(dispatched, Ordering::Release);
        child.request_id = RequestId::new("prior-successor-request").unwrap();
        child.predecessor = Some(original.clone());
        child.cancelled.store(true, Ordering::Release);
        let old_deadline = child.expires;
        let child_id = child.request_id.clone();
        let child_selection = child.selection_id.clone();
        let prepared = |intent, pending| Prepared {
            intent,
            report: None,
            ready: None,
            pending,
            release_pending: false,
            cleanup_attempted: false,
            error: None,
        };
        let (operations, queued) = mpsc::sync_channel(4);
        let (events, received) = mpsc::channel();
        let generation = AttemptId::new("replacement-catalog").unwrap();
        let entry = AttemptId::new("replacement-entry").unwrap();
        let mut controller = MediaController {
            project: original.project_id.clone(),
            revision: original.revision,
            end: Instant::now() + Duration::from_secs(120),
            lease_cap_ms: 30_000,
            catalog: Some(Catalog {
                page: "page".into(),
                generation: generation.clone(),
                value: BrowserMediaCatalog {
                    engine_epoch: original.engine_epoch.clone(),
                    project_id: original.project_id.clone(),
                    revision: original.revision,
                    entries: vec![BrowserMediaCatalogEntry {
                        label: "authored".into(),
                        selection: original.selection.clone(),
                    }],
                },
                entries: BTreeMap::from([(entry.to_string(), original.selection.clone())]),
            }),
            prepared: Some(prepared(prior, 0)),
            successor: Some(prepared(child, 1)),
            retired: Vec::new(),
            route: Arc::new(Mutex::new(None)),
            operations: Some(operations),
            events: received,
            ranges: Vec::new(),
            threads: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
            range_slots: Arc::new(AtomicUsize::new(0)),
            next_range: AtomicUsize::new(0),
            cleanup: SessionClient::from_credentials(
                &directory.path().join("missing.sock"),
                SessionId::new("authored-replacement").unwrap(),
                "authored-authentication".into(),
            )
            .unwrap(),
            // No native worker was started; the queue and replies are driven here.
            shutdown_done: true,
            shutdown_error: None,
        };
        let scope = Scope {
            page: "page".into(),
            ..Scope::default()
        };
        let body = serde_json::to_vec(&serde_json::json!({
            "catalog_generation":generation, "entry_id":entry,
            "presentation_anchor_us":"5000000", "elapsed_anchor_us":"1000000",
            "allow_declared_clock":true
        }))
        .unwrap();
        assert!(matches!(
            controller
                .handle("/media/prepare", &scope, page_cancelled.clone(), &body, 1)
                .unwrap(),
            MediaReply::Pending
        ));
        assert!(
            controller.successor.is_none(),
            "replacement inherited the original successor"
        );
        assert_eq!(controller.retired.len(), 1);
        assert_eq!(controller.retired[0].intent.request_id, child_id);
        assert_eq!(controller.retired[0].intent.expires, old_deadline);
        assert_eq!(controller.retired[0].pending, 1);
        assert!(!controller.retired[0]
            .intent
            .released
            .load(Ordering::Acquire));
        let Operation::Preparation {
            intent: new,
            kind: OpKind::Prepare,
            ..
        } = queued.try_recv().unwrap()
        else {
            panic!("fresh preparation was not queued")
        };
        assert!(new.predecessor.is_none());
        let active = controller.prepared.as_mut().unwrap();
        let mut fresh_binding = original.clone();
        fresh_binding.preparation_request_id = new.request_id.clone();
        fresh_binding.lease_id = ArtifactId::new("new-parent-lease").unwrap();
        fresh_binding.lease_ms = new.lease_ms;
        active.pending = 0;
        active.ready = Some(Arc::new(ReadySource {
            intent: active.intent.clone(),
            binding: fresh_binding.clone(),
            token: "b".repeat(64),
            mime: "video/mp4",
        }));
        let continuation =
            serde_json::to_vec(&serde_json::json!({"selection_id":new.selection_id})).unwrap();
        assert!(matches!(
            controller
                .handle("/media/continue", &scope, page_cancelled, &continuation, 2)
                .unwrap(),
            MediaReply::Pending
        ));
        let Operation::Preparation {
            intent: next,
            kind: OpKind::Prepare,
            ..
        } = queued.try_recv().unwrap()
        else {
            panic!("new continuation reused an old intent")
        };
        assert_ne!(next.request_id, child_id);
        assert_eq!(next.predecessor.as_ref(), Some(&fresh_binding));
        assert_eq!(next.selection_id, new.selection_id);
        events
            .send(Event {
                ticket: None,
                page: "page".into(),
                selection_id: Some(child_selection),
                request_id: Some(child_id.clone()),
                result: Err(anyhow::anyhow!("authored late original failure")),
            })
            .unwrap();
        assert!(controller.poll().is_empty());
        assert_eq!(controller.retired[0].pending, 0);
        assert_eq!(controller.successor.as_ref().unwrap().pending, 1);
        controller.tick();
        let Operation::Preparation {
            intent: release,
            kind: OpKind::Release,
            ..
        } = queued.try_recv().unwrap()
        else {
            panic!("original cleanup was lost")
        };
        assert_eq!(release.request_id, child_id);
        assert_eq!(release.expires, old_deadline);
        assert!(release.cancelled.load(Ordering::Acquire));
        assert_eq!(controller.retired[0].pending, 1);
        assert_eq!(release.dispatched.load(Ordering::Acquire), dispatched);
        if !dispatched {
            // The worker's local-only release never calls a native command.
            release.released.store(true, Ordering::Release);
            events
                .send(Event {
                    ticket: None,
                    page: "page".into(),
                    selection_id: Some(release.selection_id.clone()),
                    request_id: Some(release.request_id.clone()),
                    result: Ok(Payload::LocalRelease),
                })
                .unwrap();
            assert!(controller.poll().is_empty());
            controller.tick();
            assert!(controller.retired.is_empty());
        }
        // A saturated cleanup set must refuse before replacing either live slot.
        for n in controller.retired.len()..4 {
            let mut extra = intent(&original, Arc::new(AtomicBool::new(false)));
            extra.request_id = RequestId::new(format!("unsettled-cleanup-{n}")).unwrap();
            controller.retired.push(prepared(extra, 1));
        }
        let current = controller.prepared.as_mut().unwrap();
        current.intent.released.store(true, Ordering::Release);
        current.ready = None;
        let current_id = current.intent.request_id.clone();
        let successor_id = controller
            .successor
            .as_ref()
            .unwrap()
            .intent
            .request_id
            .clone();
        let error = controller
            .handle(
                "/media/prepare",
                &scope,
                Arc::new(AtomicBool::new(false)),
                &body,
                3,
            )
            .err()
            .unwrap();
        assert!(error.to_string().contains("cleanup must settle"));
        assert_eq!(controller.retired.len(), 4);
        assert_eq!(
            controller.prepared.as_ref().unwrap().intent.request_id,
            current_id
        );
        assert_eq!(
            controller.successor.as_ref().unwrap().intent.request_id,
            successor_id
        );
    }
}

fn binding() -> BrowserMediaBinding {
    BrowserMediaBinding {
        preparation_request_id: RequestId::new("prepare-source").unwrap(),
        lease_id: ArtifactId::new("native-source-lease").unwrap(),
        engine_epoch: AttemptId::new("source-engine-boot").unwrap(),
        project_id: ProjectId::new("source-project").unwrap(),
        revision: RevisionId::new(4),
        selection: BrowserMediaSelection {
            source_version: SourceVersionId::new("source-version").unwrap(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("source-bytes").unwrap(),
                sha256: "a".repeat(64),
                byte_len: 100,
            },
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("source-placement").unwrap(),
                created_revision: RevisionId::new(3),
                spec: SourcePlacementSpec {
                    source_version: SourceVersionId::new("source-version").unwrap(),
                    source_start: SourceTimestamp::new(1, 3).unwrap(),
                    source_end: SourceTimestamp::new(7, 3).unwrap(),
                    source_anchor: SourceTimestamp::new(1, 1).unwrap(),
                    project_anchor: ProjectTime::from_nanos(-500_000_000),
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
            },
        },
        clock: DeclaredSourceClock {
            presentation_anchor: SourceTimestamp::new(5, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(1, 1).unwrap(),
        },
        lease_ms: 60_000,
    }
}
fn options() -> LiveCaptureOptions {
    LiveCaptureOptions {
        inference_fallback: None,
        browser_analysis: None,
        browser_media: None,
        model_pack: None,
        source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
        continuation_mode: LiveContinuationMode::Immediate,
        encoded_source: None,
        inference: None,
        policy: LivePolicy {
            maximum_analysis_age_ns: 1_000_000_000,
            added_buffer_ns: 0,
        },
        source_anchor: SourceTimestamp::new(0, 1).unwrap(),
        project_anchor: ProjectTime::ZERO,
        exact_project_anchor: None,
        presentation_anchor: LiveClockTick(0),
        lease_ms: 1000,
    }
}
fn intent(binding: &BrowserMediaBinding, page_cancelled: Arc<AtomicBool>) -> Intent {
    Intent {
        predecessor: None,
        selection_id: AttemptId::new("native-selection").unwrap(),
        request_id: binding.preparation_request_id.clone(),
        engine_epoch: binding.engine_epoch.clone(),
        project: binding.project_id.clone(),
        revision: binding.revision,
        selection: binding.selection.clone(),
        clock: binding.clock,
        lease_ms: binding.lease_ms,
        page: "page".into(),
        page_cancelled,
        cancelled: Arc::new(AtomicBool::new(false)),
        dispatched: Arc::new(AtomicBool::new(true)),
        released: Arc::new(AtomicBool::new(false)),
        expires: Instant::now() + Duration::from_secs(60),
    }
}
#[test]
fn single_ranges_are_checked_and_preserve_open_suffix_and_full_semantics() {
    assert_eq!(byte_range(None, 100).unwrap(), None);
    for (range, expected) in [
        ("bytes=0-0", (0, 0)),
        ("bytes=20-40", (20, 40)),
        ("bytes=20-", (20, 99)),
        ("bytes=-20", (80, 99)),
        ("bytes=-999", (0, 99)),
        ("bytes=90-1000", (90, 99)),
    ] {
        assert_eq!(byte_range(Some(range), 100).unwrap(), Some(expected));
    }
    for range in [
        "bytes=100-",
        "bytes=50-20",
        "bytes=-0",
        "bytes=",
        "bytes=-",
        "bytes=+1-2",
        "bytes=0-+2",
        "bytes=0-1,3-4",
        "items=0-1",
        "bytes=0 -1",
        "bytes=18446744073709551616-",
    ] {
        assert!(byte_range(Some(range), 100).is_err(), "{range}");
    }
    assert!(byte_range(Some("bytes=0-"), 0).is_err());
}
#[test]
fn declared_anchor_strings_never_round_through_javascript_numbers() {
    assert_eq!(
        anchor("9007199254740993").unwrap(),
        SourceTimestamp::new(9_007_199_254_740_993, 1_000_000).unwrap()
    );
    assert_eq!(
        anchor("9223372036854775807").unwrap(),
        SourceTimestamp::new(i64::MAX, 1_000_000).unwrap()
    );
    for value in ["", "-1", "+1", "1.5", " 1", "1e6", "9223372036854775808"] {
        assert!(anchor(value).is_err());
    }
}
#[test]
fn media_mime_comes_from_admitted_bytes_never_label_or_html() {
    assert_eq!(
        media_mime(b"\x1a\x45\xdf\xa3\x42\x82\x84webm").unwrap(),
        "video/webm"
    );
    assert_eq!(
        media_mime(b"\0\0\0\x18ftypisom\0\0\0\0").unwrap(),
        "video/mp4"
    );
    for bytes in [
        b"<html>webm</html>".as_slice(),
        b"YUV4MPEG2 W64 H64 F8:1\n",
        b"unknown.mp4",
        b"\x1a\x45\xdf\xa3matroska",
    ] {
        assert!(media_mime(bytes).is_err());
    }
}
#[test]
fn first_eligible_frame_composes_declared_offset_placement_and_negative_project_anchor() {
    let binding = binding();
    binding.validate().unwrap();
    let mut options = options();
    set_browser_capture_anchor(
        &mut options,
        4_500_000,
        LiveSourceTimeBasis::SourceMediaPresentation,
        Some(&binding),
    )
    .unwrap();
    assert_eq!(
        ExactSeconds::from_source(options.source_anchor),
        ExactSeconds::new(9, 2).unwrap()
    );
    assert_eq!(
        options.project_anchor,
        ProjectTime::from_nanos(-1_000_000_000)
    );
    assert_eq!(
        options.exact_project_anchor,
        Some(ExactSeconds::new(-1, 1).unwrap())
    );
    assert_eq!(options.browser_media, Some(binding.clone()));
    // The declared trim begins earlier (-7/6 project seconds); no reanchor to
    // the trim boundary or zero silently removes that leading missing coverage.
    assert_eq!(
        binding
            .selection
            .placement
            .spec
            .project_at_source(ExactSeconds::new(1, 3).unwrap())
            .unwrap(),
        ExactSeconds::new(-7, 6).unwrap()
    );
    for b in [4_333_333, 6_333_334, -1] {
        assert!(set_browser_capture_anchor(
            &mut options,
            b,
            LiveSourceTimeBasis::SourceMediaPresentation,
            Some(&binding)
        )
        .is_err());
    }
    assert!(set_browser_capture_anchor(
        &mut options,
        4_500_000,
        LiveSourceTimeBasis::CameraCapturePresentation,
        Some(&binding)
    )
    .is_err());
}
#[test]
fn unbound_capture_discards_any_serialized_launch_binding() {
    let mut options = options();
    options.browser_media = Some(binding());
    set_browser_capture_anchor(
        &mut options,
        250_000,
        LiveSourceTimeBasis::CameraCapturePresentation,
        None,
    )
    .unwrap();
    assert!(options.browser_media.is_none());
    assert_eq!(
        options.source_time_basis,
        LiveSourceTimeBasis::CameraCapturePresentation
    );
    assert_eq!(
        ExactSeconds::from_source(options.source_anchor),
        ExactSeconds::new(1, 4).unwrap()
    );
}
#[test]
fn prepared_read_lifetime_is_separate_from_player_scope_and_never_renews_on_view() {
    let binding = binding();
    let cancelled = Arc::new(AtomicBool::new(false));
    let intent = intent(&binding, cancelled.clone());
    let expires = intent.expires;
    let ready = Arc::new(ReadySource {
        intent: intent.clone(),
        binding: binding.clone(),
        token: "b".repeat(64),
        mime: "video/webm",
    });
    let report = BrowserMediaPreparationReport {
        request_id: intent.request_id.clone(),
        engine_epoch: intent.engine_epoch.clone(),
        project_id: intent.project.clone(),
        revision: intent.revision,
        selection: intent.selection.clone(),
        clock: intent.clock,
        lease_ms: intent.lease_ms,
        state: BrowserMediaPreparationState::Ready,
        verified_bytes: 100,
        binding: Some(binding),
        current: true,
        error: None,
    };
    let prepared = Prepared {
        intent: intent.clone(),
        report: Some(report),
        ready: Some(ready.clone()),
        pending: 0,
        release_pending: false,
        cleanup_attempted: false,
        error: None,
    };
    let first = MediaController::view(&prepared).unwrap();
    let second = MediaController::view(&prepared).unwrap();
    assert_eq!(first, second);
    assert!(ready.current());
    assert_eq!(ready.intent.expires, expires);
    assert_eq!(first["trim_start_us"], 4_333_334);
    assert_eq!(first["trim_end_us"], 6_333_334);
    assert!(first.get("binding").is_none());
    assert!(first.get("clock").is_none());
    // Exercise the real player cancellation against the page-level read lifetime.
    // This fixture has no Engine; its independent timing Open must be reported
    // unconfirmed without retiring the prepared page-level read relationship.
    let endpoint_directory = tempfile::tempdir().unwrap();
    let client = SessionClient::from_credentials(
        &endpoint_directory.path().join("missing.sock"),
        SessionId::new("source-test-actor").unwrap(),
        "synthetic-authentication".into(),
    )
    .unwrap();
    let mut players = crate::browser_player_control::PlayerControl::new(
        &client,
        intent.project.clone(),
        intent.revision,
        None,
    )
    .unwrap();
    let page = players.page().unwrap();
    let scope = Scope {
        launch: page["launch_id"].as_str().unwrap().into(),
        page: page["page_id"].as_str().unwrap().into(),
        page_capability: page["page_capability"].as_str().unwrap().into(),
        ..Scope::default()
    };
    let read_cancel = players.media_page(&scope).unwrap();
    let mut selected = ready.intent.clone();
    selected.page_cancelled = read_cancel;
    let read = ReadySource {
        intent: selected,
        binding: ready.binding.clone(),
        token: "c".repeat(64),
        mime: "video/webm",
    };
    let body=serde_json::to_vec(&serde_json::json!({"source_id":intent.selection_id,"media_selection_id":intent.selection_id,"generation":1,"time_basis":"source_media_presentation"})).unwrap();
    let reply = players
        .handle_with_media(
            "/player/start",
            &scope,
            &body,
            1,
            Some(read.binding.clone()),
        )
        .unwrap();
    let crate::browser_player_control::ControlReply::Ready(started) = reply else {
        panic!("player registration must be synchronous");
    };
    let player_scope = Scope {
        player: Some(started["player"]["player_id"].as_str().unwrap().into()),
        source: Some(started["player"]["source_id"].as_str().unwrap().into()),
        generation: Some(1),
        player_capability: Some(started["player_capability"].as_str().unwrap().into()),
        ..scope.clone()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match players.handle("/player/timing-probe", &player_scope, b"", 2) {
            Err(error) => {
                assert!(error.to_string().contains("open_unconfirmed"));
                break;
            }
            Ok(crate::browser_player_control::ControlReply::Ready(report)) => {
                assert_eq!(report["pending"], true);
            }
            Ok(_) => panic!("absent Engine cannot provide timing"),
        }
        assert!(Instant::now() < deadline, "timing Open did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(read.current());
    players.stop();
    assert!(read.current());
    let cleanup = players.shutdown().unwrap_err().to_string();
    assert!(cleanup.contains("Browser timing cleanup unconfirmed"));
    assert!(cleanup.contains("open_unconfirmed"));
    assert!(!read.current());
    intent.cancelled.store(true, Ordering::Release);
    assert!(!ready.current());
    let retired = MediaController::view(&prepared).unwrap();
    assert_eq!(retired["current"], false);
    assert!(retired.get("url").is_none());
    prepared.intent.released.store(true, Ordering::Release);
    assert_eq!(
        MediaController::view(&prepared).unwrap()["state"],
        "cancelled"
    );
}
#[test]
fn exact_source_reply_validation_refuses_changed_clock_placement_or_original_identity() {
    let binding = binding();
    let intent = intent(&binding, Arc::new(AtomicBool::new(false)));
    let good = BrowserMediaPreparationReport {
        request_id: intent.request_id.clone(),
        engine_epoch: intent.engine_epoch.clone(),
        project_id: intent.project.clone(),
        revision: intent.revision,
        selection: intent.selection.clone(),
        clock: intent.clock,
        lease_ms: intent.lease_ms,
        state: BrowserMediaPreparationState::Pending,
        verified_bytes: 0,
        binding: None,
        current: true,
        error: None,
    };
    checked_report(
        pulsar_protocol::ResponseBody::BrowserMediaPreparation(Box::new(good.clone())),
        &intent,
    )
    .unwrap();
    for case in 0..5 {
        let mut bad = good.clone();
        match case {
            0 => bad.request_id = RequestId::new("other-original").unwrap(),
            1 => bad.project_id = ProjectId::new("other-project").unwrap(),
            2 => {
                bad.selection.placement.placement_id =
                    SourcePlacementId::new("other-placement").unwrap()
            }
            3 => bad.clock.presentation_anchor = SourceTimestamp::new(6, 1).unwrap(),
            _ => bad.lease_ms += 1,
        };
        assert!(checked_report(
            pulsar_protocol::ResponseBody::BrowserMediaPreparation(Box::new(bad)),
            &intent
        )
        .is_err());
    }
}

#[test]
fn refused_declared_clock_can_be_released_without_inventing_an_original_operation() {
    let client = SessionClient::from_credentials(
        std::path::Path::new("/unused/source-control.sock"),
        SessionId::new("source-test-actor").unwrap(),
        "synthetic-authentication".into(),
    )
    .unwrap();
    let mut media = MediaController::new(
        &client,
        ProjectId::new("source-project").unwrap(),
        RevisionId::new(4),
        30,
        None,
    )
    .unwrap();
    let scope = Scope {
        page: "page".into(),
        ..Scope::default()
    };
    let body=br#"{"catalog_generation":"catalog","entry_id":"entry","presentation_anchor_us":"0","elapsed_anchor_us":"0","allow_declared_clock":false}"#;
    assert!(media
        .handle(
            "/media/prepare",
            &scope,
            Arc::new(AtomicBool::new(false)),
            body,
            1
        )
        .is_err());
    assert!(media.prepared.is_none());
    let MediaReply::Ready(value) = media
        .handle(
            "/media/release",
            &scope,
            Arc::new(AtomicBool::new(false)),
            br#"{"selection_id":null}"#,
            2,
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        value,
        serde_json::json!({"selection_id":null,"state":"cancelled","current":false,"not_dispatched":true})
    );
    media.shutdown().unwrap();
    assert!(media.threads.is_empty());
}

#[test]
fn committed_media_route_changes_only_future_reads_without_reviving_old_expiry() {
    let old_binding = binding();
    let page = Arc::new(AtomicBool::new(false));
    let mut old_intent = intent(&old_binding, page.clone());
    old_intent.expires = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let old_deadline = old_intent.expires;
    let old = Arc::new(ReadySource {
        intent: old_intent,
        binding: old_binding.clone(),
        token: "a".repeat(64),
        mime: "video/webm",
    });
    let mut next_binding = old_binding.clone();
    next_binding.preparation_request_id = RequestId::new("next-media-prepare").unwrap();
    next_binding.lease_id = ArtifactId::new("next-media-lease").unwrap();
    let mut next_intent = intent(&next_binding, page.clone());
    next_intent.predecessor = Some(old_binding);
    let next = Arc::new(ReadySource {
        intent: next_intent,
        binding: next_binding,
        token: old.token.clone(),
        mime: old.mime,
    });
    let route = Mutex::new(Some(old.clone()));
    assert!(selected_range_source(&route, &old).is_err());
    // Authored local publication models only an already native-committed route.
    *route.lock().unwrap() = Some(next.clone());
    assert!(Arc::ptr_eq(
        &selected_range_source(&route, &old).unwrap(),
        &next
    ));
    assert!(!old.current());
    assert_eq!(old.intent.expires, old_deadline);
    assert_ne!(
        old.binding.preparation_request_id,
        next.binding.preparation_request_id
    );
    page.store(true, Ordering::Release);
    assert!(selected_range_source(&route, &old).is_err());
}
#[test]
fn media_route_never_changes_original_response_bytes_mapping_scope_or_url() {
    let binding = binding();
    let original = ReadySource {
        intent: intent(&binding, Arc::new(AtomicBool::new(false))),
        binding,
        token: "a".repeat(64),
        mime: "video/webm",
    };
    for case in 0..7 {
        let mut next = original.clone();
        match case {
            0 => next.binding.selection.source.sha256 = "b".repeat(64),
            1 => next.binding.clock.presentation_anchor = SourceTimestamp::new(6, 1).unwrap(),
            2 => next.binding.project_id = ProjectId::new("foreign-project").unwrap(),
            3 => next.binding.revision = RevisionId::new(5),
            4 => next.binding.engine_epoch = AttemptId::new("foreign-engine").unwrap(),
            5 => next.token = "b".repeat(64),
            _ => next.mime = "video/mp4",
        }
        assert!(
            selected_range_source(&Mutex::new(Some(Arc::new(next))), &original).is_err(),
            "case {case}"
        );
    }
}

#[test]
fn explicit_media_cap_gives_new_leases_without_changing_original_deadlines() {
    let admitted = Instant::now();
    let server_end = admitted + Duration::from_secs(180);
    assert_eq!(
        selected_media_lease(server_end, admitted, BROWSER_MEDIA_MAX_LEASE_MS).unwrap(),
        180_000
    );
    let first_ms = selected_media_lease(server_end, admitted, 30_000).unwrap();
    let original_deadline = admitted + Duration::from_millis(u64::from(first_ms));
    let successor_admission = admitted + Duration::from_secs(15);
    let successor_ms = selected_media_lease(server_end, successor_admission, 30_000).unwrap();
    assert_eq!(first_ms, 30_000);
    assert_eq!(successor_ms, 30_000);
    assert_eq!(original_deadline, admitted + Duration::from_secs(30));
    assert_eq!(
        successor_admission + Duration::from_millis(u64::from(successor_ms)),
        admitted + Duration::from_secs(45)
    );
    assert_eq!(
        selected_media_lease(server_end, admitted + Duration::from_secs(175), 30_000).unwrap(),
        5_000
    );
    assert!(selected_media_lease(server_end, server_end, 30_000).is_err());
    assert!(
        selected_media_lease(server_end, server_end + Duration::from_millis(1), 30_000).is_err()
    );
    for invalid in [0, 600_001, u32::MAX] {
        assert!(selected_media_lease(server_end, admitted, invalid).is_err());
    }
    assert_eq!(
        selected_media_lease(admitted + Duration::from_secs(1000), admitted, 600_000).unwrap(),
        600_000
    );
}
