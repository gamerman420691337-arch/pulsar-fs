use super::*;
#[cfg(target_os = "linux")]
#[path = "assistant_native_handoff_tests.rs"]
mod native_handoff;
#[cfg(target_os = "linux")]
struct Fixture {
    engine: Arc<Engine>,
    session: SessionId,
    token: String,
    project: ProjectId,
    _dir: tempfile::TempDir,
}
#[cfg(target_os = "linux")]
impl Fixture {
    fn new() -> Self {
        Self::with_worker(PathBuf::from("/usr/bin/true"))
    }
    fn with_worker(worker: PathBuf) -> Self {
        Self::with_worker_memory(worker, 4 * 1024 * 1024 * 1024)
    }
    fn with_worker_memory(worker: PathBuf, max_ram_bytes: u64) -> Self {
        Self::with_worker_config(worker, |config| config.max_ram_bytes = max_ram_bytes)
    }
    fn with_worker_config(worker: PathBuf, configure: impl FnOnce(&mut EngineConfig)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut config = EngineConfig::new(dir.path().join("state"), worker);
        configure(&mut config);
        let engine = Engine::open(config).unwrap();
        let pair = engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "assistant regression".into(),
                    pairing_token: engine.token.clone(),
                },
            ))
            .result
            .unwrap();
        let ResponseBody::Paired {
            session,
            auth_token: token,
        } = pair
        else {
            panic!("pair")
        };
        let mut f = Self {
            engine,
            session,
            token,
            project: ProjectId::new("pending").unwrap(),
            _dir: dir,
        };
        let mut request = f.request(
            Command::CreateProject {
                name: "assistant fixture".into(),
            },
            None,
        );
        request.project = None;
        let ResponseBody::Project(p) = f.engine.handle(request).result.unwrap() else {
            panic!("project")
        };
        f.project = p.project_id;
        f
    }
    fn request(&self, command: Command, revision: Option<u64>) -> Request {
        Request {
            version: PROTOCOL_VERSION,
            request_id: id!(RequestId),
            session: Some(self.session.clone()),
            auth_token: Some(self.token.clone()),
            project: Some(self.project.clone()),
            expected_revision: revision.map(RevisionId::new),
            command,
        }
    }
    fn profile(&self) -> AssistantProfile {
        self.engine
            .register_assistant_profile(
                crate::AssistantLaunchAssets {
                    vision: None,
                    runtime_path: self._dir.path().join("absent-runtime"),
                    model_path: self._dir.path().join("absent-model"),
                    worker_memory_bytes: 512 * 1024 * 1024,
                },
                ArtifactIdentity {
                    artifact_id: ArtifactId::new("fixture").unwrap(),
                    sha256: "a".repeat(64),
                    byte_len: 1,
                },
                ArtifactIdentity {
                    artifact_id: ArtifactId::new("fixture").unwrap(),
                    sha256: "b".repeat(64),
                    byte_len: 1,
                },
                1,
                5000,
            )
            .unwrap()
    }
    fn retained(&self, state: AssistantState) -> (AssistantReport, Arc<AtomicBool>) {
        let report = AssistantReport {
            image: None,
            attempt_id: id!(AttemptId),
            project_id: self.project.clone(),
            session_id: self.session.clone(),
            request_id: id!(RequestId),
            base_revision: RevisionId::new(0),
            engine_epoch: AttemptId::new(self.engine.engine_epoch.clone()).unwrap(),
            profile: self.profile(),
            context_sha256: "c".repeat(64),
            scope: AssistantProposalScope::default(),
            state,
            proposal: None,
            error: if state == AssistantState::Running {
                None
            } else {
                Some(ProtocolError::new(ErrorCode::Unavailable, "interrupted"))
            },
            worker_peak_memory_bytes: None,
        };
        let flag = Arc::new(AtomicBool::new(false));
        let db = self.engine.db.lock().unwrap();
        db.execute(
            "INSERT INTO assistant_attempts VALUES(?1,?2,?3,?4,?5)",
            params![
                report.attempt_id.as_str(),
                self.project.as_str(),
                self.session.as_str(),
                report.request_id.as_str(),
                encode(&report).unwrap()
            ],
        )
        .unwrap();
        self.engine
            .workers
            .lock()
            .unwrap()
            .insert(key(&report.attempt_id), flag.clone());
        (report, flag)
    }
}
#[cfg(target_os = "linux")]
#[test]
fn cancellation_receipt_refusal_preserves_state_and_unsignalled_exact_handle() {
    let f = Fixture::new();
    let (report, flag) = f.retained(AssistantState::Running);
    let request = f.request(
        Command::CancelAssistant {
            attempt_id: report.attempt_id.clone(),
        },
        None,
    );
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_assistant_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'assistant receipt regression');END;").unwrap();
    assert!(f.engine.handle(request.clone()).result.is_err());
    assert!(!flag.load(Ordering::Acquire));
    assert_eq!(
        read(&f.engine.db.lock().unwrap(), &report.attempt_id)
            .unwrap()
            .state,
        AssistantState::Running
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_assistant_receipt")
        .unwrap();
    let ResponseBody::Assistant(cancelled) = f.engine.handle(request.clone()).result.unwrap()
    else {
        panic!("cancel")
    };
    assert_eq!(cancelled.state, AssistantState::Cancelled);
    assert!(flag.load(Ordering::Acquire));
    let unrelated = Arc::new(AtomicBool::new(false));
    f.engine
        .workers
        .lock()
        .unwrap()
        .insert(key(&report.attempt_id), unrelated.clone());
    assert!(f.engine.handle(request).result.is_ok());
    assert!(!unrelated.load(Ordering::Acquire));
    f.engine.workers.lock().unwrap().clear();
}
#[cfg(target_os = "linux")]
#[test]
fn profiles_are_owner_registered_and_request_replay_never_restarts() {
    let f = Fixture::new();
    let profile = f.profile();
    let request = f.request(
        Command::StartAssistant {
            spec: AssistantStart {
                image: None,
                creative_patterns: None,
                edit_focus: vec![],
                target_focus: vec![],
                review_focus: vec![],
                profile: profile.clone(),
                user_request: "set sample stride to 8".into(),
            },
        },
        Some(0),
    );
    let mut forged = request.clone();
    forged.auth_token = Some("wrong".into());
    assert_eq!(
        f.engine.handle(forged).result.unwrap_err().code,
        ErrorCode::Unauthorized
    );
    let mut stale = request.clone();
    stale.expected_revision = Some(RevisionId::new(1));
    assert_eq!(
        f.engine.handle(stale).result.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let mut altered = request.clone();
    if let Command::StartAssistant { spec } = &mut altered.command {
        spec.profile.model.sha256 = "d".repeat(64);
    }
    assert_eq!(
        f.engine.handle(altered).result.unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
    let ResponseBody::Assistant(accepted) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("start")
    };
    assert_eq!(accepted.state, AssistantState::Running);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let current = read(&f.engine.db.lock().unwrap(), &accepted.attempt_id).unwrap();
        if current.state == AssistantState::Failed {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    f.engine
        .withdraw_assistant_profile(&profile.profile_id)
        .unwrap();
    let ResponseBody::Assistant(replayed) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("replay")
    };
    assert_eq!(replayed.attempt_id, accepted.attempt_id);
    let mut conflict = request;
    conflict.command = Command::CancelAssistant {
        attempt_id: accepted.attempt_id,
    };
    assert_eq!(
        f.engine.handle(conflict).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    assert_eq!(
        f.engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM assistant_attempts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(f.engine.workers.lock().unwrap().is_empty());
}
#[cfg(target_os = "linux")]
#[test]
fn restart_interrupts_and_owner_withdrawal_cancels_only_matching_profile() {
    let f = Fixture::new();
    let (first, first_flag) = f.retained(AssistantState::Running);
    let (second, second_flag) = f.retained(AssistantState::Running);
    f.engine
        .withdraw_assistant_profile(&first.profile.profile_id)
        .unwrap();
    assert!(first_flag.load(Ordering::Acquire));
    assert!(!second_flag.load(Ordering::Acquire));
    initialize(&mut f.engine.db.lock().unwrap()).unwrap();
    let db = f.engine.db.lock().unwrap();
    assert_eq!(
        read(&db, &first.attempt_id).unwrap().state,
        AssistantState::Cancelled
    );
    assert_eq!(
        read(&db, &second.attempt_id).unwrap().state,
        AssistantState::Interrupted
    );
    drop(db);
    f.engine.workers.lock().unwrap().clear();
}
#[cfg(target_os = "linux")]
#[test]
fn session_revocation_and_completion_freshness_cannot_publish_late_proposals() {
    let f = Fixture::new();
    let (report, flag) = f.retained(AssistantState::Running);
    let revoke = f.request(
        Command::Revoke {
            session: f.session.clone(),
            project_id: f.project.clone(),
        },
        None,
    );
    f.engine.handle(revoke).result.unwrap();
    assert!(flag.load(Ordering::Acquire));
    let fake = crate::AssistantExecution {
        response: AssistantWorkerResponse {
            vision: None,
            version: PROTOCOL_VERSION,
            attempt_id: report.attempt_id.clone(),
            runtime: report.profile.runtime.clone(),
            model: report.profile.model.clone(),
            proposal: AssistantProposal::NoAction {},
        },
        worker_identity: report.profile.worker.clone(),
        worker_peak_memory_bytes: 1,
    };
    let auth = f.request(
        Command::StartAssistant {
            spec: AssistantStart {
                image: None,
                creative_patterns: None,
                edit_focus: vec![],
                target_focus: vec![],
                review_focus: vec![],
                profile: report.profile.clone(),
                user_request: "anything".into(),
            },
        },
        Some(0),
    );
    f.engine
        .complete_assistant(&auth, &report, Ok(fake))
        .unwrap();
    let current = read(&f.engine.db.lock().unwrap(), &report.attempt_id).unwrap();
    assert_eq!(current.state, AssistantState::Cancelled);
    assert!(current.proposal.is_none());
    assert!(f.engine.workers.lock().unwrap().is_empty());
}
#[test]
fn malformed_assistant_catalog_is_refused_without_schema_mutation() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(&TABLE.replace("UNIQUE(session,request)", "UNIQUE(project,request)"))
        .unwrap();
    let before: i64 = db
        .query_row("PRAGMA schema_version", [], |r| r.get(0))
        .unwrap();
    assert!(validate(&db).is_err());
    assert_eq!(
        db.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        before
    );
}

#[cfg(target_os = "linux")]
fn execution(report: &AssistantReport) -> crate::AssistantExecution {
    crate::AssistantExecution {
        response: AssistantWorkerResponse {
            vision: None,
            version: PROTOCOL_VERSION,
            attempt_id: report.attempt_id.clone(),
            runtime: report.profile.runtime.clone(),
            model: report.profile.model.clone(),
            proposal: AssistantProposal::NoAction {},
        },
        worker_identity: report.profile.worker.clone(),
        worker_peak_memory_bytes: 1,
    }
}
#[cfg(target_os = "linux")]
#[test]
fn completion_commit_refusal_retains_result_and_observation_retries_metadata_only() {
    let f = Fixture::new();
    let (report, _) = f.retained(AssistantState::Running);
    let authorization = f.request(
        Command::StartAssistant {
            spec: AssistantStart {
                image: None,
                creative_patterns: None,
                edit_focus: vec![],
                target_focus: vec![],
                review_focus: vec![],
                profile: report.profile.clone(),
                user_request: "no action".into(),
            },
        },
        Some(0),
    );
    f.engine.assistant_completions.lock().unwrap().insert(
        report.attempt_id.clone(),
        Completion {
            authorization,
            accepted: report.clone(),
            result: Ok(execution(&report)),
        },
    );
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_completion BEFORE UPDATE ON assistant_attempts BEGIN SELECT RAISE(ABORT,'completion regression');END;").unwrap();
    assert_eq!(
        f.engine.flush_assistant_completions().unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(f.engine.assistant_completions.lock().unwrap().len(), 1);
    assert!(f
        .engine
        .workers
        .lock()
        .unwrap()
        .contains_key(&key(&report.attempt_id)));
    assert_eq!(
        read(&f.engine.db.lock().unwrap(), &report.attempt_id)
            .unwrap()
            .state,
        AssistantState::Running
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_completion")
        .unwrap();
    let status = f.request(
        Command::AssistantStatus {
            attempt_id: report.attempt_id.clone(),
        },
        None,
    );
    let ResponseBody::Assistant(current) = f.engine.handle(status).result.unwrap() else {
        panic!("status")
    };
    assert_eq!(current.state, AssistantState::Completed);
    assert!(f.engine.assistant_completions.lock().unwrap().is_empty());
    assert!(f.engine.workers.lock().unwrap().is_empty());
}
#[cfg(target_os = "linux")]
#[test]
fn changed_project_revision_and_execution_identity_refuse_completion() {
    for change in ["revision", "identity"] {
        let f = Fixture::new();
        let (report, _) = f.retained(AssistantState::Running);
        let mut output = execution(&report);
        if change == "revision" {
            f.engine
                .db
                .lock()
                .unwrap()
                .execute(
                    "UPDATE projects SET revision=1 WHERE id=?1",
                    [f.project.as_str()],
                )
                .unwrap();
        } else {
            output.response.model.sha256 = "e".repeat(64);
        }
        let authorization = f.request(
            Command::StartAssistant {
                spec: AssistantStart {
                    image: None,
                    creative_patterns: None,
                    edit_focus: vec![],
                    target_focus: vec![],
                    review_focus: vec![],
                    profile: report.profile.clone(),
                    user_request: "no action".into(),
                },
            },
            Some(0),
        );
        f.engine
            .complete_assistant(&authorization, &report, Ok(output))
            .unwrap();
        let current = read(&f.engine.db.lock().unwrap(), &report.attempt_id).unwrap();
        assert_eq!(current.state, AssistantState::Failed);
        assert!(current.proposal.is_none());
        assert_eq!(
            current.error.unwrap().code,
            if change == "revision" {
                ErrorCode::RevisionConflict
            } else {
                ErrorCode::DependencyMismatch
            }
        );
    }
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "requires current companion and pinned native model/runtime; real authenticated learned proposal, no application effects"]
fn authenticated_pinned_inference_retains_status_and_never_changes_project() {
    use std::io::Read;
    let worker = PathBuf::from(
        std::env::var_os("PULSAR_TEST_ASSISTANT_WORKER").expect("explicit companion"),
    );
    let runtime =
        PathBuf::from(std::env::var_os("PULSAR_ASSISTANT_RUNTIME").expect("pinned runtime"));
    let model = PathBuf::from(std::env::var_os("PULSAR_ASSISTANT_MODEL").expect("pinned model"));
    let identity = |path: &Path| {
        let mut file = std::fs::File::open(path).unwrap();
        let mut h = Sha256::new();
        let mut b = [0u8; 65536];
        loop {
            let n = file.read(&mut b).unwrap();
            if n == 0 {
                break;
            }
            h.update(&b[..n]);
        }
        ArtifactIdentity {
            artifact_id: ArtifactId::new("pinned").unwrap(),
            sha256: format!("{:x}", h.finalize()),
            byte_len: file.metadata().unwrap().len(),
        }
    };
    let ri = identity(&runtime);
    let mi = identity(&model);
    assert_eq!(
        ri.sha256,
        "f332257812cf358954a1701c9fb205e5ac50b4fbdf0e2c09944cc25ea58459ae"
    );
    assert_eq!(
        mi.sha256,
        "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031"
    );
    let f = Fixture::with_worker(worker);
    let before =
        encode(&project_snapshot(&f.engine.db.lock().unwrap(), &f.project).unwrap()).unwrap();
    let profile = f
        .engine
        .register_assistant_profile(
            crate::AssistantLaunchAssets {
                vision: None,
                runtime_path: runtime,
                model_path: model,
                worker_memory_bytes: 2 * 1024 * 1024 * 1024,
            },
            ri,
            mi,
            4,
            45000,
        )
        .unwrap();
    let start = f.request(
        Command::StartAssistant {
            spec: AssistantStart {
                image: None,
                creative_patterns: None,
                edit_focus: vec![],
                target_focus: vec![],
                review_focus: vec![],
                profile: profile.clone(),
                user_request: "Please set generation sample stride to 8.".into(),
            },
        },
        Some(0),
    );
    let ResponseBody::Assistant(accepted) = f.engine.handle(start.clone()).result.unwrap() else {
        panic!("start")
    };
    accepted.validate_for(&start).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(50);
    let completed = loop {
        let status = f.request(
            Command::AssistantStatus {
                attempt_id: accepted.attempt_id.clone(),
            },
            None,
        );
        let ResponseBody::Assistant(report) = f.engine.handle(status.clone()).result.unwrap()
        else {
            panic!("status")
        };
        report.validate_for(&status).unwrap();
        if report.state != AssistantState::Running {
            break report;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    };
    eprintln!(
        "authenticated proposal {:?}; peak {:?}",
        completed.proposal, completed.worker_peak_memory_bytes
    );
    assert_eq!(completed.state, AssistantState::Completed);
    assert_eq!(
        completed.proposal,
        Some(AssistantProposal::SetSampleStride { stride: 8 })
    );
    assert_eq!(
        encode(&project_snapshot(&f.engine.db.lock().unwrap(), &f.project).unwrap()).unwrap(),
        before
    );
    assert!(!f.engine.worker_fence.has_pending_worker_custody().unwrap());
    assert!(f.engine.workers.lock().unwrap().is_empty());
    f.engine
        .withdraw_assistant_profile(&profile.profile_id)
        .unwrap();
    let ResponseBody::Assistant(replay) = f.engine.handle(start).result.unwrap() else {
        panic!("replay")
    };
    assert_eq!(replay.attempt_id, accepted.attempt_id);
    let ResponseBody::AssistantAttempts(reports) = f
        .engine
        .handle(f.request(Command::ListAssistantAttempts, None))
        .result
        .unwrap()
    else {
        panic!("catalog")
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].state, AssistantState::Completed);
}

#[cfg(target_os = "linux")]
#[test]
fn start_and_cancel_require_both_read_and_edit_before_context_or_state_access() {
    for only in [Scope::Read, Scope::Edit] {
        let f = Fixture::new();
        let (report, flag) = f.retained(AssistantState::Running);
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope<>?3",
                params![f.session.as_str(), f.project.as_str(), scope_name(only)],
            )
            .unwrap();
        let start = f.request(
            Command::StartAssistant {
                spec: AssistantStart {
                    image: None,
                    creative_patterns: None,
                    edit_focus: vec![],
                    target_focus: vec![],
                    review_focus: vec![],
                    profile: report.profile.clone(),
                    user_request: "stride 8".into(),
                },
            },
            Some(0),
        );
        assert_eq!(
            f.engine.handle(start).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let cancel = f.request(
            Command::CancelAssistant {
                attempt_id: report.attempt_id.clone(),
            },
            None,
        );
        assert_eq!(
            f.engine.handle(cancel).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        assert!(!flag.load(Ordering::Acquire));
        assert_eq!(
            read(&f.engine.db.lock().unwrap(), &report.attempt_id)
                .unwrap()
                .state,
            AssistantState::Running
        );
        f.engine.workers.lock().unwrap().clear();
    }
}

#[cfg(target_os = "linux")]
fn resolution_reference(f: &Fixture, report: Option<&AssistantReport>) -> AssistantStartReference {
    AssistantStartReference {
        original_request_id: report.map_or_else(
            || RequestId::new("unpublished-original").unwrap(),
            |r| r.request_id.clone(),
        ),
        base_revision: RevisionId::new(0),
        spec: AssistantStart {
            image: None,
            creative_patterns: None,
            edit_focus: vec![],
            target_focus: vec![],
            review_focus: vec![],
            profile: report.map_or_else(|| f.profile(), |r| r.profile.clone()),
            user_request: "no action".into(),
        },
    }
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_resolution_withdrawal_survives_restart_and_conflicts_with_late_start() {
    let f = Fixture::new();
    let reference = resolution_reference(&f, None);
    let resolve = f.request(
        Command::ResolveAssistantStart {
            reference: reference.clone(),
        },
        None,
    );
    let ResponseBody::AssistantStartResolution(value) =
        f.engine.handle(resolve.clone()).result.unwrap()
    else {
        panic!("resolution")
    };
    assert!(matches!(value.outcome, AssistantStartOutcome::Withdrawn));
    assert!(f.engine.workers.lock().unwrap().is_empty());
    let mut late = f.request(
        Command::StartAssistant {
            spec: reference.spec.clone(),
        },
        Some(0),
    );
    late.request_id = reference.original_request_id.clone();
    assert_eq!(
        f.engine.handle(late.clone()).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    assert!(f.engine.handle(resolve.clone()).result.is_ok());
    let config = f.engine.config.clone();
    let session = f.session.clone();
    let token = f.token.clone();
    let project = f.project.clone();
    drop(f.engine);
    let reopened = Engine::open(config).unwrap();
    late.session = Some(session);
    late.auth_token = Some(token);
    late.project = Some(project);
    assert_eq!(
        reopened.handle(late).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    assert!(reopened.handle(resolve).result.is_ok());
    assert!(reopened.workers.lock().unwrap().is_empty());
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_resolution_receipt_failure_rolls_back_withdrawal_and_occupied_namespace_refuses() {
    let f = Fixture::new();
    let reference = resolution_reference(&f, None);
    let resolve = f.request(
        Command::ResolveAssistantStart {
            reference: reference.clone(),
        },
        None,
    );
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_resolution BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'resolution receipt failure');END;").unwrap();
    assert!(f.engine.handle(resolve.clone()).result.is_err());
    let count: i64 = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM request_identities WHERE session=?1 AND request=?2",
            params![f.session.as_str(), reference.original_request_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_resolution")
        .unwrap();
    let mut occupied = f.request(
        Command::CancelAssistant {
            attempt_id: AttemptId::new("absent").unwrap(),
        },
        None,
    );
    occupied.request_id = reference.original_request_id.clone();
    {
        let db = f.engine.db.lock().unwrap();
        claim_request_identity(
            &db,
            f.session.as_str(),
            &occupied,
            &format!(
                "{:x}",
                Sha256::digest(request_fingerprint(&occupied).unwrap())
            ),
        )
        .unwrap();
    }
    assert_eq!(
        f.engine.handle(resolve).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    assert!(f.engine.workers.lock().unwrap().is_empty());
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_resolution_recovers_accepted_exact_invocation_without_cancel_or_worker() {
    let f = Fixture::new();
    let (report, flag) = f.retained(AssistantState::Running);
    let reference = resolution_reference(&f, Some(&report));
    let mut original = f.request(
        Command::StartAssistant {
            spec: reference.spec.clone(),
        },
        Some(0),
    );
    original.request_id = report.request_id.clone();
    {
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        remember(
            &tx,
            f.session.as_str(),
            &original,
            &format!(
                "{:x}",
                Sha256::digest(request_fingerprint(&original).unwrap())
            ),
            &ResponseBody::Assistant(Box::new(report.clone())),
        )
        .unwrap();
        tx.commit().unwrap();
    }
    let resolve = f.request(
        Command::ResolveAssistantStart {
            reference: reference.clone(),
        },
        None,
    );
    let ResponseBody::AssistantStartResolution(value) =
        f.engine.handle(resolve.clone()).result.unwrap()
    else {
        panic!("resolution")
    };
    let AssistantStartOutcome::Accepted { report: accepted } = value.outcome else {
        panic!("accepted")
    };
    assert_eq!(accepted.attempt_id, report.attempt_id);
    assert_eq!(accepted.state, AssistantState::Running);
    assert!(!flag.load(Ordering::Acquire));
    let mut wrong = reference;
    wrong.spec.user_request = "changed invocation".into();
    assert_eq!(
        f.engine
            .handle(f.request(Command::ResolveAssistantStart { reference: wrong }, None))
            .result
            .unwrap_err()
            .code,
        ErrorCode::RequestConflict
    );
    assert!(f.engine.handle(resolve).result.is_ok());
    assert!(!flag.load(Ordering::Acquire));
    assert_eq!(f.engine.workers.lock().unwrap().len(), 1);
    f.engine.workers.lock().unwrap().clear();
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_resolution_requires_read_and_edit_before_reserving_original_id() {
    for only in [Scope::Read, Scope::Edit] {
        let f = Fixture::new();
        let reference = resolution_reference(&f, None);
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope<>?3",
                params![f.session.as_str(), f.project.as_str(), scope_name(only)],
            )
            .unwrap();
        assert_eq!(
            f.engine
                .handle(f.request(Command::ResolveAssistantStart { reference }, None))
                .result
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        let count:i64=f.engine.db.lock().unwrap().query_row("SELECT COUNT(*) FROM request_identities WHERE session=?1 AND request='unpublished-original'",[f.session.as_str()],|r|r.get(0)).unwrap();
        assert_eq!(count, 0);
    }
}

#[cfg(target_os = "linux")]
fn completed_stride(f: &Fixture) -> AssistantReport {
    let (mut report, _) = f.retained(AssistantState::Running);
    f.engine
        .workers
        .lock()
        .unwrap()
        .remove(&key(&report.attempt_id));
    report.state = AssistantState::Completed;
    report.proposal = Some(AssistantProposal::SetSampleStride { stride: 8 });
    report.scope.allowed_sample_strides = (1..=64).collect();
    report.worker_peak_memory_bytes = Some(1);
    let db = f.engine.db.lock().unwrap();
    report.context_sha256 = format!(
        "{:x}",
        Sha256::digest(
            f.engine
                .review_assistant_context(&db, &project_snapshot(&db, &f.project).unwrap(), &[])
                .unwrap()
                .1
                .as_bytes()
        )
    );
    write(&db, &report).unwrap();
    report
}

#[cfg(target_os = "linux")]
fn completed_creative(f: &Fixture) -> AssistantReport {
    let (mut report, _) = f.retained(AssistantState::Running);
    f.engine
        .workers
        .lock()
        .unwrap()
        .remove(&key(&report.attempt_id));
    let creative = CreativePatternScope {
        axes: vec![Axis::Stroke],
        duration: ProjectTime::from_nanos(2_000_000_000),
        sample_hz: 8,
        max_amplitude: 0.1,
    };
    let db = f.engine.db.lock().unwrap();
    let (scope, context) = super::super::assistant_creative::context(
        &project_snapshot(&db, &f.project).unwrap(),
        &creative,
    )
    .unwrap();
    report.scope = scope;
    report.context_sha256 = format!("{:x}", Sha256::digest(context.as_bytes()));
    report.state = AssistantState::Completed;
    report.proposal=Some(AssistantProposal::SynthesizePatterns { patterns:vec![parse_pattern_prompt("pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8").unwrap()] });
    report.worker_peak_memory_bytes = Some(1);
    write(&db, &report).unwrap();
    report
}
#[cfg(target_os = "linux")]
fn creative_generation(f: &Fixture, report: &AssistantReport) -> Request {
    f.request(
        Command::GenerateCreativeWithAssistant {
            approval: AssistantCreativeApproval::from_report(report).unwrap(),
        },
        Some(0),
    )
}
#[cfg(target_os = "linux")]
#[test]
fn creative_approval_derives_exact_patterns_and_refuses_changed_bindings() {
    let f = Fixture::new();
    let report = completed_creative(&f);
    let request = creative_generation(&f, &report);
    let db = f.engine.db.lock().unwrap();
    let derived = f
        .engine
        .approved_assistant_generation(&db, &request)
        .unwrap()
        .unwrap();
    let Command::GenerateInput {
        input: GenerationInput::Patterns { patterns },
    } = derived.command
    else {
        panic!("patterns");
    };
    assert_eq!(patterns[0].parameters().amplitude, 0.1);
    assert_eq!(patterns[0].parameters().sample_hz, 8);
    assert_eq!(derived.request_id, request.request_id);
    for field in 0..2 {
        let mut changed = request.clone();
        let Command::GenerateCreativeWithAssistant { approval } = &mut changed.command else {
            panic!("approval");
        };
        if field == 0 {
            approval.proposal_sha256 = "0".repeat(64)
        } else {
            approval.context_sha256 = "0".repeat(64)
        }
        assert_eq!(
            f.engine
                .approved_assistant_generation(&db, &changed)
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
    }
    let mut other = request.clone();
    other.session = Some(SessionId::new("another-owner").unwrap());
    assert_eq!(
        f.engine
            .approved_assistant_generation(&db, &other)
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    let mut epoch = report.clone();
    epoch.engine_epoch = AttemptId::new("another-epoch").unwrap();
    write(&db, &epoch).unwrap();
    assert_eq!(
        f.engine
            .approved_assistant_generation(&db, &request)
            .unwrap_err()
            .code,
        ErrorCode::Forbidden
    );
    write(&db, &report).unwrap();
    let mut mixed = report.clone();
    mixed.scope.allowed_sample_strides = vec![1];
    write(&db, &mixed).unwrap();
    assert!(f
        .engine
        .approved_assistant_generation(&db, &request)
        .is_err());
    write(&db, &report).unwrap();
    let mut changed_context = report.clone();
    changed_context
        .scope
        .creative_patterns
        .as_mut()
        .unwrap()
        .max_amplitude = 0.2;
    write(&db, &changed_context).unwrap();
    assert_eq!(
        f.engine
            .approved_assistant_generation(&db, &request)
            .unwrap_err()
            .code,
        ErrorCode::DependencyMismatch
    );
    let mut no_action = report;
    no_action.proposal = Some(AssistantProposal::NoAction {});
    write(&db, &no_action).unwrap();
    assert!(AssistantCreativeApproval::from_report(&no_action).is_err());
    assert!(f
        .engine
        .approved_assistant_generation(&db, &request)
        .is_err());
}
#[cfg(target_os = "linux")]
#[test]
fn creative_generation_requires_read_edit_generate_and_does_not_create_jobs_on_refusal() {
    for missing in [Scope::Read, Scope::Edit, Scope::Generate] {
        let f = Fixture::new();
        let report = completed_creative(&f);
        let request = creative_generation(&f, &report);
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
                params![f.session.as_str(), f.project.as_str(), scope_name(missing)],
            )
            .unwrap();
        assert_eq!(
            f.engine.handle(request).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let count: i64 = f
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
#[cfg(target_os = "linux")]
#[test]
fn creative_candidate_receipt_is_atomic_replayed_and_leaves_master_unchanged() {
    let f = Fixture::new();
    let report = completed_creative(&f);
    let request = creative_generation(&f, &report);
    let before = project_snapshot(&f.engine.db.lock().unwrap(), &f.project).unwrap();
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_creative_job BEFORE INSERT ON jobs BEGIN SELECT RAISE(ABORT,'creative job regression');END;").unwrap();
    assert!(f.engine.handle(request.clone()).result.is_err());
    let db = f.engine.db.lock().unwrap();
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM request_identities WHERE session=?1 AND request=?2",
            params![f.session.as_str(), request.request_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    db.execute_batch("DROP TRIGGER refuse_creative_job")
        .unwrap();
    drop(db);
    let ResponseBody::Job(job) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("job");
    };
    let ResponseBody::Job(replayed) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("replay");
    };
    assert_eq!(job.job_id, replayed.job_id);
    let db = f.engine.db.lock().unwrap();
    let raw: String = db
        .query_row(
            "SELECT manifest FROM jobs WHERE id=?1",
            [job.job_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    let manifest: WorkerRequest = serde_json::from_str(&raw).unwrap();
    let WorkerOperation::GenerateInput {
        input: GenerationInput::Patterns { patterns },
    } = manifest.operation
    else {
        panic!("operation");
    };
    assert_eq!(patterns[0].parameters().amplitude, 0.1);
    let after = project_snapshot(&db, &f.project).unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(
        encode(&after.motion).unwrap(),
        encode(&before.motion).unwrap()
    );
    assert_eq!(after.sources.len(), before.sources.len() + 1);
    drop(db);
    let mut changed = request;
    let Command::GenerateCreativeWithAssistant { approval } = &mut changed.command else {
        panic!("approval");
    };
    approval.proposal_sha256 = "0".repeat(64);
    assert!(f.engine.handle(changed).result.is_err());
}
#[cfg(target_os = "linux")]
fn approved_generation(
    f: &Fixture,
    report: &AssistantReport,
    source_version: SourceVersionId,
) -> Request {
    f.request(
        Command::GenerateWithAssistant {
            approval: AssistantGenerationApproval::from_report(report).unwrap(),
            source_version,
            preset: "fast".into(),
            settings: Box::default(),
            model_path: None,
        },
        Some(0),
    )
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_generation_derives_exact_stride_and_refuses_modified_or_stale_approval() {
    let f = Fixture::new();
    let report = completed_stride(&f);
    let request = approved_generation(
        &f,
        &report,
        SourceVersionId::new("explicit-source").unwrap(),
    );
    {
        let db = f.engine.db.lock().unwrap();
        let derived = f
            .engine
            .approved_assistant_generation(&db, &request)
            .unwrap()
            .unwrap();
        let Command::Generate {
            source_version,
            settings: Some(settings),
            ..
        } = derived.command
        else {
            panic!("generation")
        };
        assert_eq!(source_version.as_str(), "explicit-source");
        assert_eq!(settings.source_sample_stride, 8);
        assert_eq!(settings.axis, Axis::Stroke);
        assert_eq!(derived.request_id, request.request_id);
    }
    let mut changed = request.clone();
    if let Command::GenerateWithAssistant { approval, .. } = &mut changed.command {
        approval.proposal_sha256 = "a".repeat(64)
    }
    assert_eq!(
        f.engine.handle(changed).result.unwrap_err().code,
        ErrorCode::DependencyMismatch
    );
    let mut stale = request.clone();
    stale.expected_revision = Some(RevisionId::new(1));
    assert_eq!(
        f.engine.handle(stale).result.unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    let mut report = report;
    report.engine_epoch = id!(AttemptId);
    write(&f.engine.db.lock().unwrap(), &report).unwrap();
    assert_eq!(
        f.engine.handle(request).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_generation_requires_independent_read_edit_and_generate_grants() {
    for missing in [Scope::Read, Scope::Edit, Scope::Generate] {
        let f = Fixture::new();
        let report = completed_stride(&f);
        let request = approved_generation(&f, &report, SourceVersionId::new("absent").unwrap());
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
                params![f.session.as_str(), f.project.as_str(), scope_name(missing)],
            )
            .unwrap();
        assert_eq!(
            f.engine.handle(request).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let count: i64 = f
            .engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_generation_job_and_original_receipt_are_atomic_and_replay_exactly() {
    let f = Fixture::new();
    let file = f._dir.path().join("fixture-media");
    std::fs::write(&file, b"synthetic opaque media fixture").unwrap();
    let ResponseBody::Source { source_version } = f
        .engine
        .handle(f.request(Command::ImportSource { path: file }, None))
        .result
        .unwrap()
    else {
        panic!("source")
    };
    let report = completed_stride(&f);
    let request = approved_generation(&f, &report, source_version);
    let before = project_snapshot(&f.engine.db.lock().unwrap(), &f.project).unwrap();
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_approved_job BEFORE INSERT ON jobs BEGIN SELECT RAISE(ABORT,'approved job regression');END;").unwrap();
    assert!(f.engine.handle(request.clone()).result.is_err());
    {
        let db = f.engine.db.lock().unwrap();
        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM request_identities WHERE session=?1 AND request=?2",
                params![f.session.as_str(), request.request_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        db.execute_batch("DROP TRIGGER refuse_approved_job")
            .unwrap();
    }
    let ResponseBody::Job(job) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("job")
    };
    {
        let db = f.engine.db.lock().unwrap();
        let manifest: String = db
            .query_row(
                "SELECT manifest FROM jobs WHERE id=?1",
                [job.job_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        let manifest: WorkerRequest = serde_json::from_str(&manifest).unwrap();
        let WorkerOperation::Generate { settings, .. } = manifest.operation else {
            panic!("operation")
        };
        assert_eq!(settings.source_sample_stride, 8);
        assert_eq!(
            encode(&project_snapshot(&db, &f.project).unwrap()).unwrap(),
            encode(&before).unwrap()
        );
    }
    let ResponseBody::Job(replayed) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("replay")
    };
    assert_eq!(replayed.job_id, job.job_id);
    let mut changed = request;
    if let Command::GenerateWithAssistant { settings, .. } = &mut changed.command {
        settings.analysis_width = 640
    }
    assert_eq!(
        f.engine.handle(changed).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    // The fixture worker performs no media inference. Observe its retirement.
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while !f.engine.workers.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < end, "fixture worker retirement");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn assistant_recorded_reviews_focus_bounds_and_approval_freshness() {
    let f = Fixture::new();
    let mut report = completed_stride(&f);
    let flags: Vec<_> = (0..12)
        .map(|i| ReviewFlag {
            axis: Some(Axis::Stroke),
            range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(1_000_000)).unwrap(),
            reason: ReviewReason::new(format!("recorded_reason_{i}")).unwrap(),
            evidence: EvidenceKind::Predicted,
            confidence: None,
        })
        .collect();
    let state = lineage::ReviewState {
        flags: flags.clone(),
        decisions: vec![],
        unknown: true,
        conservative: true,
    };
    let db = f.engine.db.lock().unwrap();
    let store = |value: String| {
        db.execute(
            "UPDATE revision_lineage SET review_state=?1 WHERE project=?2 AND revision=0",
            params![value, f.project.as_str()],
        )
        .unwrap()
    };
    store(serde_json::to_string(&state).unwrap());
    let snapshot = project_snapshot(&db, &f.project).unwrap();
    let (scope, context) = f
        .engine
        .review_assistant_context(&db, &snapshot, &[])
        .unwrap();
    assert_eq!(scope.review_ids.len(), 8);
    assert!(context.contains("\"unknown\":true"));
    assert!(context.contains("\"confidence\":null"));
    let outside = flags
        .iter()
        .map(|flag| review_identity(flag).unwrap())
        .find(|id| !scope.review_ids.contains(id))
        .unwrap();
    let (focused, text) = f
        .engine
        .review_assistant_context(&db, &snapshot, std::slice::from_ref(&outside))
        .unwrap();
    assert_eq!(focused.review_ids, vec![outside.clone()]);
    assert!(text.contains(&outside));
    assert!(f
        .engine
        .review_assistant_context(&db, &snapshot, &[outside.clone(), outside])
        .is_err());
    assert_eq!(
        f.engine
            .review_assistant_context(&db, &snapshot, &["f".repeat(64)])
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    report.scope = scope;
    report.context_sha256 = format!("{:x}", Sha256::digest(context.as_bytes()));
    write(&db, &report).unwrap();
    let request = approved_generation(
        &f,
        &report,
        SourceVersionId::new("explicit-source").unwrap(),
    );
    assert!(f
        .engine
        .approved_assistant_generation(&db, &request)
        .unwrap()
        .is_some());
    let mut changed = state;
    changed.unknown = false;
    store(serde_json::to_string(&changed).unwrap());
    assert!(f
        .engine
        .approved_assistant_generation(&db, &request)
        .is_err());
    store("x".repeat(512 * 1024 + 1));
    let error = f
        .engine
        .review_assistant_context(&db, &snapshot, &[])
        .unwrap_err();
    assert!(error.message.contains("bounded capture"));
}

#[cfg(target_os = "linux")]
#[test]
fn assistant_target_catalog_authentication_and_empty_restart() {
    let f = Fixture::new();
    let request = f.request(Command::AssistantTargets, None);
    assert!(!request.command.is_mutating());
    let ResponseBody::AssistantTargets(c) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("catalog");
    };
    assert_eq!(c.project_id, f.project);
    assert_eq!(c.session_id, f.session);
    assert!(c.targets.is_empty());
    let mut bad = request;
    bad.auth_token = Some("wrong".into());
    assert!(f.engine.handle(bad).result.is_err());
}

#[cfg(target_os = "linux")]
fn completed_target(
    f: &Fixture,
    source: SourceVersionId,
) -> (AssistantReport, AssistantObservedTarget) {
    completed_target_view(f, source, None)
}
#[cfg(target_os = "linux")]
fn completed_target_view(
    f: &Fixture,
    source: SourceVersionId,
    view: Option<VideoView>,
) -> (AssistantReport, AssistantObservedTarget) {
    let context = FrameContext {
        source_version: source,
        source_placement: SourcePlacementId::new("primary").unwrap(),
        frame: FrameId::new(4),
        transform: video_view_transform(view.as_ref()).unwrap(),
        seek_generation: 1,
        request_generation: 2,
    };
    let identity = |name: &str, digest: &str| ArtifactIdentity {
        artifact_id: ArtifactId::new(name).unwrap(),
        sha256: digest.repeat(64),
        byte_len: 1,
    };
    let mut target = AssistantObservedTarget {
        target_id: String::new(),
        context: context.clone(),
        frame: identity("pixels", "a"),
        model: identity("detector", "b"),
        observation: DetectionObservation::new(
            context,
            CoordinateBox::new(CoordinateSpace::ProjectionNormalized, 0.2, 0.3, 0.6, 0.7).unwrap(),
            None,
            EvidenceKind::Observed,
            Confidence::new(0.8, None).unwrap(),
        )
        .unwrap(),
        source_frame_index: 4,
        video_view: view,
    };
    target.target_id = target.identity().unwrap();
    target.validate().unwrap();
    f.engine
        .assistant_targets
        .lock()
        .unwrap()
        .replace(
            AssistantTargetCatalog {
                project_id: f.project.clone(),
                session_id: f.session.clone(),
                revision: RevisionId::new(0),
                targets: vec![target.clone()],
            },
            &f.engine.pool,
        )
        .unwrap();
    let mut report = completed_stride(f);
    let db = f.engine.db.lock().unwrap();
    let (scope, context) = f
        .engine
        .target_assistant_context(
            &db,
            &project_snapshot(&db, &f.project).unwrap(),
            &[],
            std::slice::from_ref(&target.target_id),
            &f.session,
        )
        .unwrap();
    report.scope = scope;
    report.context_sha256 = format!("{:x}", Sha256::digest(context.as_bytes()));
    report.proposal = Some(AssistantProposal::SelectTarget {
        target_id: target.target_id.clone(),
    });
    write(&db, &report).unwrap();
    (report, target)
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_target_context_is_current_owned_bounded_and_empty_byte_stable() {
    let f = Fixture::new();
    let (report, target) = completed_target(&f, SourceVersionId::new("source").unwrap());
    let db = f.engine.db.lock().unwrap();
    let snapshot = project_snapshot(&db, &f.project).unwrap();
    let empty = f
        .engine
        .target_assistant_context(&db, &snapshot, &[], &[], &f.session)
        .unwrap();
    assert_eq!(empty.1, generation_context(&snapshot));
    assert!(empty.0.target_ids.is_empty());
    let ids = vec![target.target_id.clone()];
    let focused = f
        .engine
        .target_assistant_context(&db, &snapshot, &[], &ids, &f.session)
        .unwrap();
    assert_eq!(focused.0.target_ids, ids);
    assert!(focused.1.contains(&target.model.sha256));
    assert_eq!(
        format!("{:x}", Sha256::digest(focused.1.as_bytes())),
        report.context_sha256
    );
    assert!(f
        .engine
        .target_assistant_context(&db, &snapshot, &[], &ids, &SessionId::new("other").unwrap())
        .is_err());
    assert!(f
        .engine
        .target_assistant_context(
            &db,
            &snapshot,
            &[],
            &[target.target_id.clone(), target.target_id],
            &f.session
        )
        .is_err());
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_target_application_derives_seed_and_refuses_conflicts_and_changed_preview() {
    let f = Fixture::new();
    let source = SourceVersionId::new("source").unwrap();
    let (report, target) = completed_target(&f, source.clone());
    let request = approved_target_generation(&f, &report, source);
    let db = f.engine.db.lock().unwrap();
    let derived = f
        .engine
        .approved_assistant_generation(&db, &request)
        .unwrap()
        .unwrap();
    let Command::Generate {
        settings: Some(settings),
        ..
    } = derived.command
    else {
        panic!("derived");
    };
    assert_eq!(
        serde_json::to_value(settings.target_seed).unwrap(),
        serde_json::to_value(target.seed()).unwrap()
    );
    assert!(settings.video_view.is_none());
    assert_eq!(
        settings.source_sample_stride,
        GenerationSettings::default().source_sample_stride
    );
    let mut wrong = request.clone();
    if let Command::GenerateWithAssistant { source_version, .. } = &mut wrong.command {
        *source_version = SourceVersionId::new("wrong").unwrap();
    }
    assert!(f.engine.approved_assistant_generation(&db, &wrong).is_err());
    let mut wrong = request.clone();
    if let Command::GenerateWithAssistant { settings, .. } = &mut wrong.command {
        settings.target_seed = Some(target.seed());
    }
    assert!(f.engine.approved_assistant_generation(&db, &wrong).is_err());
    let mut changed = target;
    changed.context.request_generation += 1;
    changed.observation = DetectionObservation::new(
        changed.context.clone(),
        changed.observation.bounds().clone(),
        None,
        EvidenceKind::Observed,
        Confidence::new(0.8, None).unwrap(),
    )
    .unwrap();
    changed.target_id = changed.identity().unwrap();
    f.engine
        .assistant_targets
        .lock()
        .unwrap()
        .replace(
            AssistantTargetCatalog {
                project_id: f.project.clone(),
                session_id: f.session.clone(),
                revision: RevisionId::new(0),
                targets: vec![changed],
            },
            &f.engine.pool,
        )
        .unwrap();
    assert!(f
        .engine
        .approved_assistant_generation(&db, &request)
        .is_err());
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_target_job_captures_seed_and_historical_replay_survives_catalog_expiry() {
    let Fixture {
        engine,
        session,
        token,
        project,
        _dir,
    } = Fixture::new();
    let mut pending = engine;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut owned = loop {
        match Arc::try_unwrap(pending) {
            Ok(engine) => break engine,
            Err(engine) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "owned fixture sweeper retirement"
                );
                pending = engine;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    let runtime_path = _dir.path().join("declared-test-runtime");
    std::fs::write(&runtime_path, b"opaque pinned test runtime").unwrap();
    let (sha256, byte_len) = artifacts::digest_file(&runtime_path, 1024).unwrap();
    owned.tools.as_mut().unwrap().onnx_runtime = Some(ArtifactReference {
        path: runtime_path,
        identity: ArtifactIdentity {
            artifact_id: ArtifactId::new("test-runtime").unwrap(),
            sha256,
            byte_len,
        },
    });
    let f = Fixture {
        engine: Arc::new(owned),
        session,
        token,
        project,
        _dir,
    };
    transfers::start_sweeper(&f.engine).unwrap();
    project_package_exports::start_sweeper(&f.engine).unwrap();
    project_package_imports::start_sweeper(&f.engine).unwrap();
    let file = f._dir.path().join("target-media");
    std::fs::write(&file, b"opaque media fixture").unwrap();
    let ResponseBody::Source { source_version } = f
        .engine
        .handle(f.request(Command::ImportSource { path: file }, None))
        .result
        .unwrap()
    else {
        panic!("source");
    };
    let (report, target) = completed_target(&f, source_version.clone());
    let mut request = approved_target_generation(&f, &report, source_version);
    let model_path = f._dir.path().join("declared-detector");
    std::fs::write(&model_path, b"opaque detector fixture").unwrap();
    if let Command::GenerateWithAssistant {
        model_path: path, ..
    } = &mut request.command
    {
        *path = Some(model_path);
    }
    let ResponseBody::Job(job) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("job");
    };
    let raw: String = f
        .engine
        .db
        .lock()
        .unwrap()
        .query_row(
            "SELECT manifest FROM jobs WHERE id=?1",
            [job.job_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    let manifest: WorkerRequest = serde_json::from_str(&raw).unwrap();
    let WorkerOperation::Generate { settings, .. } = manifest.operation else {
        panic!("generate");
    };
    assert_eq!(
        serde_json::to_value(settings.target_seed).unwrap(),
        serde_json::to_value(target.seed()).unwrap()
    );
    *f.engine.assistant_targets.lock().unwrap() = assistant_targets::Registry::default();
    let ResponseBody::Job(replayed) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("replay");
    };
    assert_eq!(replayed.job_id, job.job_id);
    let mut fresh = request;
    fresh.request_id = id!(RequestId);
    assert!(f.engine.handle(fresh).result.is_err());
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while !f.engine.workers.lock().unwrap().is_empty() {
        assert!(std::time::Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn assistant_target_application_binds_captured_view_and_refuses_competing_view() {
    let f = Fixture::new();
    let source = SourceVersionId::new("view-source").unwrap();
    let view = VideoView {
        layout: StereoLayout::SideBySideLeftRight,
        eye: StereoEye::Left,
        projection: VideoProjection::Flat,
    };
    let (report, _) = completed_target_view(&f, source.clone(), Some(view.clone()));
    let request = approved_target_generation(&f, &report, source);
    let db = f.engine.db.lock().unwrap();
    let derived = f
        .engine
        .approved_assistant_generation(&db, &request)
        .unwrap()
        .unwrap();
    let Command::Generate {
        settings: Some(settings),
        ..
    } = derived.command
    else {
        panic!("generate");
    };
    assert_eq!(settings.video_view, Some(view.clone()));
    let mut conflicting = request;
    if let Command::GenerateWithAssistant { settings, .. } = &mut conflicting.command {
        settings.video_view = Some(VideoView {
            eye: StereoEye::Right,
            ..view
        });
    }
    assert!(f
        .engine
        .approved_assistant_generation(&db, &conflicting)
        .is_err());
}

#[cfg(target_os = "linux")]
fn approved_target_generation(
    f: &Fixture,
    report: &AssistantReport,
    source: SourceVersionId,
) -> Request {
    let mut request = approved_generation(f, report, source);
    if let Command::GenerateWithAssistant { settings, .. } = &mut request.command {
        settings.model_input = Some(ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        });
    }
    request
}

#[cfg(target_os = "linux")]
fn edit_master(f: &Fixture, protected: bool) -> RevisionId {
    let path = f._dir.path().join("edit-base.funscript");
    std::fs::write(&path, br#"{"actions":[{"at":0,"pos":10},{"at":10,"pos":20},{"at":20,"pos":70},{"at":30,"pos":80},{"at":40,"pos":90}]}"#).unwrap();
    let ResponseBody::Candidate(c) = f
        .engine
        .handle(f.request(Command::ImportFunscript { path }, Some(0)))
        .result
        .unwrap()
    else {
        panic!("import");
    };
    let ResponseBody::Project(p) = f
        .engine
        .handle(f.request(
            Command::CommitCandidate {
                candidate_id: c.candidate_id,
            },
            Some(0),
        ))
        .result
        .unwrap()
    else {
        panic!("commit");
    };
    if !protected {
        return p.revision;
    }
    let ResponseBody::Project(p) = f
        .engine
        .handle(f.request(
            Command::SetProtectedRegions {
                regions: vec![ProtectedRegion {
                        axis: Some(Axis::Stroke),
                        range: TimeRange::new(
                            ProjectTime::from_nanos(15_000_000),
                            ProjectTime::from_nanos(25_000_000),
                        )
                        .unwrap(),
                    }],
            },
            Some(p.revision.value()),
        ))
        .result
        .unwrap()
    else {
        panic!("protect");
    };
    p.revision
}
#[cfg(target_os = "linux")]
fn completed_edit(f: &Fixture) -> AssistantReport {
    let mut report = completed_stride(f);
    let db = f.engine.db.lock().unwrap();
    let snapshot = project_snapshot(&db, &f.project).unwrap();
    let recipe = AssistantEditRecipe::catalog(&snapshot)
        .unwrap()
        .into_iter()
        .find(|r| r.operation == ValueEditOperation::Invert && r.axis == Axis::Stroke)
        .unwrap();
    let old = f
        .engine
        .target_assistant_context(&db, &snapshot, &[], &[], &f.session)
        .unwrap();
    let empty = f
        .engine
        .edit_assistant_context(&db, &snapshot, &[], &[], &[], &f.session)
        .unwrap();
    assert_eq!(
        serde_json::to_vec(&old.0).unwrap(),
        serde_json::to_vec(&empty.0).unwrap()
    );
    assert_eq!(old.1, empty.1);
    assert!(f
        .engine
        .edit_assistant_context(
            &db,
            &snapshot,
            &[],
            &[],
            &[recipe.recipe_id.clone(), recipe.recipe_id.clone()],
            &f.session
        )
        .is_err());
    assert!(f
        .engine
        .edit_assistant_context(&db, &snapshot, &[], &[], &["0".repeat(64)], &f.session)
        .is_err());
    let (scope, context) = f
        .engine
        .edit_assistant_context(
            &db,
            &snapshot,
            &[],
            &[],
            std::slice::from_ref(&recipe.recipe_id),
            &f.session,
        )
        .unwrap();
    report.base_revision = snapshot.revision;
    report.scope = scope;
    report.context_sha256 = format!("{:x}", Sha256::digest(context.as_bytes()));
    report.proposal = Some(AssistantProposal::ApplyEditRecipe {
        recipe_id: recipe.recipe_id,
    });
    write(&db, &report).unwrap();
    report
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_edit_creates_synthesized_protected_candidate_and_commit_undo_redo() {
    let f = Fixture::new();
    let revision = edit_master(&f, true);
    let report = completed_edit(&f);
    let request = f.request(
        Command::ApplyAssistantEdit {
            approval: AssistantEditApproval::from_report(&report).unwrap(),
        },
        Some(revision.value()),
    );
    let before = project_state(&f.engine.db.lock().unwrap(), &f.project).unwrap();
    let ResponseBody::Candidate(candidate) = f.engine.handle(request.clone()).result.unwrap()
    else {
        panic!("edit candidate");
    };
    let after = project_state(&f.engine.db.lock().unwrap(), &f.project).unwrap();
    assert_eq!(after.program, before.program);
    assert_eq!(after.revision, revision);
    let edited = candidate_state(
        &f.engine.db.lock().unwrap(),
        &candidate.candidate_id,
        &f.project,
    )
    .unwrap();
    let origin = edit_clock_origins::load(
        &f.engine.db.lock().unwrap(),
        &f.project,
        &candidate.candidate_id,
        &f.engine.pool,
    )
    .unwrap();
    assert_eq!(origin.base_revision, revision);
    assert_eq!(origin.reconciled.program, edited.program);
    drop(origin);
    let old = before.program.track(Axis::Stroke).unwrap().actions();
    let new = edited.program.track(Axis::Stroke).unwrap().actions();
    assert_eq!(&old[1..4], &new[1..4]);
    assert_ne!(old[0].position(), new[0].position());
    assert_eq!(new[0].evidence(), EvidenceKind::Synthesized);
    assert!(candidate
        .review
        .iter()
        .any(|r| r.reason.as_str() == "authored_edit_envelope" && r.confidence.is_none()));
    let ResponseBody::Candidate(replay) = f.engine.handle(request).result.unwrap() else {
        panic!("replay");
    };
    assert_eq!(replay.candidate_id, candidate.candidate_id);
    let ResponseBody::Project(committed) = f
        .engine
        .handle(f.request(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id,
            },
            Some(revision.value()),
        ))
        .result
        .unwrap()
    else {
        panic!("commit");
    };
    let committed_state = project_state(&f.engine.db.lock().unwrap(), &f.project).unwrap();
    assert_eq!(committed_state.program, edited.program);
    let ResponseBody::Project(undone) = f
        .engine
        .handle(f.request(Command::Undo, Some(committed.revision.value())))
        .result
        .unwrap()
    else {
        panic!("undo");
    };
    assert_eq!(
        project_state(&f.engine.db.lock().unwrap(), &f.project)
            .unwrap()
            .program,
        before.program
    );
    f.engine
        .handle(f.request(Command::Redo, Some(undone.revision.value())))
        .result
        .unwrap();
    assert_eq!(
        project_state(&f.engine.db.lock().unwrap(), &f.project)
            .unwrap()
            .program,
        edited.program
    );
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_edit_receipt_publication_is_atomic_and_approval_revision_owner_are_checked() {
    let f = Fixture::new();
    let revision = edit_master(&f, false);
    let report = completed_edit(&f);
    let request = f.request(
        Command::ApplyAssistantEdit {
            approval: AssistantEditApproval::from_report(&report).unwrap(),
        },
        Some(revision.value()),
    );
    let mut altered = request.clone();
    if let Command::ApplyAssistantEdit { approval } = &mut altered.command {
        approval.proposal_sha256 = "a".repeat(64);
    }
    assert!(f.engine.handle(altered).result.is_err());
    let mut stale = request.clone();
    stale.expected_revision = Some(RevisionId::new(0));
    assert!(f.engine.handle(stale).result.is_err());
    let ResponseBody::Paired {
        session: other_owner,
        ..
    } = f
        .engine
        .handle(Request::new(
            id!(RequestId),
            Command::Pair {
                client_name: "foreign assistant owner".into(),
                pairing_token: f.engine.token.clone(),
            },
        ))
        .result
        .unwrap()
    else {
        panic!("pair foreign owner");
    };
    let mut foreign = report.clone();
    foreign.session_id = other_owner;
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE assistant_attempts SET session=?2,report=?3 WHERE id=?1",
            params![
                report.attempt_id.as_str(),
                foreign.session_id.as_str(),
                encoded_report(&foreign).unwrap()
            ],
        )
        .unwrap();
    assert_eq!(
        f.engine.handle(request.clone()).result.unwrap_err().code,
        ErrorCode::Forbidden
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "UPDATE assistant_attempts SET session=?2,report=?3 WHERE id=?1",
            params![
                report.attempt_id.as_str(),
                report.session_id.as_str(),
                encoded_report(&report).unwrap()
            ],
        )
        .unwrap();
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_assistant_edit BEFORE INSERT ON edit_candidate_lineage BEGIN SELECT RAISE(ABORT,'assistant edit regression');END;").unwrap();
    assert!(f.engine.handle(request.clone()).result.is_err());
    let db = f.engine.db.lock().unwrap();
    let n: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM request_identities WHERE session=?1 AND request=?2",
            params![f.session.as_str(), request.request_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    db.execute_batch("DROP TRIGGER refuse_assistant_edit")
        .unwrap();
    drop(db);
    let ResponseBody::Candidate(c) = f.engine.handle(request.clone()).result.unwrap() else {
        panic!("candidate");
    };
    let mut changed = request.clone();
    changed.expected_revision = Some(RevisionId::new(revision.value() + 1));
    assert_eq!(
        f.engine.handle(changed).result.unwrap_err().code,
        ErrorCode::RequestConflict
    );
    let removed = f
        .engine
        .db
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::Read)
            ],
        )
        .unwrap();
    assert_eq!(removed, 1);
    assert!(f.engine.handle(request).result.is_err());
    assert_eq!(
        candidate_state(&f.engine.db.lock().unwrap(), &c.candidate_id, &f.project)
            .unwrap()
            .base_revision,
        revision
    );
}

#[cfg(target_os = "linux")]
#[test]
fn assistant_accepted_edit_replay_survives_reopen_and_changed_revision_but_fresh_execution_refuses()
{
    let f = Fixture::new();
    let revision = edit_master(&f, false);
    let report = completed_edit(&f);
    let request = f.request(
        Command::ApplyAssistantEdit {
            approval: AssistantEditApproval::from_report(&report).unwrap(),
        },
        Some(revision.value()),
    );
    let ResponseBody::Candidate(candidate) = f.engine.handle(request.clone()).result.unwrap()
    else {
        panic!("candidate");
    };
    f.engine
        .handle(f.request(
            Command::CommitCandidate {
                candidate_id: candidate.candidate_id.clone(),
            },
            Some(revision.value()),
        ))
        .result
        .unwrap();
    let config = f.engine.config.clone();
    let before_epoch = f.engine.engine_epoch.clone();
    drop(f.engine);
    let reopened = Engine::open(config).unwrap();
    assert_ne!(reopened.engine_epoch, before_epoch);
    let ResponseBody::Candidate(replayed) = reopened.handle(request.clone()).result.unwrap() else {
        panic!("replay");
    };
    assert_eq!(replayed.candidate_id, candidate.candidate_id);
    assert_eq!(replayed.base_revision, revision);
    let mut fresh = request;
    fresh.request_id = id!(RequestId);
    assert!(reopened.handle(fresh).result.is_err());
    let verified = edit_clock_origins::load(
        &reopened.db.lock().unwrap(),
        &f.project,
        &candidate.candidate_id,
        &reopened.pool,
    )
    .unwrap();
    assert_eq!(verified.base_revision, revision);
}

#[cfg(target_os = "linux")]
fn handoff_job(f: &Fixture) -> (WorkerRequest, Reservation, Arc<AtomicBool>) {
    let path = f._dir.path().join("handoff-input.fixture");
    std::fs::write(&path, b"owned checkpoint input").unwrap();
    let ResponseBody::Source { source_version } = f
        .engine
        .handle(f.request(Command::ImportSource { path }, None))
        .result
        .unwrap()
    else {
        panic!("source");
    };
    let input = GenerationInput::Text {
        prompt: "hold stroke".into(),
    };
    let req = f.request(
        Command::GenerateInput {
            input: input.clone(),
        },
        Some(0),
    );
    let mut db = f.engine.db.lock().unwrap();
    let tx = db.transaction().unwrap();
    let plan = f
        .engine
        .prepare_operation(
            &tx,
            &req,
            &source_version,
            WorkerOperation::GenerateInput { input },
            None,
        )
        .unwrap();
    tx.commit().unwrap();
    drop(db);
    f.engine
        .workers
        .lock()
        .unwrap()
        .insert(plan.0.job_id.to_string(), plan.2.clone());
    plan
}
#[cfg(target_os = "linux")]
fn handoff_authorization(f: &Fixture, report: &AssistantReport) -> Request {
    f.request(
        Command::StartAssistant {
            spec: AssistantStart {
                image: None,
                creative_patterns: None,
                profile: report.profile.clone(),
                user_request: "suggest stride".into(),
                review_focus: vec![],
                target_focus: vec![],
                edit_focus: vec![],
            },
        },
        Some(0),
    )
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_memory_handoff_waits_for_claim_release_preserves_live_handles_and_resumes_same_job() {
    let f = Fixture::new();
    let (manifest, reservation, cancel) = handoff_job(&f);
    let (report, assistant_cancel) = f.retained(AssistantState::Running);
    let authorization = handoff_authorization(&f, &report);
    let live = Arc::new(AtomicBool::new(false));
    f.engine
        .workers
        .lock()
        .unwrap()
        .insert("live-fixture".into(), live.clone());
    let background = f
        .engine
        .pool
        .reserve_memory(f.engine.config.max_ram_bytes - WORKER_MEMORY - 256 * 1024 * 1024)
        .unwrap();
    let generation_cancel = cancel.clone();
    let engine = f.engine.clone();
    let job = manifest.job_id.clone();
    let retired = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !generation_cancel.load(Ordering::Acquire) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Deliberately retain the real accounting claim after the committed signal.
        std::thread::sleep(std::time::Duration::from_millis(40));
        engine.workers.lock().unwrap().remove(job.as_str());
        drop(reservation);
    });
    let admission = f
        .engine
        .acquire_assistant_memory(
            &authorization,
            &report,
            512 * 1024 * 1024,
            2,
            &assistant_cancel,
        )
        .unwrap();
    retired.join().unwrap();
    assert!(cancel.load(Ordering::Acquire));
    assert!(!live.load(Ordering::Acquire));
    assert!(Arc::ptr_eq(
        &live,
        f.engine
            .workers
            .lock()
            .unwrap()
            .get("live-fixture")
            .unwrap()
    ));
    let paused = job_snapshot(&f.engine.db.lock().unwrap(), &manifest.job_id, &f.project).unwrap();
    assert_eq!(paused.state, "interrupted");
    assert_eq!(paused.attempt_id, manifest.attempt_id);
    assert!(!f
        .engine
        .resume_assistant_generation(&authorization, &report)
        .unwrap());
    assert_eq!(
        job_snapshot(&f.engine.db.lock().unwrap(), &manifest.job_id, &f.project)
            .unwrap()
            .attempt_id,
        manifest.attempt_id
    );
    drop(admission);
    assert!(f
        .engine
        .resume_assistant_generation(&authorization, &report)
        .unwrap());
    let after = job_snapshot(&f.engine.db.lock().unwrap(), &manifest.job_id, &f.project).unwrap();
    assert_ne!(after.attempt_id, manifest.attempt_id);
    assert_eq!(after.job_id, manifest.job_id);
    assert!(!live.load(Ordering::Acquire));
    drop(background);
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_handoff_rolls_back_without_signalling_and_requires_generate_authority_and_exact_checkpoint(
) {
    let f = Fixture::new();
    let (manifest, _claim, cancel) = handoff_job(&f);
    let (report, _) = f.retained(AssistantState::Running);
    let authorization = handoff_authorization(&f, &report);
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_handoff BEFORE UPDATE OF state ON attempts WHEN NEW.state='assistant_paused' BEGIN SELECT RAISE(ABORT,'handoff refusal'); END;").unwrap();
    assert!(f
        .engine
        .pause_assistant_generation(&authorization, &report)
        .is_err());
    assert!(!cancel.load(Ordering::Acquire));
    assert_eq!(
        job_snapshot(&f.engine.db.lock().unwrap(), &manifest.job_id, &f.project)
            .unwrap()
            .state,
        "queued"
    );
    f.engine
        .db
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_handoff")
        .unwrap();
    let removed = f
        .engine
        .db
        .lock()
        .unwrap()
        .execute(
            "DELETE FROM grants WHERE session=?1 AND project=?2 AND scope=?3",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::Generate)
            ],
        )
        .unwrap();
    assert_eq!(removed, 1);
    assert!(f
        .engine
        .pause_assistant_generation(&authorization, &report)
        .is_err());
    assert!(!cancel.load(Ordering::Acquire));
    f.engine
        .db
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO grants VALUES(?1,?2,?3)",
            params![
                f.session.as_str(),
                f.project.as_str(),
                scope_name(Scope::Generate)
            ],
        )
        .unwrap();
    let checkpoint = manifest.output_dir.join("initial-checkpoint.json");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&checkpoint, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&checkpoint, b"changed").unwrap();
    assert!(f
        .engine
        .pause_assistant_generation(&authorization, &report)
        .is_err());
    assert!(!cancel.load(Ordering::Acquire));
}

#[cfg(target_os = "linux")]
#[test]
fn retiring_generation_keeps_replacement_attempt_cancellation_handle() {
    let f = Fixture::new();
    let (manifest, claim, cancel) = handoff_job(&f);
    let (report, _) = f.retained(AssistantState::Running);
    let authorization = handoff_authorization(&f, &report);
    assert_eq!(
        f.engine
            .pause_assistant_generation(&authorization, &report)
            .unwrap(),
        1
    );
    assert!(cancel.load(Ordering::Acquire));
    let replacement = Arc::new(AtomicBool::new(false));
    f.engine
        .workers
        .lock()
        .unwrap()
        .insert(manifest.job_id.to_string(), replacement.clone());
    f.engine
        .run_job(manifest.clone(), cancel, claim, None, None, None);
    assert!(Arc::ptr_eq(
        f.engine
            .workers
            .lock()
            .unwrap()
            .get(manifest.job_id.as_str())
            .unwrap(),
        &replacement
    ));
    assert!(!replacement.load(Ordering::Acquire));
    assert_eq!(
        job_snapshot(&f.engine.db.lock().unwrap(), &manifest.job_id, &f.project)
            .unwrap()
            .state,
        "interrupted"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn journal_capture_refuses_damaged_cells_before_restart_or_cancellation_effects() {
    for sql in [
        "UPDATE assistant_attempts SET report=CAST(zeroblob(1048576) AS TEXT)",
        "UPDATE assistant_attempts SET report=zeroblob(32)",
        "UPDATE assistant_attempts SET report='{}'",
        "UPDATE assistant_attempts SET report=json_set(report,'$.attempt_id','different-attempt')",
        "UPDATE assistant_attempts SET request=printf('%0130d',1)",
    ] {
        let f = Fixture::new();
        let (report, flag) = f.retained(AssistantState::Running);
        let mut db = f.engine.db.lock().unwrap();
        db.execute_batch(sql).unwrap();
        let before: (String, String) = db
            .query_row(
                "SELECT typeof(report),hex(report) FROM assistant_attempts WHERE id=?1",
                [report.attempt_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(read(&db, &report.attempt_id).is_err(), "{sql}");
        assert!(validate(&db).is_err(), "{sql}");
        assert!(initialize(&mut db).is_err(), "{sql}");
        if sql.contains("1048576") {
            let Err(error) = Engine::open(f.engine.config.clone()) else {
                panic!("damaged journal reopened");
            };
            assert!(
                error.to_string().contains("typed capture admission"),
                "{error}"
            );
        }
        assert!(
            revoke(&db, &f.session, &f.project, &f.engine.workers).is_err(),
            "{sql}"
        );
        assert!(!flag.load(Ordering::Acquire), "{sql}");
        let after: (String, String) = db
            .query_row(
                "SELECT typeof(report),hex(report) FROM assistant_attempts WHERE id=?1",
                [report.attempt_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(before, after, "{sql}");
    }
}
#[cfg(target_os = "linux")]
#[test]
fn report_publication_checks_serialized_byte_boundary_and_exact_storage_identity() {
    let f = Fixture::new();
    let (mut report, flag) = f.retained(AssistantState::Interrupted);
    report.error = Some(ProtocolError::new(ErrorCode::Unavailable, ""));
    let overhead = encode(&report).unwrap().len();
    report.error.as_mut().unwrap().message = "x".repeat(MAX_REPORT_BYTES as usize - overhead);
    let exact = encoded_report(&report).unwrap();
    assert_eq!(exact.len(), MAX_REPORT_BYTES as usize);
    write(&f.engine.db.lock().unwrap(), &report).unwrap();
    report.error.as_mut().unwrap().message.push('x');
    assert_eq!(
        encoded_report(&report).unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
    assert!(write(&f.engine.db.lock().unwrap(), &report).is_err());
    report.error.as_mut().unwrap().message = "\0".repeat(MAX_REPORT_BYTES as usize / 2);
    assert_eq!(
        encoded_report(&report).unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
    report.error.as_mut().unwrap().message = "small".into();
    report.request_id = id!(RequestId);
    assert!(write(&f.engine.db.lock().unwrap(), &report).is_err());
    let db = f.engine.db.lock().unwrap();
    let stored: String = db
        .query_row(
            "SELECT report FROM assistant_attempts WHERE id=?1",
            [report.attempt_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, exact);
    assert!(!flag.load(Ordering::Acquire));
}
#[cfg(target_os = "linux")]
#[test]
fn retained_catalog_overflow_refuses_restart_and_preserves_all_running_rows() {
    let f = Fixture::new();
    let (report, flag) = f.retained(AssistantState::Running);
    let mut db = f.engine.db.lock().unwrap();
    for _ in 1..=MAX_RETAINED {
        let mut next = report.clone();
        next.attempt_id = id!(AttemptId);
        next.request_id = id!(RequestId);
        db.execute(
            "INSERT INTO assistant_attempts VALUES(?1,?2,?3,?4,?5)",
            params![
                next.attempt_id.as_str(),
                f.project.as_str(),
                f.session.as_str(),
                next.request_id.as_str(),
                encoded_report(&next).unwrap()
            ],
        )
        .unwrap();
    }
    assert!(reports(&db, None, None).is_err());
    assert!(reports(&db, Some((&f.session, &f.project)), None).is_err());
    assert!(initialize(&mut db).is_err());
    let running:i64=db.query_row("SELECT COUNT(*) FROM assistant_attempts WHERE json_extract(report,'$.state')='running'",[],|r|r.get(0)).unwrap();
    assert_eq!(running, MAX_RETAINED + 1);
    assert!(!flag.load(Ordering::Acquire));
}

#[cfg(target_os = "linux")]
#[test]
fn pending_generation_resumes_on_external_capacity_release_without_client_polling() {
    let f = Fixture::new();
    let (manifest, claim, _cancel) = handoff_job(&f);
    let (accepted, _) = f.retained(AssistantState::Running);
    let authorization = handoff_authorization(&f, &accepted);
    let background = f
        .engine
        .pool
        .reserve_memory(f.engine.config.max_ram_bytes - WORKER_MEMORY - 256 * 1024 * 1024)
        .unwrap();
    assert_eq!(
        f.engine
            .pause_assistant_generation(&authorization, &accepted)
            .unwrap(),
        1
    );
    f.engine.assistant_completions.lock().unwrap().insert(
        accepted.attempt_id.clone(),
        Completion {
            authorization,
            accepted: accepted.clone(),
            result: Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "authored assistant outcome, no learned inference",
            )),
        },
    );
    f.engine.schedule_assistant_recovery().unwrap();
    f.engine.schedule_assistant_recovery().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let report = read(&f.engine.db.lock().unwrap(), &accepted.attempt_id).unwrap();
        if report.state == AssistantState::Failed {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // The completion mutex is held across its first resume attempt. Taking it
    // after observing Failed fences that attempt and proves it stayed pending.
    assert!(f
        .engine
        .assistant_completions
        .lock()
        .unwrap()
        .contains_key(&accepted.attempt_id));
    assert_eq!(
        job_snapshot(&f.engine.db.lock().unwrap(), &manifest.job_id, &f.project)
            .unwrap()
            .attempt_id,
        manifest.attempt_id
    );
    f.engine
        .workers
        .lock()
        .unwrap()
        .remove(manifest.job_id.as_str());
    drop(claim);
    loop {
        let job = job_snapshot(&f.engine.db.lock().unwrap(), &manifest.job_id, &f.project).unwrap();
        if job.attempt_id != manifest.attempt_id {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no automatic resume after capacity release"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    loop {
        if !f.engine.assistant_recovery_running.load(Ordering::Acquire) {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(f.engine.assistant_completions.lock().unwrap().is_empty());
    drop(background);
}
#[cfg(target_os = "linux")]
#[test]
fn assistant_recovery_wait_does_not_retain_the_engine_lifetime() {
    let f = Fixture::new();
    let (accepted, _) = f.retained(AssistantState::Running);
    let authorization = handoff_authorization(&f, &accepted);
    f.engine.db.lock().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_recovery BEFORE UPDATE ON assistant_attempts BEGIN SELECT RAISE(ABORT,'held recovery fixture'); END;").unwrap();
    f.engine.assistant_completions.lock().unwrap().insert(
        accepted.attempt_id.clone(),
        Completion {
            authorization,
            accepted,
            result: Err(ProtocolError::new(ErrorCode::Unavailable, "owned fixture")),
        },
    );
    f.engine.schedule_assistant_recovery().unwrap();
    let pool = f.engine.pool.clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while pool.release_watcher_count() == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "recovery watcher never registered"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let weak = Arc::downgrade(&f.engine);
    drop(f.engine);
    while weak.strong_count() != 0 || pool.release_watcher_count() != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "recovery wait retains Engine"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn impossible_assistant_cpu_claim_does_not_pause_generation() {
    let f = Fixture::new();
    let (manifest, reservation, cancel) = handoff_job(&f);
    let (report, assistant_cancel) = f.retained(AssistantState::Running);
    let authorization = handoff_authorization(&f, &report);
    let error = f
        .engine
        .acquire_assistant_memory(
            &authorization,
            &report,
            1,
            f.engine.config.max_cpu_threads + 1,
            &assistant_cancel,
        )
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::ResourceExhausted);
    assert!(!cancel.load(Ordering::Acquire));
    let current = job_snapshot(&f.engine.db.lock().unwrap(), &manifest.job_id, &f.project).unwrap();
    assert_eq!(current.attempt_id, manifest.attempt_id);
    assert_eq!(current.state, "queued");
    assert_eq!(f.engine.pool.reserved_cpu_threads().unwrap(), 2);
    drop(reservation);
    assert_eq!(f.engine.pool.reserved_cpu_threads().unwrap(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn creative_context_diagnostic_fixtures_preserve_declared_scope() {
    let f = Fixture::new();
    let db = f.engine.db.lock().unwrap();
    let mut snapshot = project_snapshot(&db, &f.project).unwrap();
    snapshot.project_id = ProjectId::new("creative-context-fixture").unwrap();
    snapshot.motion.binding = MotionBinding::ProjectRevision {
        project_id: snapshot.project_id.clone(),
        revision: snapshot.revision,
    };
    assert_eq!(snapshot.motion.binding.project_id(), &snapshot.project_id);
    for axes in [vec![Axis::Stroke], Axis::ALL.to_vec()] {
        let creative = CreativePatternScope {
            axes,
            duration: ProjectTime::from_nanos(2_000_000_000),
            sample_hz: 8,
            max_amplitude: 0.1,
        };
        let (scope, context) =
            super::super::assistant_creative::context(&snapshot, &creative).unwrap();
        assert_eq!(scope.creative_patterns, Some(creative));
        assert!(context.len() <= 8192);
        println!(
            "CREATIVE_CONTEXT_FIXTURE {}",
            serde_json::json!({"scope":scope,"system_context":context})
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn creative_approval_refuses_retained_unoffered_parameters_before_jobs_or_replay() {
    for change in [
        "amplitude=0.03",
        "frequency=0.75hz",
        "phase=0.2",
        "offset=0.6",
    ] {
        let f = Fixture::new();
        let mut report = completed_creative(&f);
        let base = "pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 phase=0 sample_hz=8";
        let key = change.split('=').next().unwrap();
        let prompt = base
            .split_whitespace()
            .map(|part| {
                if part.starts_with(&format!("{key}=")) {
                    change
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let patterns = vec![parse_pattern_prompt(&prompt).unwrap()];
        // These proposals satisfy the broader public Core/scope contract.
        // A matching retained report and exact approval must not bypass the
        // narrower choices advertised by the native assistant runtime.
        report
            .scope
            .creative_patterns
            .as_ref()
            .unwrap()
            .validate_proposal(&patterns)
            .unwrap();
        report.proposal = Some(AssistantProposal::SynthesizePatterns { patterns });
        let before;
        {
            let db = f.engine.db.lock().unwrap();
            before = serde_json::to_vec(&project_snapshot(&db, &f.project).unwrap()).unwrap();
            write(&db, &report).unwrap();
        }
        let request = creative_generation(&f, &report);
        for _ in 0..2 {
            let response = f.engine.handle(request.clone());
            assert!(
                response.result.is_err(),
                "retained approval admitted {change}"
            );
            assert_eq!(response.result.unwrap_err().code, ErrorCode::InvalidRequest);
        }
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            serde_json::to_vec(&project_snapshot(&db, &f.project).unwrap()).unwrap(),
            before
        );
        let jobs: i64 = db
            .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(jobs, 0);
        let replay: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM request_identities WHERE session=?1 AND request=?2",
                params![f.session.as_str(), request.request_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(replay, 0);
    }
}
