use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_player_guard_status_keeps_the_original_binding_and_project() {
        assert!(matches!(Cli::try_parse_from([
            "pulsar", "handy-player-guard-status", "project", "--binding-request-id", "original-bind"
        ]).unwrap().command, Some(Commands::HandyPlayerGuardStatus {project,binding_request_id})
            if project == "project" && binding_request_id == "original-bind"));
        for args in [
            vec!["pulsar", "handy-player-guard-status", "project"],
            vec![
                "pulsar",
                "handy-player-guard-status",
                "--binding-request-id",
                "original-bind",
            ],
            vec![
                "pulsar",
                "handy-player-guard-status",
                "project",
                "--binding-request-id",
                "original-bind",
                "--revision",
                "1",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }
    #[test]
    fn browser_media_lease_cap_is_optional_bounded_and_independent_of_server_lifetime() {
        let base = ["pulsar", "browser-live", "project", "7", "options.json"];
        assert!(matches!(
            Cli::try_parse_from(base).unwrap().command,
            Some(Commands::BrowserLive {
                media_lease_ms: None,
                duration_seconds: 300,
                ..
            })
        ));
        for cap in ["1", "30000", "600000"] {
            let mut args = base.to_vec();
            args.extend(["--duration-seconds", "180", "--media-lease-ms", cap]);
            assert!(matches!(Cli::try_parse_from(args).unwrap().command,
                Some(Commands::BrowserLive { media_lease_ms: Some(value), duration_seconds: 180, .. }) if value == cap.parse::<u32>().unwrap()));
        }
        for cap in ["0", "600001", "4294967296", "-1", "not-a-duration"] {
            let mut args = base.to_vec();
            args.extend(["--media-lease-ms", cap]);
            assert!(Cli::try_parse_from(args).is_err(), "{cap}");
        }
    }
    #[test]
    fn browser_player_monitoring_requires_both_explicit_consent_and_selection() {
        let base = ["pulsar", "browser-live", "project", "7", "options.json"];
        let plain = Cli::try_parse_from(base).unwrap();
        assert!(matches!(
            plain.command,
            Some(Commands::BrowserLive {
                handy_player_guard: None,
                allow_handy_player_monitoring: false,
                ..
            })
        ));
        let mut consent_only = base.to_vec();
        consent_only.push("--allow-handy-player-monitoring");
        assert!(Cli::try_parse_from(consent_only).is_err());
        let mut selection_only = base.to_vec();
        selection_only.extend(["--handy-player-guard", "guard.json"]);
        assert!(Cli::try_parse_from(selection_only.clone()).is_err());
        selection_only.push("--allow-handy-player-monitoring");
        assert!(
            matches!(Cli::try_parse_from(selection_only).unwrap().command,
            Some(Commands::BrowserLive { handy_player_guard: Some(path),
                allow_handy_player_monitoring: true, .. }) if path.as_path() == std::path::Path::new("guard.json"))
        );
    }
    #[test]
    fn handy_refresh_timing_requires_consent_and_each_exact_play_selection() {
        let args = [
            "pulsar",
            "handy-refresh-timing",
            "project",
            "--revision",
            "7",
            "--acquisition-request-id",
            "acquire",
            "--controller-id",
            "controller",
            "--generation",
            "3",
            "--play-request-id",
            "play",
            "--play-record-id",
            "record",
            "--request-id",
            "refresh",
            "--allow-online-inspection",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::HandyRefreshTiming {project,revision:7,acquisition_request_id,controller_id,generation:3,play_request_id,play_record_id,request_id,allow_online_inspection:true})
            if project=="project" && acquisition_request_id=="acquire" && controller_id=="controller"
                && play_request_id=="play" && play_record_id=="record" && request_id=="refresh")
        );
        for option in [
            "--revision",
            "--acquisition-request-id",
            "--controller-id",
            "--generation",
            "--play-request-id",
            "--play-record-id",
            "--request-id",
            "--allow-online-inspection",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.remove(index);
            if option != "--allow-online-inspection" {
                missing.remove(index);
            }
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
        let mut missing_project = args.to_vec();
        missing_project.remove(2);
        assert!(Cli::try_parse_from(missing_project).is_err());
    }

    #[test]
    fn handy_refresh_timing_status_requires_original_project_without_revision() {
        let args = [
            "pulsar",
            "handy-refresh-timing-status",
            "project",
            "--refresh-request-id",
            "refresh",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::HandyRefreshTimingStatus {project,refresh_request_id})
            if project=="project" && refresh_request_id=="refresh")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-refresh-timing-status", "project"]).is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-refresh-timing-status",
            "--refresh-request-id",
            "refresh"
        ])
        .is_err());
        let mut with_revision = args.to_vec();
        with_revision.extend(["--revision", "7"]);
        assert!(Cli::try_parse_from(with_revision).is_err());
    }

    #[test]
    fn handy_reconcile_plan_requires_every_captured_selection() {
        let args = [
            "pulsar",
            "handy-reconcile-plan",
            "project",
            "--device-id",
            "device",
            "--registration-revision",
            "1",
            "--engine-epoch",
            "boot",
            "--device-session",
            "session",
            "--qualification-id",
            "qualification",
            "--qualification-revision",
            "2",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyReconcilePlan { project,device_id,registration_revision:1,engine_epoch,device_session,qualification_id,qualification_revision:2 })
            if project=="project" && device_id=="device" && engine_epoch=="boot" && device_session=="session" && qualification_id=="qualification")
        );
        for option in [
            "--device-id",
            "--registration-revision",
            "--engine-epoch",
            "--device-session",
            "--qualification-id",
            "--qualification-revision",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.drain(index..index + 2);
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
    }

    #[test]
    fn handy_reconcile_requires_exact_plan_request_and_both_consents() {
        let args = [
            "pulsar",
            "handy-reconcile",
            "project",
            "--plan-id",
            "plan",
            "--request-id",
            "reconcile",
            "--allow-online-inspection",
            "--allow-resolution",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyReconcile { project,plan_id,request_id,allow_online_inspection:true,allow_resolution:true })
            if project=="project" && plan_id=="plan" && request_id=="reconcile")
        );
        for option in [
            "--plan-id",
            "--request-id",
            "--allow-online-inspection",
            "--allow-resolution",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.remove(index);
            if !option.starts_with("--allow-") {
                missing.remove(index);
            }
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
    }

    #[test]
    fn handy_reconcile_status_and_cancel_require_original_request_identity() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "handy-reconcile-status",
            "project",
            "--reconciliation-request-id",
            "original",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyReconcileStatus { project,reconciliation_request_id })
            if project=="project" && reconciliation_request_id=="original")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-reconcile-status", "project"]).is_err());
        let args = [
            "pulsar",
            "handy-reconcile-cancel",
            "project",
            "--reconciliation-request-id",
            "original",
            "--request-id",
            "cancel",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyReconcileCancel { project,reconciliation_request_id,request_id })
            if project=="project" && reconciliation_request_id=="original" && request_id=="cancel")
        );
        for option in ["--reconciliation-request-id", "--request-id"] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.drain(index..index + 2);
            assert!(Cli::try_parse_from(missing).is_err());
        }
    }

    #[test]
    fn handy_hsp_preparation_cli_requires_explicit_consent_and_original_selection() {
        let args = [
            "pulsar",
            "handy-hsp-prepare",
            "project",
            "--revision",
            "2",
            "--engine-epoch",
            "boot",
            "--device-session",
            "session",
            "--qualification-id",
            "dossier",
            "--qualification-revision",
            "2",
            "--timeout-ms",
            "1000",
            "--request-id",
            "prepare",
            "--allow-online-synchronization",
        ];
        assert!(matches!(Cli::try_parse_from(args).unwrap().command,
            Some(Commands::HandyHspPrepare {project,timeout_ms:1000,allow_online_synchronization:true,..}) if project=="project"));
        for option in [
            "--revision",
            "--engine-epoch",
            "--device-session",
            "--qualification-id",
            "--qualification-revision",
            "--timeout-ms",
            "--request-id",
            "--allow-online-synchronization",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|v| *v == option).unwrap();
            let length = if option == "--allow-online-synchronization" {
                1
            } else {
                2
            };
            missing.drain(index..index + length);
            assert!(Cli::try_parse_from(missing).is_err(), "{option}");
        }
        for timeout in ["0", "60001"] {
            let mut invalid = args.to_vec();
            let index = invalid.iter().position(|v| *v == "--timeout-ms").unwrap();
            invalid[index + 1] = timeout;
            assert!(Cli::try_parse_from(invalid).is_err());
        }
        assert!(
            matches!(Cli::try_parse_from(["pulsar","handy-hsp-preparation-status","project","--preparation-request-id","prepare"]).unwrap().command,
            Some(Commands::HandyHspPreparationStatus {project,preparation_request_id}) if project=="project" && preparation_request_id=="prepare")
        );
        assert!(
            matches!(Cli::try_parse_from(["pulsar","handy-hsp-preparation-cancel","project","--preparation-request-id","prepare","--request-id","cancel"]).unwrap().command,
            Some(Commands::HandyHspPreparationCancel {project,preparation_request_id,request_id}) if project=="project" && preparation_request_id=="prepare" && request_id=="cancel")
        );
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-preparation-cancel",
            "project",
            "--preparation-request-id",
            "prepare"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-preparation-status",
            "project",
            "--preparation-request-id",
            "prepare",
            "--timeout-ms",
            "1000"
        ])
        .is_err());
    }
    #[test]
    fn handy_hsp_status_requires_project_and_original_stream_without_revision() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "handy-hsp-status",
            "project",
            "--stream-request-id",
            "acquire",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyHspStatus { project, stream_request_id })
            if project == "project" && stream_request_id == "acquire")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-hsp-status", "project"]).is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-status",
            "--stream-request-id",
            "acquire"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-status",
            "project",
            "--stream-request-id",
            "acquire",
            "--revision",
            "1"
        ])
        .is_err());
    }

    #[test]
    fn handy_stop_status_requires_project_and_original_acquisition_without_revision() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "handy-stop-status",
            "project",
            "--acquisition-request-id",
            "acquire",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyStopStatus { project, acquisition_request_id })
            if project == "project" && acquisition_request_id == "acquire")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-stop-status", "project"]).is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-stop-status",
            "--acquisition-request-id",
            "acquire"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-stop-status",
            "project",
            "--acquisition-request-id",
            "acquire",
            "--revision",
            "1"
        ])
        .is_err());
    }

    #[test]
    fn handy_play_requires_consent_and_every_captured_selection() {
        let args = [
            "pulsar",
            "handy-play",
            "project",
            "--revision",
            "7",
            "--acquisition-request-id",
            "acquire",
            "--controller-id",
            "controller",
            "--generation",
            "3",
            "--clock-request-id",
            "clock",
            "--start-time-ms",
            "125",
            "--request-id",
            "play",
            "--allow-play",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(matches!(parsed.command, Some(Commands::HandyPlay {
            project, revision: 7, acquisition_request_id, controller_id, generation: 3,
            clock_request_id, start_time_ms: 125, request_id, allow_play: true,
        }) if project == "project" && acquisition_request_id == "acquire"
            && controller_id == "controller" && clock_request_id == "clock" && request_id == "play"));
        for option in [
            "--revision",
            "--acquisition-request-id",
            "--controller-id",
            "--generation",
            "--clock-request-id",
            "--start-time-ms",
            "--request-id",
            "--allow-play",
        ] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.remove(index);
            if option != "--allow-play" {
                missing.remove(index);
            }
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
    }

    #[test]
    fn handy_play_status_requires_the_original_play_request() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "handy-play-status",
            "project",
            "--play-request-id",
            "play",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyPlayStatus { project, play_request_id }) if project == "project" && play_request_id == "play")
        );
        assert!(Cli::try_parse_from(["pulsar", "handy-play-status", "project"]).is_err());
        assert!(
            Cli::try_parse_from(["pulsar", "handy-play-status", "--play-request-id", "play"])
                .is_err()
        );
    }

    #[test]
    fn handy_play_cancel_requires_exact_record_and_separate_request_selection() {
        let args = [
            "pulsar",
            "handy-play-cancel",
            "project",
            "--play-request-id",
            "play",
            "--record-id",
            "record",
            "--request-id",
            "cancel",
        ];
        let parsed = Cli::try_parse_from(args).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::HandyPlayCancel { project, play_request_id, record_id, request_id }) if project == "project" && play_request_id == "play" && record_id == "record" && request_id == "cancel")
        );
        for option in ["--play-request-id", "--record-id", "--request-id"] {
            let mut missing = args.to_vec();
            let index = missing.iter().position(|arg| *arg == option).unwrap();
            missing.drain(index..index + 2);
            assert!(Cli::try_parse_from(missing).is_err(), "missing {option}");
        }
    }

    #[test]
    fn retained_scheduler_requires_state_and_keeps_explicit_step_and_poll_controls() {
        assert!(Cli::try_parse_from(["pulsar", "library-batch-run", "batch"]).is_err());
        let parsed = Cli::try_parse_from([
            "pulsar",
            "library-batch-run",
            "batch",
            "--publication-state",
            "/state",
            "--steps",
            "2",
            "--poll-ms",
            "1",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::LibraryBatchRun {
                steps: 2,
                poll_ms: 1,
                ..
            })
        ));
        assert!(Cli::try_parse_from([
            "pulsar",
            "library-batch-workflow-status",
            "batch",
            "--publication-state",
            "/state"
        ])
        .is_ok());
    }

    #[test]
    fn library_publication_commands_require_named_private_state_and_preserve_step_count() {
        assert!(Cli::try_parse_from(["pulsar", "library-batch-publish", "batch"]).is_err());
        let parsed = Cli::try_parse_from([
            "pulsar",
            "library-batch-publication-prepare",
            "batch",
            "--publication-state",
            "/state",
            "--steps",
            "2",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::LibraryBatchPublicationPrepare { steps: 2, .. })
        ));
        assert!(Cli::try_parse_from([
            "pulsar",
            "library-batch-publication-status",
            "--publication-state",
            "/state"
        ])
        .is_ok());
    }

    #[test]
    fn library_cancel_requires_explicit_replay_identity() {
        assert!(Cli::try_parse_from(["pulsar", "library-batch-cancel", "batch"]).is_err());
        let parsed = Cli::try_parse_from([
            "pulsar",
            "library-batch-cancel",
            "batch",
            "--request-id",
            "cancel-id",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::LibraryBatchCancel {batch,request_id}) if batch=="batch" && request_id=="cancel-id")
        );
    }

    #[test]
    fn library_release_requires_explicit_replay_identity() {
        assert!(Cli::try_parse_from(["pulsar", "library-batch-release", "batch"]).is_err());
        let parsed = Cli::try_parse_from([
            "pulsar",
            "library-batch-release",
            "batch",
            "--request-id",
            "release-id",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::LibraryBatchRelease {batch,request_id}) if batch=="batch" && request_id=="release-id")
        );
    }

    #[test]
    fn vision_setup_and_image_prepare_require_their_explicit_scope_controls() {
        let setup = [
            "pulsar",
            "assistant-setup",
            "--output",
            "/policy.json",
            "--runtime",
            "/runtime",
            "--model",
            "/model",
            "--projector",
            "/projector",
        ];
        assert!(Cli::try_parse_from(setup).is_err());
        let mut approved = setup.to_vec();
        approved.push("--approve-host-inference");
        let parsed = Cli::try_parse_from(approved).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::AssistantSetup { projector: Some(path), .. }) if path == std::path::Path::new("/projector"))
        );
        let prepare = [
            "pulsar",
            "assistant",
            "prepare",
            "project",
            "--profile",
            "profile",
            "--revision",
            "0",
            "--prompt",
            "Suggest a theme",
            "--request-id",
            "intent",
            "--output",
            "/intent.json",
            "--image-source",
            "still",
        ];
        assert!(Cli::try_parse_from(prepare).is_err());
        let mut creative = prepare.to_vec();
        creative.extend(["--creative-scope", "/scope.json"]);
        let parsed = Cli::try_parse_from(creative).unwrap();
        assert!(
            matches!(parsed.command, Some(Commands::Assistant { command: AssistantCommands::Prepare { image_source: Some(source), .. } }) if source == "still")
        );
    }

    #[test]
    fn creative_cli_requires_separate_approval_and_exclusive_scope() {
        let base = [
            "pulsar",
            "assistant",
            "creative-candidate",
            "project",
            "attempt",
            "--revision",
            "0",
            "--request-id",
            "generation",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut args = base.to_vec();
        args.push("--approve-generation");
        assert!(Cli::try_parse_from(args).is_ok());
        let base = [
            "pulsar",
            "assistant",
            "prepare",
            "project",
            "--profile",
            "profile",
            "--revision",
            "0",
            "--prompt",
            "hold",
            "--request-id",
            "start",
            "--output",
            "/intent.json",
            "--creative-scope",
            "/scope.json",
        ];
        assert!(Cli::try_parse_from(base).is_ok());
        let mut args = base.to_vec();
        args.extend(["--review-id", "flag"]);
        assert!(Cli::try_parse_from(args).is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "assistant",
            "prepare-creative-generation",
            "project",
            "attempt",
            "--revision",
            "0",
            "--request-id",
            "generate",
            "--output",
            "/generation.json"
        ])
        .is_ok());
    }
    #[test]
    fn explicit_pattern_candidate_requires_project_file_and_revision() {
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "pattern-candidate",
                "project",
                "/patterns.txt",
                "--revision",
                "7"
            ])
            .unwrap()
            .command,
            Some(Commands::PatternCandidate { revision: 7, .. })
        ));
        assert!(
            Cli::try_parse_from(["pulsar", "pattern-candidate", "project", "/patterns.txt"])
                .is_err()
        );
    }
    #[test]
    fn inspect_candidate_requires_both_ids_and_uses_the_engine() {
        let args = ["pulsar", "inspect-candidate", "project", "candidate"];
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Some(Commands::InspectCandidate { project, candidate })
                if project == "project" && candidate == "candidate"
        ));
        assert!(Cli::try_parse_from(&args[..3]).is_err());
        assert!(crate::command_needs_engine(
            &args.map(std::ffi::OsString::from)
        ));
    }

    #[test]
    fn history_commands_require_revision_and_accept_explicit_request_identity() {
        for name in ["undo", "redo"] {
            assert!(Cli::try_parse_from(["pulsar", name, "project"]).is_err());
            assert!(Cli::try_parse_from([
                "pulsar",
                name,
                "project",
                "--revision",
                "7",
                "--request-id",
                "history-request",
            ])
            .is_ok());
        }
        assert!(
            Cli::try_parse_from(["pulsar", "rebase-candidate", "project", "candidate"]).is_err()
        );
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar", "rebase-candidate", "project", "candidate", "--revision", "7",
                "--request-id", "rebase-request",
            ])
            .unwrap()
            .command,
            Some(Commands::RebaseCandidate { revision: 7, request_id: Some(id), .. }) if id == "rebase-request"
        ));
    }
    #[test]
    fn saved_assistant_generation_cli_requires_separate_approval_and_complete_prepare_options() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "assistant",
            "generate-saved",
            "--intent",
            "/private/request.json"
        ])
        .is_err());
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "assistant",
                "generate-saved",
                "--intent",
                "/private/request.json",
                "--approve-generation"
            ])
            .unwrap()
            .command,
            Some(Commands::Assistant {
                command: AssistantCommands::GenerateSaved {
                    approve_generation: true,
                    ..
                }
            })
        ));
        let args = [
            "pulsar",
            "assistant",
            "prepare-generation",
            "project",
            "attempt",
            "--source",
            "source",
            "--revision",
            "7",
            "--request-id",
            "original",
            "--output",
            "/private/request.json",
        ];
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Some(Commands::Assistant {
                command: AssistantCommands::PrepareGeneration { revision: 7, .. }
            })
        ));
        assert!(Cli::try_parse_from(&args[..args.len() - 2]).is_err());
    }
    #[test]
    fn assistant_review_cli_requires_revision_and_accepts_repeated_focus() {
        assert!(
            Cli::try_parse_from(["pulsar", "assistant", "explain", "project", "attempt"]).is_err()
        );
        let parsed = Cli::try_parse_from([
            "pulsar",
            "assistant",
            "prepare",
            "project",
            "--profile",
            "profile",
            "--revision",
            "7",
            "--prompt",
            "review",
            "--request-id",
            "request",
            "--output",
            "/private/intent",
            "--review-id",
            "first",
            "--review-id",
            "second",
            "--edit-id",
            "edit-first",
            "--target-id",
            "observed-first",
            "--target-id",
            "observed-second",
        ])
        .unwrap();
        let Some(Commands::Assistant {
            command:
                AssistantCommands::Prepare {
                    image_source: None,
                    review_focus,
                    target_focus,
                    edit_focus,
                    ..
                },
        }) = parsed.command
        else {
            panic!("prepare")
        };
        assert_eq!(review_focus, vec!["first", "second"]);
        assert_eq!(target_focus, vec!["observed-first", "observed-second"]);
        assert_eq!(edit_focus, vec!["edit-first"]);

        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "assistant",
                "explain",
                "project",
                "attempt",
                "--revision",
                "7"
            ])
            .unwrap()
            .command,
            Some(Commands::Assistant {
                command: AssistantCommands::Explain { revision: 7, .. }
            })
        ));
    }
    #[test]
    fn assistant_generation_cli_requires_explicit_approval_source_revision_and_identity() {
        let args = [
            "pulsar",
            "assistant",
            "generate",
            "project",
            "attempt",
            "--source",
            "source",
            "--revision",
            "0",
            "--request-id",
            "original",
        ];
        assert!(Cli::try_parse_from(args).is_err());
        let mut approved = args.to_vec();
        approved.push("--approve-generation");
        let parsed = Cli::try_parse_from(approved).unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::Assistant {
                command: AssistantCommands::Generate {
                    approve_generation: true,
                    ..
                }
            })
        ));
    }
    #[test]
    fn pack_withdrawal_cli_keeps_both_revision_fences_and_explicit_legacy_scope() {
        let digest = "a".repeat(64);
        let parsed = Cli::try_parse_from([
            "pulsar",
            "model",
            "pack-withdraw",
            &digest,
            "--selection-revision",
            "1",
            "--expected-selection-revision",
            "3",
            "--include-unclassified-generation",
            "--request-id",
            "original-withdrawal",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::Model {command:ModelCommands::PackWithdraw {selection_revision:1,expected_selection_revision:3,include_unclassified_generation:true,request_id:Some(ref id),..}}) if id=="original-withdrawal")
        );
        assert!(Cli::try_parse_from(["pulsar", "model", "pack-withdraw", &digest]).is_err());
    }

    #[test]
    fn original_live_cuda_download_cli_requires_an_explicit_known_part_and_destination() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "download-evidence",
            "project",
            "0",
            "capture",
            "trace",
            "out.json"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "download-evidence",
            "project",
            "0",
            "capture",
            "unknown",
            "out.json"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "download-evidence",
            "project",
            "0",
            "capture",
            "trace"
        ])
        .is_err());
    }
    #[test]
    fn retention_removal_cli_requires_confirmation_and_effect_identity() {
        let hash = "0".repeat(64);
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "remove-evidence",
            "project",
            "0",
            "capture",
            &hash,
            "--request-id",
            "removal",
            "--confirm-removal"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "remove-evidence",
            "project",
            "0",
            "capture",
            &hash,
            "--request-id",
            "removal"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "remove-evidence",
            "project",
            "0",
            "capture",
            &hash,
            "--confirm-removal"
        ])
        .is_err());
    }
    #[test]
    fn retained_cuda_listing_requires_revision_and_bounded_page() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "list-evidence",
            "project",
            "7",
            "--limit",
            "2",
            "--after",
            "capture"
        ])
        .is_ok());
        for limit in ["0", "17", "65535"] {
            assert!(Cli::try_parse_from([
                "pulsar",
                "live-capture",
                "list-evidence",
                "project",
                "7",
                "--limit",
                limit
            ])
            .is_err());
        }
        assert!(
            Cli::try_parse_from(["pulsar", "live-capture", "list-evidence", "project"]).is_err()
        );
    }
    #[test]
    fn retained_live_cuda_evidence_cli_is_explicitly_project_revision_epoch_scoped() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "evidence",
            "project",
            "7",
            "capture",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::LiveCapture {
                command: LiveCaptureCommands::Evidence { revision: 7, .. }
            })
        ));
        assert!(
            Cli::try_parse_from(["pulsar", "live-capture", "evidence", "project", "capture"])
                .is_err()
        );
    }
    #[test]
    fn stash_multifile_capture_requires_explicit_path_file() {
        let base = [
            "pulsar",
            "stash",
            "capture-source",
            "project",
            "scene",
            "file",
            "--mappings",
            "mapping.json",
        ];
        assert!(Cli::try_parse_from(base).is_ok());
        let without_path = [
            "pulsar",
            "stash",
            "capture-source",
            "project",
            "scene",
            "file",
            "--mappings",
            "mapping.json",
            "--allow-multifile-selection",
        ];
        assert!(Cli::try_parse_from(without_path).is_err());
        let explicit = [
            "pulsar",
            "stash",
            "capture-source",
            "project",
            "scene",
            "file",
            "--mappings",
            "mapping.json",
            "--allow-multifile-selection",
            "--expected-server-path-file",
            "path.json",
        ];
        assert!(matches!(
            Cli::try_parse_from(explicit).unwrap().command,
            Some(Commands::Stash {
                command: StashCommands::CaptureSource {
                    allow_multifile_selection: true,
                    expected_server_path_file: Some(_),
                    ..
                }
            })
        ));
    }

    #[test]
    fn legacy_aliases_remain_parseable() {
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "audit", "/media"])
                .unwrap()
                .command,
            Some(Commands::Scan { .. })
        ));
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "stash", "query"])
                .unwrap()
                .command,
            Some(Commands::Stash {
                command: StashCommands::ListMissing { .. }
            })
        ));
    }
    #[test]
    fn stash_environment_name_and_device_profile_grammar_are_explicit() {
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "stash", "query", "--api-key-env", "STASH_API_KEY"])
                .unwrap()
                .command,
            Some(Commands::Stash {
                command: StashCommands::ListMissing {
                    api_key_env: Some(_),
                    ..
                }
            })
        ));
        assert!(Cli::try_parse_from([
            "pulsar",
            "export-device-motion",
            "project-id",
            "proposal.json",
            "--profile",
            "dynamics.json"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "pulsar",
            "export-device-motion",
            "project-id",
            "proposal.json"
        ])
        .is_err());
    }

    #[test]
    fn capture_old_boot_replay_requires_explicit_request_identity() {
        for mut args in [
            vec![
                "pulsar",
                "live-capture",
                "start",
                "project",
                "0",
                "options.json",
            ],
            vec![
                "pulsar",
                "live-capture",
                "continue",
                "project",
                "0",
                "prior",
                "options.json",
            ],
            vec![
                "pulsar",
                "live-capture",
                "continue-at",
                "project",
                "0",
                "prior",
                "anchor.json",
            ],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok());
            args.extend(["--engine-epoch", "original-boot"]);
            assert!(Cli::try_parse_from(args.clone()).is_err());
            args.extend(["--request-id", "original-request"]);
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }
    #[test]
    fn live_finish_grammar_is_explicit_and_keeps_request_identity() {
        let cli = Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "finish",
            "project",
            "3",
            "epoch",
            "--request-id",
            "finish-id",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::LiveCapture {
                command: LiveCaptureCommands::Finish {
                    revision: 3,
                    request_id: Some(_),
                    ..
                }
            })
        ));
        assert!(
            Cli::try_parse_from(["pulsar", "live-capture", "finish", "project", "epoch"]).is_err()
        );
    }
    #[test]
    fn recording_stage_keeps_explicit_options_revision_and_request() {
        let cli = Cli::try_parse_from([
            "pulsar",
            "live-capture",
            "recording-stage",
            "project",
            "3",
            "/absolute/options.json",
            "--request-id",
            "stage-id",
        ])
        .unwrap();
        assert!(
            matches!(cli.command,Some(Commands::LiveCapture {command:LiveCaptureCommands::RecordingStage {revision:3,request_id:Some(id),..}}) if id=="stage-id")
        );
    }
    #[test]
    fn gap_recipe_requires_explicit_authored_policy_and_revision() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "gap-candidate",
            "project",
            "/absolute/gaps.json",
            "--revision",
            "1"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "gap-candidate",
            "project",
            "/absolute/gaps.json",
            "--revision",
            "1",
            "--linear-edge-hold"
        ])
        .is_ok());
    }
    #[test]
    fn generation_dimensions_distinguish_omitted_and_explicit_cli_requests() {
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "generate", "video.mp4"])
                .unwrap()
                .command,
            Some(Commands::Generate {
                width: None,
                height: None,
                ..
            })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "stash",
                "generate-missing",
                "--mappings",
                "roots.json"
            ])
            .unwrap()
            .command,
            Some(Commands::Stash {
                command: StashCommands::GenerateMissing {
                    width: None,
                    height: None,
                    ..
                }
            })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "stash",
                "generate-missing",
                "--mappings",
                "roots.json",
                "--width",
                "320"
            ])
            .unwrap()
            .command,
            Some(Commands::Stash {
                command: StashCommands::GenerateMissing {
                    width: Some(320),
                    height: None,
                    ..
                }
            })
        ));
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "generate", "video.mp4", "--height", "180"])
                .unwrap()
                .command,
            Some(Commands::Generate {
                width: None,
                height: Some(180),
                ..
            })
        ));
    }

    #[test]
    fn detector_score_options_are_checked_and_omission_keeps_legacy_default() {
        let base = [
            "pulsar",
            "generate",
            "video.mp4",
            "--model",
            "model.onnx",
            "--model-class-count",
            "1",
        ];
        assert!(matches!(
            Cli::try_parse_from(base).unwrap().command,
            Some(Commands::Generate { conf: None, .. })
        ));
        for value in ["0", "0.35", "1"] {
            let mut args = base.to_vec();
            args.extend(["--conf", value]);
            assert!(matches!(
                Cli::try_parse_from(args).unwrap().command,
                Some(Commands::Generate { conf: Some(_), .. })
            ));
        }
        for value in ["NaN", "inf", "1.01", "-0.1"] {
            let mut args = base.to_vec();
            args.extend(["--conf", value]);
            assert!(Cli::try_parse_from(args).is_err());
            assert!(Cli::try_parse_from([
                "pulsar",
                "stash",
                "generate-missing",
                "--mappings",
                "roots.json",
                "--detector-score-threshold",
                value
            ])
            .is_err());
        }
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "stash",
                "generate-missing",
                "--mappings",
                "roots.json",
                "--model-pack",
                "pack",
                "--detector-score-threshold",
                "0.75"
            ])
            .unwrap()
            .command,
            Some(Commands::Stash {
                command: StashCommands::GenerateMissing {
                    detector_score_threshold: Some(0.75),
                    ..
                }
            })
        ));
    }
    #[test]
    fn visual_tier_cli_preserves_explicit_source_stride_and_refuses_invalid_counts() {
        for tier in ["default", "fast", "maximum", "quality"] {
            let parsed = Cli::try_parse_from([
                "pulsar",
                "generate",
                "video.mkv",
                "--profile",
                tier,
                "--source-sample-stride",
                "3",
            ])
            .unwrap();
            assert!(
                matches!(parsed.command,Some(Commands::Generate {profile,source_sample_stride:3,..}) if profile==tier)
            );
        }
        assert!(matches!(
            Cli::try_parse_from(["pulsar", "generate", "video.mkv"])
                .unwrap()
                .command,
            Some(Commands::Generate {
                source_sample_stride: 1,
                ..
            })
        ));
        for count in ["0", "-1", "1.5", "NaN", "4294967296"] {
            assert!(Cli::try_parse_from([
                "pulsar",
                "generate",
                "video.mkv",
                "--source-sample-stride",
                count
            ])
            .is_err());
        }
    }
    #[test]
    fn complete_legacy_generate_grammar_is_preserved() {
        Cli::try_parse_from([
            "pulsar",
            "generate",
            "video.mp4",
            "-o",
            "out.funscript",
            "--model",
            "model.onnx",
            "--conf",
            "0.5",
            "--fps",
            "24",
            "--width",
            "256",
            "--height",
            "256",
            "--detrend-window",
            "2",
            "--norm-window",
            "3",
            "--batch-size",
            "20",
            "--vr",
            "--pov",
            "--no-balance-global",
            "--no-keyframe-reduction",
            "--cut-diff",
            "30",
            "--cut-flow",
            "8",
            "--multi-axis",
            "--profile",
            "dense",
            "--cuts",
            "cuts.json",
        ])
        .unwrap();
    }

    #[test]
    fn annotated_preview_requires_one_explicit_model_and_source_clock() {
        let base = [
            "pulsar",
            "project",
            "preview-annotated",
            "project",
            "125000000",
            "--annotated-view",
            "view.json",
            "--source-version",
            "source",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut pack = base.to_vec();
        pack.extend(["--model-pack", "digest"]);
        assert!(Cli::try_parse_from(&pack).is_ok());
        pack.extend([
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
        ]);
        assert!(Cli::try_parse_from(pack).is_err());
        let mut file = base.to_vec();
        file.extend([
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
        ]);
        assert!(Cli::try_parse_from(file).is_ok());
    }
    #[test]
    fn detector_letterbox_profile_is_explicit_and_conflicts_with_temporal_recipes() {
        let base = [
            "pulsar",
            "model",
            "profile",
            "/private/model.onnx",
            "--width",
            "640",
            "--height",
            "640",
            "--class-count",
            "80",
        ];
        let mut args = base.to_vec();
        args.extend(["--letterbox", "--padding-byte", "7"]);
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Some(Commands::Model {
                command: ModelCommands::Profile {
                    letterbox: true,
                    padding_byte: Some(7),
                    ..
                }
            })
        ));
        for tail in [
            vec!["--padding-byte", "114"],
            vec!["--letterbox", "--padding-byte", "256"],
            vec!["--letterbox", "--temporal-script-position"],
            vec!["--letterbox", "--annotated-temporal-position"],
        ] {
            let mut args = base.to_vec();
            args.extend(tail);
            assert!(Cli::try_parse_from(args).is_err());
        }
        assert!(matches!(
            Cli::try_parse_from(base).unwrap().command,
            Some(Commands::Model {
                command: ModelCommands::Profile {
                    letterbox: false,
                    padding_byte: None,
                    ..
                }
            })
        ));
    }
    #[test]
    fn annotated_view_requires_profile_and_rejects_competing_inputs() {
        let base = [
            "pulsar",
            "generate",
            "video.mkv",
            "--annotated-view",
            "view.json",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut pack = base.to_vec();
        pack.extend(["--model-pack", "digest"]);
        assert!(Cli::try_parse_from(&pack).is_ok());
        pack.extend(["--video-view", "view.json"]);
        assert!(Cli::try_parse_from(pack).is_err());
        let mut incomplete = base.to_vec();
        incomplete.extend(["--model", "model.onnx"]);
        assert!(Cli::try_parse_from(incomplete).is_err());
        let mut args = base.to_vec();
        args.extend([
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
        ]);
        assert!(Cli::try_parse_from(&args).is_ok());
        assert!(Cli::try_parse_from([
            "pulsar",
            "generate",
            "video.mkv",
            "--model-pack",
            "digest",
            "--annotated-view",
            "view.json"
        ])
        .is_ok());
        for flag in [
            "--video-view",
            "--target-seed",
            "--raw-frame-pair",
            "--model-pack",
        ] {
            let mut bad = args.clone();
            bad.extend([flag, "other"]);
            assert!(Cli::try_parse_from(bad).is_err());
        }
    }
    #[test]
    fn annotated_stash_catalog_requires_file_profile_or_pack_and_refuses_global_view() {
        let base = [
            "pulsar",
            "stash",
            "generate-missing",
            "--mappings",
            "mappings.json",
            "--annotated-views",
            "views.json",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut pack = base.to_vec();
        pack.extend(["--model-pack", "digest"]);
        assert!(Cli::try_parse_from(&pack).is_ok());
        pack.extend(["--video-view", "view.json"]);
        assert!(Cli::try_parse_from(pack).is_err());
        let mut incomplete = base.to_vec();
        incomplete.extend(["--model", "model.onnx"]);
        assert!(Cli::try_parse_from(incomplete).is_err());
        let mut args = base.to_vec();
        args.extend([
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
        ]);
        assert!(Cli::try_parse_from(&args).is_ok());
        for flag in ["--video-view", "--model-pack"] {
            let mut bad = args.clone();
            bad.extend([flag, "other"]);
            assert!(Cli::try_parse_from(bad).is_err());
        }
    }

    #[test]
    fn explicit_inference_profile_requires_model_and_replaces_class_guessing() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "generate",
            "video.mp4",
            "--inference-profile",
            "profile.json"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "pulsar",
            "generate",
            "video.mp4",
            "--model",
            "model.onnx",
            "--inference-profile",
            "profile.json",
            "--model-class-count",
            "1"
        ])
        .is_err());
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "generate",
                "video.mp4",
                "--model",
                "model.onnx",
                "--inference-profile",
                "profile.json"
            ])
            .unwrap()
            .command,
            Some(Commands::Generate {
                inference_profile: Some(_),
                ..
            })
        ));
    }
}

#[cfg(test)]
mod native_preparation_cli_tests {
    use super::*;
    #[test]
    fn native_preparation_local_recovery_commands_require_project_and_identity() {
        assert!(Cli::try_parse_from(["pulsar", "native-preparation", "saved", "project"]).is_ok());
        assert!(
            Cli::try_parse_from(["pulsar", "native-preparation", "dismiss", "project"]).is_err()
        );
        assert!(Cli::try_parse_from([
            "pulsar",
            "native-preparation",
            "dismiss",
            "project",
            "--original-request-id",
            "original"
        ])
        .is_ok());
    }

    #[test]
    fn native_preparation_start_requires_recoverable_identity_and_epoch() {
        let args = [
            "pulsar",
            "native-preparation",
            "start",
            "project",
            "--revision",
            "0",
            "--engine-epoch",
            "boot",
            "--preparation-ms",
            "1000",
            "--retention-ms",
            "2000",
        ];
        assert!(Cli::try_parse_from(args).is_err());
        let mut complete = args.to_vec();
        complete.extend(["--request-id", "start-identity"]);
        let parsed = Cli::try_parse_from(complete).unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::NativePreparation {command:NativePreparationCommands::Start {request_id,..}}) if request_id=="start-identity")
        );
    }
}

#[cfg(test)]
mod image_origin_cli_tests {
    use super::*;
    #[test]
    fn retained_image_origin_query_requires_explicit_project_and_candidate() {
        let parsed = Cli::try_parse_from([
            "pulsar",
            "project",
            "candidate-image-origin-status",
            "project",
            "candidate",
        ])
        .unwrap();
        assert!(
            matches!(parsed.command,Some(Commands::Project {command:ProjectCommands::CandidateImageOriginStatus {project_id,candidate_id}}) if project_id=="project" && candidate_id=="candidate")
        );
        assert!(Cli::try_parse_from([
            "pulsar",
            "project",
            "candidate-image-origin-status",
            "project"
        ])
        .is_err());
    }
}

#[cfg(test)]
mod browser_start_parsing_tests {
    use super::*;
    #[test]
    fn prepared_browser_controls_require_selection_enablement_and_exclude_existing_guard() {
        let base = ["pulsar", "browser-live", "project", "1", "options.json"];
        let mut selected = base.to_vec();
        selected.extend(["--handy-browser-play", "/input/prepared.json"]);
        assert!(Cli::try_parse_from(&selected).is_err());
        selected.push("--allow-handy-browser-control");
        assert!(matches!(
            Cli::try_parse_from(&selected).unwrap().command,
            Some(Commands::BrowserLive {
                handy_browser_play: Some(_),
                allow_handy_browser_control: true,
                ..
            })
        ));
        selected.extend([
            "--handy-player-guard",
            "/input/guard.json",
            "--allow-handy-player-monitoring",
        ]);
        assert!(Cli::try_parse_from(selected).is_err());
        let mut missing = base.to_vec();
        missing.push("--allow-handy-browser-control");
        assert!(Cli::try_parse_from(missing).is_err());
    }
    #[test]
    fn browser_dossier_review_preserves_project_and_record_revisions_with_explicit_operator() {
        let args = [
            "pulsar",
            "handy-browser-start-qualification-accept",
            "project",
            "dossier",
            "--revision",
            "4",
            "--record-revision",
            "2",
            "--operator",
            "operator",
            "--request-id",
            "review",
        ];
        assert!(Cli::try_parse_from(args).is_err());
        let mut approved = args.to_vec();
        approved.push("--acknowledge-measured-evidence");
        assert!(matches!(
            Cli::try_parse_from(approved).unwrap().command,
            Some(Commands::HandyBrowserStartQualificationAccept {
                revision: 4,
                record_revision: 2,
                acknowledge_measured_evidence: true,
                ..
            })
        ));
        let withdraw = [
            "pulsar",
            "handy-browser-start-qualification-withdraw",
            "project",
            "dossier",
            "--record-revision",
            "3",
            "--reason",
            "evidence replaced",
            "--request-id",
            "withdraw",
        ];
        assert!(Cli::try_parse_from(withdraw).is_ok());
    }
}

#[cfg(test)]
mod browser_hsp_parsing_tests {
    use super::*;

    #[test]
    fn browser_hsp_defaults_to_disabled_and_requires_selection_and_enablement_together() {
        let base = ["pulsar", "browser-live", "project", "7", "options.json"];
        assert!(matches!(
            Cli::try_parse_from(base).unwrap().command,
            Some(Commands::BrowserLive {
                handy_hsp: None,
                allow_handy_hsp_control: false,
                ..
            })
        ));
        let mut enablement_only = base.to_vec();
        enablement_only.push("--allow-handy-hsp-control");
        assert!(Cli::try_parse_from(enablement_only).is_err());
        let mut selected = base.to_vec();
        selected.extend(["--handy-hsp", "/input/hsp.json"]);
        assert!(Cli::try_parse_from(&selected).is_err());
        selected.push("--allow-handy-hsp-control");
        assert!(matches!(
            Cli::try_parse_from(selected).unwrap().command,
            Some(Commands::BrowserLive {
                project,
                revision: 7,
                handy_hsp: Some(path),
                allow_handy_hsp_control: true,
                handy_player_guard: None,
                handy_browser_play: None,
                ..
            }) if project == "project" && path == std::path::Path::new("/input/hsp.json")
        ));
    }

    #[test]
    fn browser_hsp_excludes_each_other_handy_mode_in_either_argument_order() {
        let base = ["pulsar", "browser-live", "project", "7", "options.json"];
        let hsp = [
            "--handy-hsp",
            "/input/hsp.json",
            "--allow-handy-hsp-control",
        ];
        for other in [
            [
                "--handy-player-guard",
                "/input/guard.json",
                "--allow-handy-player-monitoring",
            ],
            [
                "--handy-browser-play",
                "/input/prepared.json",
                "--allow-handy-browser-control",
            ],
        ] {
            for hsp_first in [true, false] {
                let mut args = base.to_vec();
                for mode in if hsp_first {
                    [hsp, other]
                } else {
                    [other, hsp]
                } {
                    args.extend(mode);
                }
                assert_eq!(
                    Cli::try_parse_from(args).unwrap_err().kind(),
                    clap::error::ErrorKind::ArgumentConflict
                );
            }
        }
    }
}

#[cfg(test)]
mod hsp_qualification_cli_tests {
    use super::*;
    #[test]
    fn qualification_context_defaults_to_hssp_and_requires_an_explicit_supported_selector() {
        let legacy = Cli::try_parse_from(["pulsar", "handy-qualification-context"]).unwrap();
        assert!(
            matches!(legacy.command, Some(Commands::HandyQualificationContext {transport}) if transport == "hssp")
        );
        let hsp = Cli::try_parse_from([
            "pulsar",
            "handy-qualification-context",
            "--transport",
            "hsp",
        ])
        .unwrap();
        assert!(
            matches!(hsp.command, Some(Commands::HandyQualificationContext {transport}) if transport == "hsp")
        );
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-qualification-context",
            "--transport",
            "automatic"
        ])
        .is_err());
    }
}

#[cfg(test)]
mod live_hsp_cli_tests {
    use super::*;

    #[test]
    fn live_binding_requires_separate_control_consent_and_complete_selection() {
        let args = [
            "pulsar",
            "handy-hsp-live-bind",
            "project",
            "--revision",
            "3",
            "--preparation-request-id",
            "prepare",
            "--capture-epoch",
            "capture",
            "--inference-attempt-id",
            "inference",
            "--browser-timing-binding-id",
            "timing",
            "--source-qualification-id",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--source-qualification-revision",
            "2",
            "--request-id",
            "stream",
            "--allow-online-control",
            "--acknowledge-current-environment",
        ];
        assert!(matches!(
            Cli::try_parse_from(args).unwrap().command,
            Some(Commands::HandyHspLiveBind {
                acknowledge_current_environment: true,
                allow_online_control: true,
                revision: 3,
                ..
            })
        ));
        for omitted in [
            "--allow-online-control",
            "--acknowledge-current-environment",
        ] {
            let missing = args
                .iter()
                .copied()
                .filter(|value| *value != omitted)
                .collect::<Vec<_>>();
            assert!(Cli::try_parse_from(missing).is_err(), "missing {omitted}");
        }
        let missing_capture = args
            .iter()
            .copied()
            .filter(|value| *value != "--capture-epoch" && *value != "capture")
            .collect::<Vec<_>>();
        assert!(Cli::try_parse_from(missing_capture).is_err());
    }

    #[test]
    fn live_play_requires_revision_but_original_stop_and_status_do_not() {
        assert!(Cli::try_parse_from([
            "pulsar",
            "handy-hsp-live-play",
            "project",
            "--stream-request-id",
            "stream",
            "--request-id",
            "play"
        ])
        .is_err());
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "handy-hsp-live-play",
                "project",
                "--revision",
                "3",
                "--stream-request-id",
                "stream",
                "--request-id",
                "play"
            ])
            .unwrap()
            .command,
            Some(Commands::HandyHspLivePlay { revision: 3, .. })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "handy-hsp-live-stop",
                "project",
                "--stream-request-id",
                "stream",
                "--request-id",
                "stop"
            ])
            .unwrap()
            .command,
            Some(Commands::HandyHspLiveStop { .. })
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "pulsar",
                "handy-hsp-live-status",
                "project",
                "--stream-request-id",
                "stream"
            ])
            .unwrap()
            .command,
            Some(Commands::HandyHspLiveStatus { .. })
        ));
    }
}

#[cfg(test)]
mod finite_input_cli_tests {
    use super::*;
    #[test]
    fn closing_input_requires_an_original_request_and_one_exact_final_selection() {
        let base = [
            "pulsar",
            "live-capture",
            "close-input",
            "project",
            "1",
            "epoch",
        ];
        assert!(Cli::try_parse_from(base).is_err());
        let mut zero = base.to_vec();
        zero.extend(["--request-id", "close-original", "--no-frames"]);
        assert!(matches!(
            Cli::try_parse_from(zero).unwrap().command,
            Some(Commands::LiveCapture {
                command: LiveCaptureCommands::CloseInput {
                    no_frames: true,
                    final_frame: None,
                    ..
                }
            })
        ));
        let mut exact = base.to_vec();
        exact.extend([
            "--request-id",
            "close-original",
            "--final-frame",
            "/absolute/original-header.json",
        ]);
        assert!(Cli::try_parse_from(exact.clone()).is_ok());
        exact.push("--no-frames");
        assert!(Cli::try_parse_from(exact).is_err());
        let mut missing_id = base.to_vec();
        missing_id.push("--no-frames");
        assert!(Cli::try_parse_from(missing_id).is_err());
    }
}

#[cfg(test)]
mod inference_equivalence_cli_tests {
    use super::*;
    #[test]
    fn equivalence_environment_requires_profile_but_no_mutation_request_id() {
        let args = ["pulsar", "model", "equivalence", "environment"];
        assert!(Cli::try_parse_from(args).is_err());
        let path = crate::test_host_path("declared-profile.json");
        let mut complete = args.to_vec();
        complete.extend(["--profile", path.as_str()]);
        let cli = Cli::try_parse_from(complete.clone()).unwrap();
        assert!(
            matches!(cli.command, Some(Commands::Model { command: ModelCommands::Equivalence { command: InferenceEquivalenceCommands::Environment { profile } } }) if profile.as_path() == std::path::Path::new(&path))
        );
        complete.extend(["--request-id", "unused-mutation-id"]);
        assert!(Cli::try_parse_from(complete).is_err());
    }

    #[test]
    fn equivalence_decisions_require_original_identity_and_explicit_acceptance() {
        for incomplete in [
            vec![
                "pulsar",
                "model",
                "equivalence",
                "import",
                "/authored/manifest.json",
                "--manifest-sha256",
                "digest",
            ],
            vec![
                "pulsar",
                "model",
                "equivalence",
                "accept",
                "dossier",
                "--expected-record-revision",
                "1",
                "--operator",
                "supplied-owner",
                "--request-id",
                "accept-original",
            ],
            vec![
                "pulsar",
                "model",
                "equivalence",
                "withdraw",
                "dossier",
                "--expected-record-revision",
                "1",
                "--reason",
                "supplied reason",
            ],
        ] {
            assert!(Cli::try_parse_from(incomplete).is_err());
        }
        let cli = Cli::try_parse_from([
            "pulsar",
            "model",
            "equivalence",
            "accept",
            "dossier",
            "--expected-record-revision",
            "1",
            "--operator",
            "supplied-owner",
            "--acknowledge-measured-equivalence",
            "--request-id",
            "accept-original",
        ])
        .unwrap();
        assert!(
            matches!(cli.command, Some(Commands::Model { command: ModelCommands::Equivalence { command: InferenceEquivalenceCommands::Accept { request_id, acknowledge_measured_equivalence: true, expected_record_revision: 1, .. } } }) if request_id == "accept-original")
        );
        for limit in ["0", "17"] {
            assert!(Cli::try_parse_from([
                "pulsar",
                "model",
                "equivalence",
                "list",
                "--limit",
                limit
            ])
            .is_err());
        }
        assert!(Cli::try_parse_from(["pulsar", "model", "equivalence", "recovery-retry"]).is_err());
        let retry = Cli::try_parse_from([
            "pulsar",
            "model",
            "equivalence",
            "recovery-retry",
            "original-id",
        ])
        .unwrap();
        assert!(
            matches!(retry.command, Some(Commands::Model { command: ModelCommands::Equivalence { command: InferenceEquivalenceCommands::RecoveryRetry { request_id } } }) if request_id == "original-id")
        );
    }
}

#[cfg(test)]
mod inference_fallback_cli_tests {
    use super::*;

    #[test]
    fn fallback_cli_is_opt_in_and_requires_explicit_profile_model_or_pack() {
        let parsed = Cli::try_parse_from(["pulsar", "generate", "video.mp4"]).unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::Generate {
                fallback_selection: None,
                ..
            })
        ));
        let selection = crate::test_host_path("fallback-selection.json");
        for extra in [
            vec![],
            vec!["--model", "model.onnx"],
            vec!["--model", "model.onnx", "--model-class-count", "1"],
            vec!["--inference-profile", "profile.json"],
        ] {
            let mut args = vec![
                "pulsar",
                "generate",
                "video.mp4",
                "--fallback-selection",
                selection.as_str(),
            ];
            args.extend(extra);
            assert!(Cli::try_parse_from(args).is_err());
        }
        for extra in [
            vec![
                "--model",
                "model.onnx",
                "--inference-profile",
                "profile.json",
            ],
            vec!["--model-pack", "declared-pack-digest"],
        ] {
            let mut args = vec![
                "pulsar",
                "generate",
                "video.mp4",
                "--fallback-selection",
                selection.as_str(),
            ];
            args.extend(extra);
            assert!(
                matches!(Cli::try_parse_from(args.clone()).unwrap().command,Some(Commands::Generate {fallback_selection:Some(path),..}) if path==std::path::Path::new(&selection))
            );
            args.extend(["--raw-frame-pair", "pair.json"]);
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn fallback_cli_refuses_relative_or_non_normalized_selection_paths() {
        let root = std::env::temp_dir();
        for path in [
            "policy.json".to_owned(),
            root.join("..")
                .join("policy.json")
                .to_string_lossy()
                .into_owned(),
            format!("{}/./policy.json", root.display()),
        ] {
            assert!(Cli::try_parse_from([
                "pulsar",
                "generate",
                "video.mp4",
                "--model-pack",
                "digest",
                "--fallback-selection",
                path.as_str()
            ])
            .is_err());
        }
    }
}
