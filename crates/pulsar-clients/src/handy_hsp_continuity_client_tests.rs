fn continuity_report() -> HandyHspContinuityStatus {
    HandyHspContinuityStatus {
        project_id: ProjectId::new("project").unwrap(),
        stream_request_id: RequestId::new("stream").unwrap(),
        enable_request_id: RequestId::new("enable").unwrap(),
        duration_ms: 8_000,
        history: None,
        native: None,
        error: None,
        qualified: false,
    }
}
fn continuity_api(value: HandyHspContinuityStatus) -> Api {
    Api {
        calls: Vec::new(),
        reply: Some(ResponseBody::HandyHspContinuity(Box::new(value))),
    }
}
fn continuity_intent() -> ContinuityIntent {
    ContinuityIntent {
        project: ProjectId::new("project").unwrap(),
        revision: RevisionId::new(2),
        stream_request_id: RequestId::new("stream").unwrap(),
        duration_ms: 8_000,
    }
}
#[test]
fn hsp_continuity_client_requires_consent_and_keeps_original_intent() {
    let directory = directory();
    let mut api = continuity_api(continuity_report());
    let value = enable_continuity(
        &mut api,
        directory.path(),
        RequestId::new("enable").unwrap(),
        &continuity_intent(),
        true,
    )
    .unwrap();
    assert!(value.history.is_none() && value.native.is_none() && !value.qualified);
    let [(id, command, project, revision)] = api.calls.as_slice() else {
        panic!("one original request")
    };
    assert_eq!(id.as_ref(), Some(&value.enable_request_id));
    assert_eq!(project.as_ref(), Some(&value.project_id));
    assert_eq!(*revision, Some(RevisionId::new(2)));
    assert!(
        matches!(command, Command::EnableHandyHspContinuity { stream_request_id, duration_ms: 8_000, allow_online_observations: true, .. } if stream_request_id == &value.stream_request_id)
    );
    for mode in 0..3 {
        let mut intent = continuity_intent();
        if mode == 1 {
            intent.duration_ms = 0;
        }
        if mode == 2 {
            intent.stream_request_id = RequestId::new("enable").unwrap();
        }
        let mut api = Api::default();
        assert!(enable_continuity(
            &mut api,
            Path::new("/absent-proof-directory"),
            RequestId::new("enable").unwrap(),
            &intent,
            mode != 0
        )
        .is_err());
        assert!(api.calls.is_empty());
    }
}
#[test]
fn hsp_continuity_client_rejects_changed_response_subject_or_duration() {
    let directory = directory();
    for mode in 0..5 {
        let mut value = continuity_report();
        match mode {
            0 => value.project_id = ProjectId::new("foreign").unwrap(),
            1 => value.stream_request_id = RequestId::new("other-stream").unwrap(),
            2 => value.enable_request_id = RequestId::new("other-enable").unwrap(),
            3 => value.duration_ms += 1,
            _ => value.qualified = true,
        }
        let mut api = continuity_api(value);
        assert!(enable_continuity(
            &mut api,
            directory.path(),
            RequestId::new("enable").unwrap(),
            &continuity_intent(),
            true
        )
        .is_err());
        assert_eq!(api.calls.len(), 1);
    }
}
#[test]
fn hsp_continuity_client_status_never_creates_identified_work() {
    let directory = directory();
    let value = continuity_report();
    let mut api = continuity_api(value.clone());
    let actual = continuity_status(
        &mut api,
        directory.path(),
        value.project_id.clone(),
        value.enable_request_id.clone(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(&value).unwrap()
    );
    let [(id, command, project, revision)] = api.calls.as_slice() else {
        panic!("one status read")
    };
    assert!(id.is_none() && revision.is_none());
    assert_eq!(project.as_ref(), Some(&value.project_id));
    assert!(
        matches!(command, Command::HandyHspContinuityStatus { original_request_id, .. } if original_request_id == &value.enable_request_id)
    );
}
#[test]
fn hsp_continuity_cli_requires_explicit_bounded_admission() {
    use clap::Parser;
    let args = [
        "pulsar",
        "handy-hsp-continuity-enable",
        "project",
        "--revision",
        "2",
        "--stream-request-id",
        "stream",
        "--duration-ms",
        "8000",
        "--request-id",
        "enable",
        "--allow-online-observations",
    ];
    assert!(matches!(
        crate::cli::Cli::try_parse_from(args).unwrap().command,
        Some(crate::cli::Commands::HandyHspContinuityEnable {
            duration_ms: 8000,
            allow_online_observations: true,
            ..
        })
    ));
    assert!(crate::cli::Cli::try_parse_from(&args[..args.len() - 1]).is_err());
    let mut invalid = args;
    invalid[8] = "0";
    assert!(crate::cli::Cli::try_parse_from(invalid).is_err());
    assert!(matches!(
        crate::cli::Cli::try_parse_from([
            "pulsar",
            "handy-hsp-continuity-status",
            "project",
            "--original-request-id",
            "enable"
        ])
        .unwrap()
        .command,
        Some(crate::cli::Commands::HandyHspContinuityStatus { .. })
    ));
}
