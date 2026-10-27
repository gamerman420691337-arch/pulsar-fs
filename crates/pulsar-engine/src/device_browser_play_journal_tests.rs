//! Authored format/runtime fixtures only; no physical measurement or acceptance.
use super::*;

#[path = "device_browser_play_journal_native_tests.rs"]
mod native_fixture_tests;

impl Fixture {
    fn browser(mut self) -> Self {
        let source_version = SourceVersionId::new("authored-browser-source").unwrap();
        let selection = BrowserMediaSelection {
            source_version: source_version.clone(),
            source: ArtifactIdentity {
                artifact_id: ArtifactId::new("d".repeat(64)).unwrap(),
                sha256: "d".repeat(64),
                byte_len: 100,
            },
            placement: SourcePlacement {
                placement_id: SourcePlacementId::new("authored-placement").unwrap(),
                created_revision: self.record.original_controller.revision,
                spec: SourcePlacementSpec {
                    source_version,
                    source_start: SourceTimestamp::new(0, 1).unwrap(),
                    source_end: SourceTimestamp::new(3, 1).unwrap(),
                    source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                    project_anchor: ProjectTime::ZERO,
                    rate_numerator: 1,
                    rate_denominator: 1,
                },
            },
        };
        let source_clock = DeclaredSourceClock {
            presentation_anchor: SourceTimestamp::new(0, 1).unwrap(),
            elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
        };
        let source = BrowserMediaBinding {
            preparation_request_id: RequestId::new("authored-source-prepare").unwrap(),
            lease_id: ArtifactId::new("authored-source-lease").unwrap(),
            engine_epoch: AttemptId::new(self.record.original_controller.engine_epoch.as_str())
                .unwrap(),
            project_id: self.record.original_controller.project_id.clone(),
            revision: self.record.original_controller.revision,
            selection: selection.clone(),
            clock: source_clock,
            lease_ms: 60_000,
        };
        let timing_binding = BrowserTimingBinding {
            request_id: RequestId::new("authored-browser-open").unwrap(),
            binding_id: ArtifactId::new("authored-browser-binding").unwrap(),
            engine_epoch: source.engine_epoch.clone(),
            project_id: source.project_id.clone(),
            revision: source.revision,
            player: HandyPlayerIdentity {
                launch_id: AttemptId::new("authored-launch").unwrap(),
                page_id: AttemptId::new("authored-page").unwrap(),
                player_id: AttemptId::new("authored-player").unwrap(),
                source_id: AttemptId::new("authored-selection").unwrap(),
                generation: 1,
                time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
            },
            browser_media: source,
            policy: BrowserTimingPolicy {
                probe_timeout_ms: 50,
                observation_timeout_ms: 100,
            },
        };
        let observation = BrowserTimingObservation {
            observation_id: ArtifactId::new("authored-observation").unwrap(),
            binding_id: timing_binding.binding_id.clone(),
            sequence: 1,
            frame: BrowserTimingFrame {
                sample: HandyPlayerSample {
                    browser_tick_us: 1234,
                    media_time_us: 1_000_000,
                    presented_frames: 1,
                    playing: true,
                    seeking: false,
                    ended: false,
                    visible: true,
                    playback_rate_milli: 1000,
                },
                callback_tick_us: 1234,
                presentation_time_us: Some(1233),
                expected_display_time_us: Some(1235),
            },
            bracket: BrowserClockBracket {
                native_issue_ns: 99_000_000,
                native_receive_ns: 99_100_000,
                browser_sample_us: 1234,
            },
            project_phase: ExactSeconds::new(1, 1).unwrap(),
            expires_at_boottime_ns: 199_000_000,
        };
        let setup = crate::device_effect_journal::setup::row(
            &self.journal.db,
            self.record.setup_record_id.as_str(),
        )
        .unwrap();
        let subject = HandyBrowserStartSubject {
            schema_version: 1,
            origin: HandyEvidenceOrigin::Physical,
            context: HandyQualificationContext {
                schema_version: 1,
                control_image: ArtifactIdentity {
                    artifact_id: ArtifactId::new("1".repeat(64)).unwrap(),
                    sha256: "1".repeat(64),
                    byte_len: 100,
                },
                platform: "authored-journal-fixture-not-physical-evidence".into(),
                transport: ArtifactId::new(HANDY_QUALIFICATION_TRANSPORT).unwrap(),
            },
            device_qualification: self.record.qualification.clone(),
            browser_environment: ArtifactIdentity {
                artifact_id: ArtifactId::new("2".repeat(64)).unwrap(),
                sha256: "2".repeat(64),
                byte_len: 100,
            },
            project_id: timing_binding.project_id.clone(),
            revision: timing_binding.revision,
            selection,
            clock: source_clock,
            master_sha256: setup.record.master_sha256,
            script_actor_session: self.record.actor_session.clone(),
            script_request_id: RequestId::new("authored-script").unwrap(),
            script: setup.record.script,
            timing: HandyBrowserStartTiming {
                clock: BrowserClockAssumptions {
                    timestamp_error_ns: 1,
                    relative_rate_error_ppm: 0,
                    maximum_age_ns: 20_000_000,
                    maximum_interval_ns: 1_000_000,
                },
                reference: HandyBrowserStartReference::Callback,
                display_early_ns: 0,
                display_late_ns: 0,
                source_correspondence_error_ns: 1,
                discrete_media_phase_error_ns: 1,
                media_rate_error_ppm: 0,
                physical_execution_error_ns: 10,
                maximum_start_age_ns: 10_000_000,
                maximum_alignment_error_ns: 1_000_000,
            },
        };
        let mut native = clock();
        let alignment = subject
            .timing
            .phase_request(
                &observation,
                native.monotonic_ns,
                0,
                native.uncertainty_ns,
                105_000_000,
            )
            .unwrap()
            .calculate()
            .unwrap();
        let decision = HandyBrowserStartDecision {
            qualification: HandyBrowserStartQualificationBinding {
                dossier_id: ArtifactId::new("f".repeat(64)).unwrap(),
                revision: 2,
            },
            subject,
            timing_binding,
            observation,
            reference_boottime_ns: native.monotonic_ns,
            alignment,
        };
        decision.validate().unwrap();
        native.uncertainty_ns = decision.alignment.total_error_ns;
        native.valid_until_ns = decision.alignment.valid_until_boottime_ns;
        let admission = self.record.effect.admission.clone();
        let controller = self.record.original_controller.controller_id.clone();
        let qualification = DeviceQualification::new(
            admission.configuration.clone(),
            self.record.qualification.dossier_id.clone(),
            QualificationAuthority::LocalHumanAcceptance,
            Some(7_000_000),
        )
        .unwrap();
        let mut runtime = HandyPlaybackRuntime::new(
            admission.configuration.device.clone(),
            self.record.original_controller.device_session.clone(),
            QualifiedCommandTiming::new(qualification, 10_000_000, 1_000_000, 100).unwrap(),
        )
        .unwrap();
        runtime
            .arm(controller.clone(), admission.clone(), None)
            .unwrap();
        let pending = runtime
            .prepare_referenced_start(
                &controller,
                native.clone(),
                decision.alignment.start_time_ms,
                decision.subject.timing.media_rate_error_ppm,
            )
            .unwrap();
        let effect = runtime
            .dispatch_start(
                pending,
                &admission,
                &controller,
                &native.domain,
                native.monotonic_ns + 2_000_000,
            )
            .unwrap();
        self.record.clock = PlayClockRecord::from(&native);
        self.record.start_time_ms = decision.alignment.start_time_ms;
        self.record.browser_start = Some(decision);
        self.record.effect = self
            .journal
            .receipt_binding()
            .record_for_effect(&effect)
            .unwrap();
        self.record.validate().unwrap();
        self.runtime = runtime;
        self.effect = effect;
        self
    }
}

#[test]
fn browser_reference_original_decision_survives_claim_completion_and_restart_as_history_only() {
    let mut f = Fixture::new().browser();
    let original = serde_json::to_vec(&f.record).unwrap();
    let prepared = f.prepare();
    let id = f.history().id;
    let status = f.history().status().unwrap();
    assert_eq!(
        status.history.as_ref().unwrap().browser_start,
        f.record.browser_start
    );
    let claim = f.journal.claim_play(prepared).unwrap();
    f.journal
        .complete_play_operation(&claim, DispatchOutcome::Acknowledged)
        .unwrap();
    f.history().check_witness(&status).unwrap();
    let reopened = DeviceEffectJournal::open(
        f.dir.path(),
        DeviceSessionId::new("new-browser-boot").unwrap(),
    )
    .unwrap();
    let history = reopened
        .play_request_history(&f.record.actor_session, &f.record.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(history.id, id);
    assert_eq!(serde_json::to_vec(&history.record).unwrap(), original);
    assert_eq!(history.state, PlayOperationState::Acknowledged);
    let retired_intent = f.intent();
    assert!(f.journal.prepare_play(&retired_intent).is_err());
}

#[test]
fn referenced_effect_requires_original_browser_record_and_exact_combined_clock() {
    let mut f = Fixture::new().browser();
    assert!(f.journal.prepare(&f.runtime, &f.effect).is_err());
    assert_eq!(
        f.journal
            .db
            .query_row("SELECT count(*) FROM effects", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        0
    );
    let mut no_decision = f.record.clone();
    no_decision.browser_start = None;
    assert!(f
        .journal
        .receipt_binding()
        .capture_play_intent(&f.runtime, &f.effect, no_decision)
        .is_err());
    for mutation in 0..8 {
        let mut changed = f.record.clone();
        match mutation {
            0 => changed.start_time_ms += 1,
            1 => changed.clock.estimated_server_ms += 1,
            2 => changed.clock.monotonic_ns += 1,
            3 => changed.clock.uncertainty_ns += 1,
            4 => {
                changed.clock.valid_until_ns = changed
                    .browser_start
                    .as_ref()
                    .unwrap()
                    .alignment
                    .valid_until_boottime_ns
                    + 1
            }
            5 => {
                changed.browser_start.as_mut().unwrap().subject.origin =
                    HandyEvidenceOrigin::Synthetic
            }
            6 => {
                changed
                    .browser_start
                    .as_mut()
                    .unwrap()
                    .timing_binding
                    .engine_epoch = AttemptId::new("foreign-boot").unwrap()
            }
            _ => {
                changed
                    .browser_start
                    .as_mut()
                    .unwrap()
                    .subject
                    .script_actor_session = SessionId::new("foreign-script-actor").unwrap()
            }
        }
        assert!(changed.validate().is_err(), "mutation {mutation}");
    }
    f.prepare();
}

#[test]
fn browser_reference_parent_script_mismatch_cannot_publish_effect_or_membership() {
    let mut f = Fixture::new().browser();
    f.record
        .browser_start
        .as_mut()
        .unwrap()
        .subject
        .master_sha256 = "3".repeat(64);
    let intent = f.intent();
    assert!(f.journal.prepare_play(&intent).is_err());
    for table in ["effects", "play_operations", "play_reference_members"] {
        assert_eq!(
            f.journal
                .db
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn browser_reference_membership_failure_rolls_back_original_play_and_effect_together() {
    let mut f = Fixture::new().browser();
    let intent = f.intent();
    f.journal.db.execute_batch("CREATE TRIGGER reject_reference_insert BEFORE INSERT ON play_reference_members BEGIN SELECT RAISE(ABORT,'authored membership fault'); END").unwrap();
    assert!(f.journal.prepare_play(&intent).is_err());
    for table in ["effects", "play_operations", "play_reference_members"] {
        assert_eq!(
            f.journal
                .db
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        f.journal
            .db
            .query_row(
                "SELECT operation_count FROM play_reference_census",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        0
    );
    f.journal
        .db
        .execute_batch("DROP TRIGGER reject_reference_insert")
        .unwrap();
    let prepared = f.journal.prepare_play(&intent).unwrap();
    assert_eq!(f.history().record.browser_start, f.record.browser_start);
    assert_eq!(
        f.journal
            .db
            .query_row(
                "SELECT operation_count FROM play_reference_census",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        1
    );
    f.journal.claim_play(prepared).unwrap();
}

#[test]
fn version_nine_missing_or_corrupt_reference_membership_refuses_without_changing_bytes() {
    for mutation in [
        "DELETE FROM play_reference_members",
        "UPDATE play_reference_members SET decision_sha256=NULL",
        "UPDATE play_reference_members SET membership_sha256='bad'",
        "UPDATE play_reference_members SET operation_id=CASE substr(operation_id,1,1) WHEN '0' THEN '1'||substr(operation_id,2) ELSE '0'||substr(operation_id,2) END",
        "DELETE FROM play_reference_census",
        "UPDATE play_reference_census SET operation_count=0",
        "DELETE FROM play_operations; DELETE FROM play_reference_members",
        "DROP TABLE play_reference_members",
    ] {
        let mut f = Fixture::new().browser(); f.prepare();
        f.journal.db.execute_batch(mutation).unwrap();
        let before = f.bytes();
        assert!(DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("rejected-owner").unwrap()).is_err(), "{mutation}");
        assert_eq!(f.bytes(), before, "{mutation}");
    }
}

#[test]
fn version_eight_manual_record_upgrade_preserves_exact_bytes_and_none_field_hash() {
    let mut f = Fixture::new();
    f.prepare();
    let id = f.history().id;
    let original: String = f
        .journal
        .db
        .query_row(
            "SELECT record FROM play_operations WHERE id=?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!original.contains("browser_start"));
    f.journal.db.execute_batch("DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; PRAGMA user_version=8;").unwrap();
    let reopened =
        DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("version-ten").unwrap())
            .unwrap();
    let history = reopened
        .play_request_history(&f.record.actor_session, &f.record.request_id)
        .unwrap()
        .unwrap();
    assert_eq!(history.id, id);
    assert_eq!(serde_json::to_string(&history.record).unwrap(), original);
    assert!(history.record.browser_start.is_none());
    assert_eq!(
        reopened
            .db
            .query_row(
                "SELECT decision_sha256 FROM play_reference_members WHERE operation_id=?1",
                [id],
                |r| r.get::<_, Option<String>>(0)
            )
            .unwrap(),
        None
    );
    assert_eq!(
        reopened
            .db
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        12
    );
}

#[test]
fn forged_pre_v9_browser_record_and_newer_format_are_rejected_before_migration() {
    for mutation in [
        "DROP TABLE hsp_preparation_resolutions; DROP TABLE hsp_preparation_links; DROP TABLE hsp_preparations; DROP TABLE hsp_preparation_census; DROP TABLE hsp_resolution_members; DROP TABLE hsp_emergency_observations; DROP TABLE hsp_qualification_links; DROP TABLE hsp_recovery_census; DROP TABLE hsp_effects; DROP TABLE hsp_streams; DROP TABLE hsp_census; DROP TABLE play_reference_members; DROP TABLE play_reference_census; PRAGMA user_version=8;",
        "PRAGMA user_version=13;",
    ] {
        let mut f = Fixture::new().browser(); f.prepare();
        f.journal.db.execute_batch(mutation).unwrap();
        let before = f.bytes();
        assert!(DeviceEffectJournal::open(f.dir.path(), DeviceSessionId::new("rejected-reader").unwrap()).is_err());
        assert_eq!(f.bytes(), before);
    }
}
