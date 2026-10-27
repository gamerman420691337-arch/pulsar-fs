mod recording_candidate_tests {
    use super::*;
    fn recorded() -> (Fixture, LiveRecordingStageOptions) {
        let f = fixture();
        let (capture, attempt, work, result) = analysis_work(&f);
        let start = request(&f.session, Some(&f.project), Some(0), Command::StartLiveRecording { epoch: capture.identity.epoch.clone(), maximum_samples: 4 });
        f.engine.handle(start.clone()).result.unwrap();
        let options = LiveRecordingStageOptions {
            engine_epoch: capture.engine_epoch.clone(), recordings: vec![LiveRecordingSelection {epoch:capture.identity.epoch.clone(),recording_request_id:start.request_id}],
            range:TimeRange::new(ProjectTime::ZERO,ProjectTime::from_nanos(2_000_000_000)).unwrap(),maximum_interpolation_ns:100_000_000,
        };
        assert!(f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::StageLiveRecording {options:options.clone()})).result.is_err());
        assert!(f.engine.live_analysis_complete(&capture.identity.epoch,&attempt,work,result).unwrap());
        f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::StopLiveCapture {epoch:capture.identity.epoch})).result.unwrap();
        (f,options)
    }
    #[test]
    fn recording_candidate_retains_proofs_and_replays_after_registry_loss() {
        let (f,options)=recorded();
        let stage=request(&f.session,Some(&f.project),Some(0),Command::StageLiveRecording {options:options.clone()});
        let body=f.engine.handle(stage.clone()).result.unwrap();
        let ResponseBody::Candidate(candidate)=&body else {panic!("candidate response")};
        assert!(candidate.review.iter().any(|r|r.reason.as_str()=="live_recording_unqualified"));
        assert!(candidate.review.iter().any(|r|r.reason.as_str()=="live_recording_missing_coverage"));
        {
            let db=f.engine.db.lock().unwrap();let project=project_snapshot(&db,&f.project).unwrap();
            assert_eq!(project.revision.value(),0);assert_eq!(project.sources.len(),1);assert_eq!(project.sources[0].kind,SourceKind::LiveRecording);
            assert!(project_state(&db,&f.project).unwrap().program.tracks().is_empty());
            let state=candidate_state(&db,&candidate.candidate_id,&f.project).unwrap();
            assert_eq!(state.program.tracks()[0].actions()[0].evidence(),EvidenceKind::Inferred);assert_eq!(state.program.tracks()[0].gaps().len(),2);
            let source=f.engine.source(&db,&f.project,&project.sources[0].source_version).unwrap();
            let proof:serde_json::Value=serde_json::from_slice(&fs::read(source.path).unwrap()).unwrap();
            assert_eq!(proof["qualified"],false);assert_eq!(proof["pages"][0]["recording_request_id"],options.recordings[0].recording_request_id.as_str());assert_eq!(proof["pages"][0]["samples"].as_array().unwrap().len(),1);
        }
        *f.engine.live_captures.lock().unwrap()=live_captures::Registry::default();
        assert_eq!(encode(&body).unwrap(),encode(&f.engine.handle(stage).result.unwrap()).unwrap());
        assert_eq!(f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::StageLiveRecording {options})).result.unwrap_err().code,ErrorCode::NotFound);
        f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::CommitCandidate {candidate_id:candidate.candidate_id.clone()})).result.unwrap();
        let db=f.engine.db.lock().unwrap();assert_eq!(project_snapshot(&db,&f.project).unwrap().revision.value(),1);assert_eq!(project_state(&db,&f.project).unwrap().program.tracks()[0].gaps().len(),2);assert_eq!(db.query_row("SELECT COUNT(*) FROM candidates",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    }
    #[test]
    fn empty_recording_cannot_create_zero_motion_or_source() {
        let f=fixture();let (capture,_,_,_)=analysis_work(&f);
        let start=request(&f.session,Some(&f.project),Some(0),Command::StartLiveRecording {epoch:capture.identity.epoch.clone(),maximum_samples:4});
        f.engine.handle(start.clone()).result.unwrap();
        f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::StopLiveCapture {epoch:capture.identity.epoch.clone()})).result.unwrap();
        let options=LiveRecordingStageOptions {engine_epoch:capture.engine_epoch,recordings:vec![LiveRecordingSelection {epoch:capture.identity.epoch,recording_request_id:start.request_id}],
            range:TimeRange::new(ProjectTime::ZERO,ProjectTime::from_nanos(2_000_000_000)).unwrap(),maximum_interpolation_ns:100_000_000};
        assert!(f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::StageLiveRecording {options})).result.is_err());
        let db=f.engine.db.lock().unwrap();assert_eq!(db.query_row("SELECT COUNT(*) FROM candidates",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        let project=project_snapshot(&db,&f.project).unwrap();assert_eq!(project.revision.value(),0);assert!(project.sources.is_empty());
    }
    #[test]
    fn recording_candidate_refuses_foreign_intent_boot_and_revision() {
        let (f,good)=recorded();
        let mut bad=good.clone();bad.recordings[0].recording_request_id=RequestId::new("wrong-intent").unwrap();
        assert_eq!(f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::StageLiveRecording {options:bad})).result.unwrap_err().code,ErrorCode::RequestConflict);
        let mut bad=good.clone();bad.engine_epoch=AttemptId::new("wrong-boot").unwrap();
        assert_eq!(f.engine.handle(request(&f.session,Some(&f.project),Some(0),Command::StageLiveRecording {options:bad})).result.unwrap_err().code,ErrorCode::DependencyMismatch);
        assert_eq!(f.engine.handle(request(&f.session,Some(&f.project),Some(1),Command::StageLiveRecording {options:good.clone()})).result.unwrap_err().code,ErrorCode::RevisionConflict);
        let foreign=pair(&f.engine,"foreign recorder");
        f.engine.handle(request(&f.session,Some(&f.project),None,Command::Grant {session:foreign.clone(),project_id:f.project.clone(),scopes:vec![Scope::Read,Scope::Generate,Scope::Edit,Scope::ImportSource]})).result.unwrap();
        assert_eq!(f.engine.handle(request(&foreign,Some(&f.project),Some(0),Command::StageLiveRecording {options:good})).result.unwrap_err().code,ErrorCode::Forbidden);
        assert_eq!(f.engine.db.lock().unwrap().query_row("SELECT COUNT(*) FROM candidates",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}
