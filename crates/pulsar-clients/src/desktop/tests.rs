use super::*;

#[cfg(test)]
mod detector_score_tests {
    use super::*;
    #[test]
    fn generation_and_batch_score_choice_requires_an_applicable_detector() {
        let detector = ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        assert_eq!(
            selected_detector_score_threshold(true, 0.75, Some(&detector)),
            Some(0.75)
        );
        assert_eq!(
            selected_detector_score_threshold(false, 0.75, Some(&detector)),
            None
        );
        assert_eq!(selected_detector_score_threshold(true, 0.75, None), None);
        let temporal = ModelInputContract {
            channels: 6,
            decoder: DetectorDecoder::TemporalScriptPosition,
            ..detector.clone()
        };
        assert_eq!(
            selected_detector_score_threshold(true, 0.75, Some(&temporal)),
            None
        );
        let corrupt = GenerationSettings {
            model_input: Some(detector.clone()),
            detector_score_threshold: selected_detector_score_threshold(
                true,
                f32::NAN,
                Some(&detector),
            ),
            ..Default::default()
        };
        assert!(corrupt.validate().is_err());
    }
}

#[cfg(test)]
mod source_kind_tests {
    use super::{independent_audio_rate, independent_audio_selection, select_media_source};
    use pulsar_protocol::{SourceKind, SourceSummary, SourceVersionId};

    fn source(id: &str, label: &str, kind: SourceKind) -> SourceSummary {
        SourceSummary {
            source_version: SourceVersionId::new(id).unwrap(),
            label: label.into(),
            kind,
        }
    }

    #[test]
    fn independent_audio_selection_refuses_stale_same_and_nonmedia_sources() {
        let sources = vec![
            source("video", "Video", SourceKind::Media),
            source("audio", "Audio", SourceKind::Media),
            source("script", "Audio.wav", SourceKind::Funscript),
        ];
        let primary = Some(&sources[0].source_version);
        let selected = independent_audio_selection(
            &sources,
            primary,
            Some(&sources[1].source_version),
            [(0, 1), (1, 8)],
            (1, 1),
        )
        .unwrap();
        assert_eq!(selected.source_version, sources[1].source_version);
        assert_eq!(
            selected.alignment.audio_anchor,
            pulsar_protocol::SourceTimestamp::new(1, 8).unwrap()
        );
        for candidate in [None, primary, Some(&sources[2].source_version)] {
            assert!(independent_audio_selection(
                &sources,
                primary,
                candidate,
                [(0, 1), (0, 1)],
                (1, 1)
            )
            .is_err());
        }
        assert!(independent_audio_selection(
            &sources[..1],
            primary,
            Some(&sources[1].source_version),
            [(0, 1), (0, 1)],
            (1, 1)
        )
        .is_err());
        assert!(independent_audio_selection(
            &sources,
            primary,
            Some(&sources[1].source_version),
            [(0, 0), (0, 1)],
            (1, 1)
        )
        .is_err());
    }

    #[test]
    fn audio_rate_text_preserves_full_integer_precision() {
        assert_eq!(
            independent_audio_rate("9223372036854775807", "4294967295").unwrap(),
            (i64::MAX, u32::MAX)
        );
        assert_eq!(
            independent_audio_rate("9007199254740993", "1").unwrap(),
            (9_007_199_254_740_993, 1)
        );
        for (n, d) in [
            ("9223372036854775808", "1"),
            ("1", "4294967296"),
            ("0", "1"),
            ("-1", "1"),
            ("1", "0"),
            ("1.5", "1"),
            ("", "1"),
        ] {
            assert!(independent_audio_rate(n, d).is_err());
        }
    }

    #[test]
    fn independent_audio_selection_retains_rate_and_refuses_invalid_rates() {
        let sources = vec![
            source("video", "Video", SourceKind::Media),
            source("audio", "Audio", SourceKind::Media),
        ];
        let select = |rate| {
            independent_audio_selection(
                &sources,
                Some(&sources[0].source_version),
                Some(&sources[1].source_version),
                [(0, 1), (1, 8)],
                rate,
            )
        };
        assert!(select((2, 2)).unwrap().alignment.rate.is_none());
        let selected = select((1001, 1000)).unwrap();
        assert_eq!(selected.alignment.rate.unwrap().numerator(), 1001);
        assert_eq!(selected.alignment.rate.unwrap().denominator(), 1000);
        for rate in [(0, 1), (-1, 1), (1, 0)] {
            assert!(select(rate).is_err());
        }
    }

    #[test]
    fn synthetic_and_script_sources_never_become_media_from_their_labels() {
        let sources = vec![
            source("text", "movie.mp4", SourceKind::GenerationInput),
            source("script", "image.png", SourceKind::Funscript),
        ];
        assert_eq!(select_media_source(&sources, None), None);
        for source in &sources {
            assert_eq!(
                select_media_source(&sources, Some(&source.source_version)),
                None
            );
        }
    }

    #[test]
    fn media_selection_skips_nonmedia_and_preserves_an_existing_media_choice() {
        let sources = vec![
            source("text", "Generation input", SourceKind::GenerationInput),
            source("media-one", "script.funscript", SourceKind::Media),
            source("script", "movie.mp4", SourceKind::Funscript),
            source("media-two", "Generation input", SourceKind::Media),
        ];
        assert_eq!(
            select_media_source(&sources, None),
            Some(sources[1].source_version.clone())
        );
        assert_eq!(
            select_media_source(&sources, Some(&sources[3].source_version)),
            Some(sources[3].source_version.clone())
        );
        assert_eq!(
            select_media_source(&sources, Some(&sources[0].source_version)),
            Some(sources[1].source_version.clone())
        );
    }

    #[test]
    fn removed_or_reclassified_media_is_not_retained_after_snapshot_refresh() {
        let selected = SourceVersionId::new("old-media").unwrap();
        assert_eq!(select_media_source(&[], Some(&selected)), None);
        let sources = vec![source(
            "old-media",
            "movie.mp4",
            SourceKind::GenerationInput,
        )];
        assert_eq!(select_media_source(&sources, Some(&selected)), None);
    }
}

#[cfg(test)]
mod diagnostics_freshness_tests {
    use super::accept_diagnostics_report;
    use pulsar_protocol::{CandidateId, DiagnosticsReport, ProjectId, RevisionId};

    fn report(project: &str, revision: u64, candidate: Option<&str>) -> DiagnosticsReport {
        DiagnosticsReport {
            project_id: ProjectId::new(project).unwrap(),
            revision: RevisionId::new(revision),
            candidate_id: candidate.map(|id| CandidateId::new(id).unwrap()),
            base_revision: candidate.map(|_| RevisionId::new(revision)),
            protected_regions: Vec::new(),
            segment_speed: None,
            issues: Vec::new(),
        }
    }

    #[test]
    fn late_old_project_and_revision_reports_do_not_replace_current_diagnostics() {
        let project = ProjectId::new("current-project").unwrap();
        let view = Some((&project, RevisionId::new(7)));
        let mut current = None;
        assert!(accept_diagnostics_report(
            &mut current,
            report("current-project", 7, None),
            view,
            None
        ));
        assert!(!accept_diagnostics_report(
            &mut current,
            report("old-project", 7, None),
            view,
            None
        ));
        assert!(!accept_diagnostics_report(
            &mut current,
            report("current-project", 6, None),
            view,
            None
        ));
        let retained = current.unwrap();
        assert_eq!(retained.project_id, project);
        assert_eq!(retained.revision, RevisionId::new(7));
    }

    #[test]
    fn candidate_diagnostics_require_the_visible_candidate_identity() {
        let project = ProjectId::new("project").unwrap();
        let candidate = CandidateId::new("visible-candidate").unwrap();
        let view = Some((&project, RevisionId::new(7)));
        let mut current = None;
        assert!(!accept_diagnostics_report(
            &mut current,
            report("project", 7, Some("old-candidate")),
            view,
            Some(&candidate)
        ));
        assert!(!accept_diagnostics_report(
            &mut current,
            report("project", 7, Some("visible-candidate")),
            view,
            None
        ));
        assert!(accept_diagnostics_report(
            &mut current,
            report("project", 7, Some("visible-candidate")),
            view,
            Some(&candidate)
        ));
        assert_eq!(
            current.as_ref().unwrap().candidate_id.as_ref(),
            Some(&candidate)
        );
    }

    #[test]
    fn diagnostics_for_an_unopened_project_are_never_accepted() {
        let mut current = None;
        assert!(!accept_diagnostics_report(
            &mut current,
            report("project", 7, None),
            None,
            None
        ));
        assert!(current.is_none());
    }
}

use crate::motion::{CandidateSnapshot, ProjectSnapshot};

#[cfg(test)]
mod edit_transport_tests {
    use super::*;
    use crate::motion::{fixture_descriptor, EditProposal};

    struct EditApi {
        calls: Vec<&'static str>,
        conflict: bool,
    }
    impl EngineApi for EditApi {
        fn upload_edit(
            &mut self,
            project: ProjectId,
            revision: RevisionId,
            program: &MotionProgram,
            _: &str,
        ) -> anyhow::Result<CandidateSnapshot> {
            assert_eq!(project.as_str(), "project");
            assert_eq!(revision.value(), 4);
            self.calls.push("upload");
            let candidate_id = CandidateId::new("edit-candidate").unwrap();
            Ok(CandidateSnapshot {
                project_interval: None,
                candidate_id: candidate_id.clone(),
                project_id: project.clone(),
                base_revision: revision,
                motion: fixture_descriptor(
                    program,
                    MotionBinding::Candidate {
                        project_id: project,
                        candidate_id,
                        base_revision: revision,
                    },
                ),
                program: program.clone().into(),
                job_id: None,
                review: Vec::new(),
            })
        }
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> anyhow::Result<ResponseBody> {
            assert!(
                matches!(command, Command::CommitCandidate { candidate_id } if candidate_id.as_str() == "edit-candidate")
            );
            let project = project.unwrap();
            assert_eq!(project.as_str(), "project");
            assert_eq!(revision, Some(RevisionId::new(4)));
            self.calls.push("commit");
            if self.conflict {
                Err(ProtocolError::new(ErrorCode::RevisionConflict, "stale base").into())
            } else {
                let revision = RevisionId::new(5);
                let program = MotionProgram::default();
                Ok(ResponseBody::Project(Box::new(ProjectSnapshot {
                    motion: fixture_descriptor(
                        &program,
                        MotionBinding::ProjectRevision {
                            project_id: project.clone(),
                            revision,
                        },
                    ),
                    project_id: project,
                    revision,
                    program: program.into(),
                    name: "Project".into(),
                    sources: vec![],
                    placements: vec![],
                })))
            }
        }
    }
    fn draft() -> RpcWork {
        RpcWork {
            command: RpcCommand::Edit(EditProposal {
                program: MotionProgram::default(),
                label: "Edit".into(),
            }),
            project: Some(ProjectId::new("project").unwrap()),
            revision: Some(RevisionId::new(4)),
        }
    }
    fn repair_work() -> RpcWork {
        let program = MotionProgram::new(vec![MotionTrack::new(
            Axis::Stroke,
            [(0, 0.0), (20, 1.0)]
                .into_iter()
                .map(|(t, p)| {
                    MotionAction::new(
                        ProjectTime::from_nanos(t * 1_000_000),
                        NormalizedPosition::new(p).unwrap(),
                        EvidenceKind::Synthesized,
                    )
                    .unwrap()
                })
                .collect(),
        )
        .unwrap()])
        .unwrap();
        RpcWork {
            command: RpcCommand::Repair(crate::repair_panel::RepairRequest {
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(4),
                program: std::sync::Arc::new(program),
                options: ScriptRepairOptions::new(450.0, 20, 2).unwrap(),
            }),
            project: Some(ProjectId::new("project").unwrap()),
            revision: Some(RevisionId::new(4)),
        }
    }
    #[test]
    fn repair_rpc_runs_as_background_candidate_work_without_commit() {
        let parent = std::thread::current().id();
        let (result, calls) = std::thread::spawn(move || {
            assert_ne!(std::thread::current().id(), parent);
            let mut api = EditApi {
                calls: vec![],
                conflict: false,
            };
            let result = execute_work(&mut api, repair_work());
            (result, api.calls)
        })
        .join()
        .unwrap();
        assert!(matches!(result.unwrap(), ResponseBody::Candidate(_)));
        assert_eq!(calls, vec!["upload"]);
    }
    #[test]
    fn repair_rpc_refuses_missing_or_rebound_context_before_effects() {
        let mut api = EditApi {
            calls: vec![],
            conflict: false,
        };
        for (project, revision) in [
            (None, Some(RevisionId::new(4))),
            (
                Some(ProjectId::new("other").unwrap()),
                Some(RevisionId::new(4)),
            ),
            (Some(ProjectId::new("project").unwrap()), None),
        ] {
            let mut work = repair_work();
            work.project = project;
            work.revision = revision;
            assert!(execute_work(&mut api, work).is_err());
        }
        assert!(api.calls.is_empty());
    }
    #[test]
    fn gui_draft_uses_shared_upload_then_normal_pinned_candidate_commit() {
        let mut api = EditApi {
            calls: Vec::new(),
            conflict: false,
        };
        assert!(matches!(
            execute_work(&mut api, draft()).unwrap(),
            ResponseBody::Project(_)
        ));
        assert_eq!(api.calls, vec!["upload", "commit"]);
    }
    #[test]
    fn stale_gui_commit_retains_candidate_identity_and_is_never_replayed() {
        let mut api = EditApi {
            calls: Vec::new(),
            conflict: true,
        };
        let error = execute_work(&mut api, draft()).unwrap_err();
        assert!(error.to_string().contains("edit-candidate"));
        assert!(error.to_string().contains("stale base"));
        assert_eq!(
            error.downcast_ref::<ProtocolError>().unwrap().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(api.calls, vec!["upload", "commit"]);
    }
}

#[cfg(test)]
mod placement_snapshot_seek_tests {
    use super::*;
    fn placement(anchor: i64, rate: i32) -> SourcePlacementSpec {
        SourcePlacementSpec {
            source_version: SourceVersionId::new("source").unwrap(),
            source_start: SourceTimestamp::new(1, 2).unwrap(),
            source_end: SourceTimestamp::new(7, 2).unwrap(),
            source_anchor: SourceTimestamp::new(1, 2).unwrap(),
            project_anchor: ProjectTime::from_nanos(anchor),
            rate_numerator: rate,
            rate_denominator: 1,
        }
    }
    #[test]
    fn saved_placement_previews_valid_anchor_instead_of_out_of_trim_old_playhead() {
        for (anchor, rate) in [(1_000_000_000, 2), (-2_000_000_000, -2)] {
            let spec = placement(anchor, rate);
            spec.validate().unwrap();
            assert!(spec.source_at_project(ProjectTime::from_nanos(0)).is_err());
            for source_changed in [false, true] {
                let seconds =
                    snapshot_preview_seek(0.0, Some(&spec), true, source_changed).unwrap();
                assert_eq!(seconds, anchor as f64 / 1e9);
                assert_eq!(
                    spec.source_at_project(ProjectTime::from_nanos((seconds * 1e9).round() as i64))
                        .unwrap(),
                    ExactSeconds::from_source(spec.source_anchor)
                );
            }
        }
    }
    #[test]
    fn refresh_preserves_seek_and_original_clip_selection_preserves_reset_semantics() {
        let spec = placement(1_000_000_000, 2);
        assert_eq!(snapshot_preview_seek(1.25, Some(&spec), false, false), None);
        assert_eq!(
            snapshot_preview_seek(1.25, Some(&spec), false, true),
            Some(1.0)
        );
        assert_eq!(snapshot_preview_seek(1.25, None, false, true), Some(0.0));
        assert_eq!(snapshot_preview_seek(1.25, None, true, false), Some(1.25));
        assert_eq!(snapshot_preview_seek(1.25, None, false, false), None);
    }
}

#[cfg(test)]
mod image_origin_scope_tests {
    use super::*;
    #[test]
    fn candidate_origin_presentation_rejects_changed_namespace_revision_and_motion() {
        let program = MotionProgram::new(vec![]).unwrap();
        let project_id = ProjectId::new("project").unwrap();
        let candidate_id = CandidateId::new("candidate").unwrap();
        let base_revision = RevisionId::new(4);
        let motion = crate::motion::fixture_descriptor(
            &program,
            MotionBinding::Candidate {
                project_id: project_id.clone(),
                candidate_id: candidate_id.clone(),
                base_revision,
            },
        );
        let candidate = CandidateSnapshot {
            project_interval: None,
            project_id: project_id.clone(),
            candidate_id: candidate_id.clone(),
            base_revision,
            motion: motion.clone(),
            program: program.into(),
            job_id: None,
            review: vec![],
        };
        // Scope comparison is independent of the typed evidence validator;
        // native Engine tests exercise valid retained records and mutation refusals.
        let report = CandidateImageOriginReport {
            project_id,
            candidate_id,
            base_revision,
            motion,
            origins: vec![],
            observed_temporal_motion: false,
            execution_authority: false,
            qualified: false,
        };
        assert!(image_origin_scope_matches(&report, &candidate));
        assert!(report.validate().is_err());
        let mut changed = report.clone();
        changed.project_id = ProjectId::new("other").unwrap();
        assert!(!image_origin_scope_matches(&changed, &candidate));
        let mut changed = report.clone();
        changed.candidate_id = CandidateId::new("other").unwrap();
        assert!(!image_origin_scope_matches(&changed, &candidate));
        let mut changed = report.clone();
        changed.base_revision = RevisionId::new(5);
        assert!(!image_origin_scope_matches(&changed, &candidate));
        let mut changed = report;
        changed.motion.program.sha256 = "0".repeat(64);
        assert!(!image_origin_scope_matches(&changed, &candidate));
    }
}
