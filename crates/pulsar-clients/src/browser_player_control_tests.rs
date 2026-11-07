//! Process-local authority tests. No Engine connection or physical-device claim.
use super::*;
use std::path::Path;

fn control() -> PlayerControl {
    let client = SessionClient::from_credentials(
        Path::new("/mnt/4tb/tmp/pulsar-player-control-tests-unused.sock"),
        SessionId::new("synthetic-browser-test-session").unwrap(),
        "synthetic-browser-test-credential".into(),
    )
    .unwrap();
    PlayerControl::new(
        &client,
        ProjectId::new("synthetic-browser-test-project").unwrap(),
        RevisionId::new(0),
        None,
    )
    .unwrap()
}

fn page(control: &mut PlayerControl) -> Scope {
    let value = control.page().unwrap();
    Scope {
        launch: value["launch_id"].as_str().unwrap().into(),
        page: value["page_id"].as_str().unwrap().into(),
        page_capability: value["page_capability"].as_str().unwrap().into(),
        ..Scope::default()
    }
}

fn start_body(source: &str, generation: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "source_id": source,
        "generation": generation,
        "time_basis": "source_media_presentation",
    }))
    .unwrap()
}

fn start(control: &mut PlayerControl, page: &Scope, source: &str, generation: u64) -> Scope {
    let reply = control
        .handle("/player/start", page, &start_body(source, generation), 1)
        .unwrap();
    let ControlReply::Ready(value) = reply else {
        panic!("local registration must not wait on Engine work")
    };
    assert_eq!(value["guard"]["configured"], false);
    assert_eq!(value["guard"]["pending"], false);
    assert_eq!(value["guard"]["current"], false);
    assert_eq!(value["heartbeat_interval_ms"], 100);
    Scope {
        player: Some(value["player"]["player_id"].as_str().unwrap().into()),
        source: Some(value["player"]["source_id"].as_str().unwrap().into()),
        generation: value["player"]["generation"].as_u64(),
        player_capability: Some(value["player_capability"].as_str().unwrap().into()),
        ..page.clone()
    }
}

#[test]
fn page_get_never_replaces_or_revokes_an_active_player() {
    let mut control = control();
    let first_page = page(&mut control);
    let first = start(&mut control, &first_page, "source-one", 1);
    let permit = control.authorize(&first, true).unwrap();
    let second_page = page(&mut control);
    assert_ne!(first_page.page, second_page.page);
    assert_ne!(first_page.page_capability, second_page.page_capability);
    assert!(control.active());
    assert!(control.still_matches(&permit));
    assert!(control.authorize(&second_page, true).is_err());
    assert!(control.authorize(&second_page, false).is_err());
    assert!(control
        .handle(
            "/player/start",
            &second_page,
            &start_body("source-two", 2),
            2
        )
        .is_err());
    assert_eq!(
        control.player.as_ref().unwrap().identity,
        permit.identity.unwrap()
    );
}

#[test]
fn all_launch_page_player_source_generation_and_capability_fields_are_exact() {
    let mut control = control();
    let page = page(&mut control);
    let scope = start(&mut control, &page, "source-one", 7);
    let mut cases = Vec::new();
    let mut bad = scope.clone();
    bad.launch.push('x');
    cases.push(bad);
    let mut bad = scope.clone();
    bad.page.push('x');
    cases.push(bad);
    let mut bad = scope.clone();
    bad.page_capability.push('x');
    cases.push(bad);
    let mut bad = scope.clone();
    bad.player = Some("other-player".into());
    cases.push(bad);
    let mut bad = scope.clone();
    bad.source = Some("other-source".into());
    cases.push(bad);
    let mut bad = scope.clone();
    bad.generation = Some(8);
    cases.push(bad);
    let mut bad = scope.clone();
    bad.player_capability = Some("other-capability".into());
    cases.push(bad);
    let mut bad = scope.clone();
    bad.player = None;
    cases.push(bad);
    for bad in cases {
        let before = control.player.as_ref().unwrap().heartbeat;
        assert!(control.authorize(&bad, true).is_err());
        assert!(control.authorize(&bad, false).is_err());
        assert!(control.handle("/player/challenge", &bad, &[], 3).is_err());
        assert!(control.handle("/player/stop", &bad, &[], 4).is_err());
        assert_eq!(control.player.as_ref().unwrap().heartbeat, before);
        assert!(control.active());
    }
}

#[test]
fn retired_generation_cannot_be_replayed_and_replacement_changes_native_player() {
    let mut control = control();
    let page = page(&mut control);
    let first = start(&mut control, &page, "source-one", 7);
    control.handle("/player/stop", &first, &[], 2).unwrap();
    for generation in [0, 6, 7] {
        assert!(control
            .handle(
                "/player/start",
                &page,
                &start_body("source-one", generation),
                3
            )
            .is_err());
        assert!(!control.active());
    }
    let second = start(&mut control, &page, "source-one", 8);
    assert_ne!(first.player, second.player);
    assert_ne!(first.player_capability, second.player_capability);
    control.handle("/player/stop", &second, &[], 4).unwrap();
    let third = start(&mut control, &page, "new-source", 1);
    assert_ne!(second.player, third.player);
    assert_eq!(third.source.as_deref(), Some("new-source"));
}

#[test]
fn old_stop_challenge_and_pagehide_compatibility_stop_are_inert_after_replacement() {
    let mut control = control();
    let first_page = page(&mut control);
    let old = start(&mut control, &first_page, "source-one", 1);
    control.handle("/player/stop", &old, &[], 2).unwrap();
    let new_page = page(&mut control);
    let new = start(&mut control, &new_page, "source-two", 2);
    let permit = control.authorize(&new, true).unwrap();
    let before = control.player.as_ref().unwrap().heartbeat;
    for route in ["/player/stop", "/player/challenge", "/player/observe"] {
        assert!(control.handle(route, &old, &[], 3).is_err());
    }
    // HTTP /stop checks this permit before invoking the compatibility stop.
    assert!(control.authorize(&old, false).is_err());
    assert!(control.authorize(&first_page, false).is_err());
    assert_eq!(control.player.as_ref().unwrap().heartbeat, before);
    assert!(control.active());
    assert!(control.still_matches(&permit));
}

#[test]
fn retired_editor_scope_is_usable_only_until_an_explicit_replacement() {
    let mut control = control();
    let page = page(&mut control);
    let first = start(&mut control, &page, "source-one", 1);
    let live = control.authorize(&first, true).unwrap();
    control.handle("/player/stop", &first, &[], 2).unwrap();
    assert!(!live.current());
    assert!(!control.still_matches(&live));
    assert!(control.authorize(&first, true).is_err());
    let editor = control.authorize(&first, false).unwrap();
    assert!(control.still_matches(&editor));
    let second = start(&mut control, &page, "source-two", 2);
    assert!(!editor.current());
    assert!(!control.still_matches(&editor));
    assert!(control.authorize(&first, false).is_err());
    control.handle("/player/stop", &second, &[], 3).unwrap();
    assert!(control.authorize(&first, false).is_err());
    assert!(control.authorize(&second, false).is_ok());
}

#[test]
fn queued_page_and_player_work_are_cancelled_before_replacement_can_publish() {
    let mut control = control();
    let page = page(&mut control);
    let page_job = control.authorize(&page, false).unwrap();
    let first = start(&mut control, &page, "source-one", 1);
    assert!(!page_job.current());
    assert!(!control.still_matches(&page_job));
    let frame = control.authorize(&first, true).unwrap();
    let editor = control.authorize(&first, false).unwrap();
    control.stop();
    assert!(!frame.current());
    assert!(editor.current());
    let second = start(&mut control, &page, "source-two", 2);
    assert!(!editor.current());
    assert!(!control.still_matches(&editor));
    assert!(control.authorize(&second, true).unwrap().current());
}

#[test]
fn unconfigured_guard_heartbeat_is_local_and_never_fabricates_an_observation() {
    let mut control = control();
    let page = page(&mut control);
    let scope = start(&mut control, &page, "source-one", 1);
    let old = Instant::now() - Duration::from_millis(50);
    control.player.as_mut().unwrap().heartbeat = old;
    let ControlReply::Ready(value) = control.handle("/player/challenge", &scope, &[], 2).unwrap()
    else {
        panic!("unconfigured guard has no native operation to wait for")
    };
    assert_eq!(value, json!({"pending":true,"configured":false}));
    assert!(control.player.as_ref().unwrap().heartbeat > old);
    assert_eq!(control.player.as_ref().unwrap().sequence, 0);
    assert!(control.player.as_ref().unwrap().binding.is_none());
    assert!(control.native.is_none());
    assert!(control.poll().is_empty());
}

#[test]
fn rejected_control_requests_do_not_renew_local_heartbeat() {
    let mut control = control();
    let page = page(&mut control);
    let scope = start(&mut control, &page, "source-one", 1);
    let old = Instant::now() - Duration::from_millis(50);
    control.player.as_mut().unwrap().heartbeat = old;
    for (route, body) in [
        ("/player/challenge", b"not-empty".as_slice()),
        ("/player/unknown", b"".as_slice()),
        ("/player/observe", b"{}".as_slice()),
        ("/player/stop", b"not-empty".as_slice()),
    ] {
        assert!(control.handle(route, &scope, body, 2).is_err());
        assert_eq!(control.player.as_ref().unwrap().heartbeat, old, "{route}");
        assert!(control.active());
    }
}

#[test]
fn local_expiry_revokes_live_permits_and_cannot_be_revived_by_challenge() {
    let mut control = control();
    let page = page(&mut control);
    let scope = start(&mut control, &page, "source-one", 1);
    let permit = control.authorize(&scope, true).unwrap();
    control.player.as_mut().unwrap().heartbeat =
        Instant::now() - LOCAL_HEARTBEAT - Duration::from_millis(1);
    assert!(control.tick());
    assert!(!control.active());
    assert!(!permit.current());
    assert!(!control.tick());
    assert!(control.handle("/player/challenge", &scope, &[], 3).is_err());
    assert!(!control.active());
}

#[test]
fn malformed_or_out_of_bounds_registration_preserves_retired_identity() {
    let mut control = control();
    let page = page(&mut control);
    let scope = start(&mut control, &page, "source-one", 1);
    control.stop();
    let retained = control.authorize(&scope, false).unwrap();
    for body in [
        br#"{"source_id":"source-two","generation":2,"time_basis":"source_media_presentation","extra":true}"#.to_vec(),
        start_body("source-two", i64::MAX as u64 + 1),
        b"not-json".to_vec(),
    ] {
        assert!(control.handle("/player/start", &page, &body, 2).is_err());
        assert!(!control.active());
        assert!(control.still_matches(&retained));
    }
}

#[test]
fn page_admission_is_bounded_without_replacing_an_existing_player() {
    let mut control = control();
    let page = page(&mut control);
    let scope = start(&mut control, &page, "source-one", 1);
    let permit = control.authorize(&scope, true).unwrap();
    for _ in 1..16 {
        let value = control.page().unwrap();
        let serialized = value.to_string();
        assert!(!serialized.contains("synthetic-browser-test-credential"));
        assert!(!serialized.contains("synthetic-browser-test-session"));
    }
    assert!(control.page().is_err());
    assert_eq!(control.pages.len(), 16);
    assert!(control.active());
    assert!(control.still_matches(&permit));
}

#[test]
fn shutdown_revokes_queued_editor_work_and_all_issued_page_capabilities() {
    let mut control = control();
    let page = page(&mut control);
    let scope = start(&mut control, &page, "source-one", 1);
    let live = control.authorize(&scope, true).unwrap();
    let editor = control.authorize(&scope, false).unwrap();
    control.shutdown().unwrap();
    assert!(!live.current());
    assert!(!editor.current());
    assert!(!control.active());
    assert!(control.authorize(&page, false).is_err());
    assert!(control.authorize(&scope, false).is_err());
}

#[test]
fn continuity_refuses_hosted_guard_without_replacing_or_retiring_original_player() {
    let mut control = control();
    let page = page(&mut control);
    let scope = start(&mut control, &page, "continuity-source", 1);
    let permit = control.authorize(&scope, true).unwrap();
    let identity = permit.identity.clone();
    control.require_analysis_continuity_supported().unwrap();
    control.guard_configured = true;
    assert!(control.require_analysis_continuity_supported().is_err());
    assert!(control.still_matches(&permit));
    assert_eq!(control.player.as_ref().unwrap().identity, identity.unwrap());
    assert!(control.active());
}
