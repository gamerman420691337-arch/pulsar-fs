//! One-shot reply custody for explicit tests; never compiled into production.
use super::*;
use pulsar_core::{ArtifactId, ProjectId, RequestId, SessionId};
use sha2::{Digest, Sha256};
use std::sync::Condvar;

const MAXIMUM_HOLD: Duration = Duration::from_secs(3);
const SHUTDOWN_POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TestResponseGateTarget {
    BindHandyHspLive,
    SourceQualificationStatus { dossier_id: ArtifactId },
}
#[derive(Clone, Debug)]
pub(crate) struct TestResponseGateScope {
    pub actor: SessionId,
    pub project: ProjectId,
    pub target: TestResponseGateTarget,
    /// None captures the first matching actual handled request. Some also
    /// requires this original identity; no replacement identity is generated.
    pub request_id: Option<RequestId>,
}
impl TestResponseGateScope {
    fn matches(&self, request: &Request) -> bool {
        request.session.as_ref() == Some(&self.actor)
            && request.project.as_ref() == Some(&self.project)
            && self
                .request_id
                .as_ref()
                .is_none_or(|original| original == &request.request_id)
            && match (&self.target, &request.command) {
                (TestResponseGateTarget::BindHandyHspLive, Command::BindHandyHspLive { .. }) => {
                    true
                }
                (
                    TestResponseGateTarget::SourceQualificationStatus {
                        dossier_id: expected,
                    },
                    Command::HandyHspSourceQualificationStatus { dossier_id, .. },
                ) => expected == dossier_id,
                _ => false,
            }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) enum TestResponseGateRelease {
    Explicit,
    HandleDropped,
    EndpointStopping,
    EngineStopping,
    DeadlineReached,
}
#[derive(Clone, Debug, Default, serde::Serialize)]
pub(crate) struct TestResponseGateSnapshot {
    pub request_id: Option<RequestId>,
    /// Actual selected identity, including a later explicit read; never a generated ID.
    pub last_matching_request_id: Option<RequestId>,
    pub source_qualification_response_matched: Option<bool>,
    pub response_sha256: Option<String>,
    pub response_succeeded: Option<bool>,
    /// Static observation of the actual handled Bind response, not authority.
    pub bind_response_classification: Option<&'static str>,
    pub captured_write_attempted: bool,
    pub captured_write_succeeded: Option<bool>,
    pub captures: u8,
    pub matching_handled_responses: u64,
    pub waiting: bool,
    pub release: Option<TestResponseGateRelease>,
}
fn classify_bind_response(response: &Response) -> &'static str {
    match &response.result {
        Err(_) => "outer_error",
        Ok(ResponseBody::HandyHspLive(value)) if value.error.is_some() => "body_error",
        Ok(ResponseBody::HandyHspLive(value)) => {
            if value.origin.is_some()
                && value.source_subject.is_some()
                && value
                    .native
                    .as_ref()
                    .is_some_and(|native| !native.retired && native.error.is_none())
            {
                "admitted_live"
            } else {
                "no_native_admission"
            }
        }
        Ok(_) => "unexpected_body",
    }
}
// Issued only to the first captured connection/frame; a duplicate same-ID
// response receives no token and cannot attribute its write to the original.
pub(super) struct CapturedResponseWriteToken(Arc<()>);
pub(super) struct ResponseGate {
    write_owner: Arc<()>,
    scope: TestResponseGateScope,
    maximum_hold: Duration,
    state: Mutex<TestResponseGateSnapshot>,
    changed: Condvar,
}
pub(crate) struct TestResponseGateHandle(Arc<ResponseGate>);
impl TestResponseGateHandle {
    pub(crate) fn wait_held(&self, timeout: Duration) -> bool {
        let state = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        let (state, _) = self
            .0
            .changed
            .wait_timeout_while(state, timeout, |state| {
                state.request_id.is_none() && state.release.is_none()
            })
            .unwrap_or_else(|p| p.into_inner());
        state.waiting && state.release.is_none()
    }
    pub(crate) fn snapshot(&self) -> TestResponseGateSnapshot {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub(crate) fn wait_written(&self, timeout: Duration) -> Option<bool> {
        let state = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        let (state, _) = self
            .0
            .changed
            .wait_timeout_while(state, timeout, |state| !state.captured_write_attempted)
            .unwrap_or_else(|p| p.into_inner());
        state.captured_write_succeeded
    }
    pub(crate) fn release(&self) {
        self.0.release(TestResponseGateRelease::Explicit);
    }
}
impl Drop for TestResponseGateHandle {
    fn drop(&mut self) {
        self.0.release(TestResponseGateRelease::HandleDropped);
    }
}
impl ResponseGate {
    pub(super) fn new(
        scope: TestResponseGateScope,
        maximum_hold: Duration,
    ) -> Result<(Arc<Self>, TestResponseGateHandle)> {
        anyhow::ensure!(
            !maximum_hold.is_zero() && maximum_hold <= MAXIMUM_HOLD,
            "test response hold must be nonzero and at most the existing three-second RPC limit"
        );
        let gate = Arc::new(Self {
            write_owner: Arc::new(()),
            scope,
            maximum_hold,
            state: Mutex::new(TestResponseGateSnapshot::default()),
            changed: Condvar::new(),
        });
        Ok((gate.clone(), TestResponseGateHandle(gate)))
    }
    pub(super) fn select(&self, request: &Request) -> Option<RequestId> {
        self.scope
            .matches(request)
            .then(|| request.request_id.clone())
    }
    fn release(&self, reason: TestResponseGateRelease) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        self.release_locked(&mut state, reason);
    }
    // Shared with the deterministic mutex-held regression below, so its
    // explicit release executes the same mutation and notification as callers.
    fn release_locked(
        &self,
        state: &mut TestResponseGateSnapshot,
        reason: TestResponseGateRelease,
    ) {
        state.release.get_or_insert(reason);
        self.changed.notify_all();
    }
    pub(super) fn endpoint_stopping(&self) {
        self.release(TestResponseGateRelease::EndpointStopping);
    }
    pub(super) fn after_handle(
        &self,
        engine: &Engine,
        request_id: RequestId,
        response: &Response,
        received_at: Instant,
    ) -> Option<CapturedResponseWriteToken> {
        // Hash only the actual completed response; never retain a request,
        // token, owner proof or reconstructed replacement response here.
        let response_sha256 = serde_json::to_vec(response)
            .ok()
            .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
        let deadline = received_at.checked_add(self.maximum_hold);
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.matching_handled_responses = state.matching_handled_responses.saturating_add(1);
        state.last_matching_request_id = Some(request_id.clone());
        if state.request_id.is_some() || state.release.is_some() {
            return None;
        }
        state.request_id = Some(request_id);
        state.response_sha256 = response_sha256;
        state.response_succeeded = Some(response.result.is_ok());
        state.source_qualification_response_matched = match &self.scope.target {
            TestResponseGateTarget::SourceQualificationStatus { dossier_id } => Some(matches!(
                &response.result,
                Ok(ResponseBody::HandyHspSourceQualification(report))
                    if &report.dossier_id == dossier_id
                        && report.manifest.subject.project_id == self.scope.project
            )),
            TestResponseGateTarget::BindHandyHspLive => None,
        };
        state.bind_response_classification =
            matches!(self.scope.target, TestResponseGateTarget::BindHandyHspLive)
                .then(|| classify_bind_response(response));
        state.captures = 1;
        state.waiting = true;
        self.changed.notify_all();
        while state.release.is_none() {
            if engine.is_shutting_down() {
                state.release = Some(TestResponseGateRelease::EngineStopping);
                break;
            }
            let Some(remaining) = deadline.and_then(|d| d.checked_duration_since(Instant::now()))
            else {
                state.release = Some(TestResponseGateRelease::DeadlineReached);
                break;
            };
            if remaining.is_zero() {
                state.release = Some(TestResponseGateRelease::DeadlineReached);
                break;
            }
            // Condvar releases this gate's private mutex throughout the wait.
            // The original frame deadline is never recomputed or extended.
            let (next, _) = self
                .changed
                .wait_timeout(state, remaining.min(SHUTDOWN_POLL))
                .unwrap_or_else(|p| p.into_inner());
            state = next;
        }
        // A release may have been recorded while this waiter could not
        // reacquire the private mutex. Check the unchanged ingress deadline
        // before completing custody, even when a release ended the loop.
        if deadline.is_none_or(|deadline| Instant::now() >= deadline) {
            state.release = Some(TestResponseGateRelease::DeadlineReached);
        }
        state.waiting = false;
        self.changed.notify_all();
        Some(CapturedResponseWriteToken(self.write_owner.clone()))
    }
    pub(super) fn after_write(&self, token: CapturedResponseWriteToken, succeeded: bool) {
        assert!(
            Arc::ptr_eq(&token.0, &self.write_owner),
            "write token belongs to another gate"
        );
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        assert!(
            !state.captured_write_attempted,
            "captured write token was already consumed"
        );
        state.captured_write_attempted = true;
        state.captured_write_succeeded = Some(succeeded);
        self.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        engine: Arc<Engine>,
        actor: SessionId,
        auth: String,
        project: ProjectId,
        owner_proof: String,
        _directory: tempfile::TempDir,
    }
    fn id() -> RequestId {
        RequestId::new(format!("response-gate-{}", uuid::Uuid::new_v4())).unwrap()
    }
    impl Fixture {
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let worker = directory.path().join("authored-unexecuted-worker");
            fs::write(&worker, b"authored identity only; never executed").unwrap();
            let engine =
                Engine::open(EngineConfig::new(directory.path().join("state"), worker)).unwrap();
            let bootstrap = fs::read_to_string(engine.config.bootstrap_path()).unwrap();
            let ResponseBody::Paired {
                session: actor,
                auth_token: auth,
                ..
            } = engine
                .handle(Request::new(
                    id(),
                    Command::Pair {
                        client_name: "framed response gate fixture".into(),
                        pairing_token: bootstrap.trim().to_owned(),
                    },
                ))
                .result
                .unwrap()
            else {
                panic!("fixture pairing failed");
            };
            let mut create = Request::new(
                id(),
                Command::CreateProject {
                    name: "response gate fixture".into(),
                },
            );
            create.session = Some(actor.clone());
            create.auth_token = Some(auth.clone());
            let ResponseBody::Project(project) = engine.handle(create).result.unwrap() else {
                panic!("fixture project creation failed");
            };
            let owner_proof = fs::read_to_string(engine.config.state_dir.join("handy-owner.token"))
                .unwrap()
                .trim()
                .to_owned();
            Self {
                engine,
                actor,
                auth,
                project: project.project_id,
                owner_proof,
                _directory: directory,
            }
        }
        fn request(&self, command: Command) -> Request {
            let mut request = Request::new(id(), command);
            request.session = Some(self.actor.clone());
            request.auth_token = Some(self.auth.clone());
            request.project = Some(self.project.clone());
            request
        }
        fn status(&self, digit: char) -> Request {
            self.request(Command::HandyHspSourceQualificationStatus {
                dossier_id: ArtifactId::new(digit.to_string().repeat(64)).unwrap(),
                owner_proof: self.owner_proof.clone(),
            })
        }
        fn scope(&self, request: &Request) -> crate::daemon::TestResponseGateScope {
            let Command::HandyHspSourceQualificationStatus { dossier_id, .. } = &request.command
            else {
                panic!("source status scope required");
            };
            crate::daemon::TestResponseGateScope {
                actor: self.actor.clone(),
                project: self.project.clone(),
                target: crate::daemon::TestResponseGateTarget::SourceQualificationStatus {
                    dossier_id: dossier_id.clone(),
                },
                request_id: Some(request.request_id.clone()),
            }
        }
        fn call(&self, request: &Request) -> std::result::Result<Response, TransportError> {
            LocalClient::connect(self.engine.config.endpoint(), MAXIMUM_HOLD)?.call(request)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let result = self.engine.finish_shutdown();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
    // Declared inside scoped thread closures: release happens during unwind
    // before thread::scope joins any client waiting for its original reply.
    struct ReleaseOnDrop(Arc<ResponseGate>);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            self.0.release(TestResponseGateRelease::Explicit);
        }
    }
    fn digest(response: &Response) -> String {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(response).unwrap())
        )
    }

    #[test]
    fn framed_response_gate_holds_one_original_reply_and_keeps_public_stop_available() {
        let fixture = Fixture::new();
        let original = fixture.status('a');
        let (mut endpoints, gate) = TestEndpoints::start_with_response_gate(
            fixture.engine.clone(),
            fixture.scope(&original),
            MAXIMUM_HOLD,
        )
        .unwrap();
        thread::scope(|scope| {
            let _release = ReleaseOnDrop(gate.0.clone());
            let held = scope.spawn(|| fixture.call(&original));
            assert!(gate.wait_held(Duration::from_secs(2)));
            assert!(!held.is_finished());
            let captured: crate::daemon::TestResponseGateSnapshot = gate.snapshot();
            assert_eq!(captured.request_id.as_ref(), Some(&original.request_id));
            assert_eq!(captured.captures, 1);
            assert_eq!(captured.response_succeeded, Some(false));
            assert_eq!(
                captured.last_matching_request_id.as_ref(),
                Some(&original.request_id)
            );
            assert_eq!(captured.source_qualification_response_matched, Some(false));
            assert_eq!(captured.bind_response_classification, None);

            let unrelated = fixture.call(&fixture.status('b')).unwrap();
            assert!(
                unrelated.result.is_err(),
                "absent fixture dossier must stay absent"
            );
            // This is the actual public Stop RPC on its own connection. No
            // internal cancellation shortcut bypasses the request handler.
            let stop = fixture.request(Command::StopHandyHspLive {
                stream_request_id: RequestId::new("absent-original-stream").unwrap(),
                owner_proof: fixture.owner_proof.clone(),
            });
            let stopped = fixture.call(&stop).unwrap();
            assert_eq!(stopped.request_id, stop.request_id);
            assert!(stopped.result.is_err(), "no fixture stream was admitted");
            assert!(gate.snapshot().waiting);

            // Read-only repeat proves the one-shot gate cannot capture again;
            // the hook itself never retries any request or effect.
            let repeated = fixture.call(&original).unwrap();
            assert_eq!(Some(digest(&repeated)), captured.response_sha256);
            assert!(
                !gate.snapshot().captured_write_attempted,
                "duplicate same-ID write cannot credit held original"
            );
            assert_eq!(gate.snapshot().captured_write_succeeded, None);
            assert!(!held.is_finished());
            gate.release();
            let delivered = held.join().unwrap().unwrap();
            assert_eq!(delivered.request_id, original.request_id);
            assert_eq!(Some(digest(&delivered)), captured.response_sha256);
            assert_eq!(gate.wait_written(Duration::from_secs(1)), Some(true));
            let finished = gate.snapshot();
            assert!(finished.captured_write_attempted);
            assert_eq!(finished.captured_write_succeeded, Some(true));
            assert_eq!(finished.captures, 1);
            assert_eq!(finished.matching_handled_responses, 2);
            assert_eq!(
                finished.last_matching_request_id.as_ref(),
                Some(&original.request_id)
            );
            assert_eq!(finished.source_qualification_response_matched, Some(false));
            assert_eq!(
                finished.release,
                Some(crate::daemon::TestResponseGateRelease::Explicit)
            );
            assert!(!finished.waiting);
        });
        endpoints.finish().unwrap();
        assert!(endpoints.0.is_empty());
    }

    #[test]
    fn response_gate_scope_excludes_other_actor_project_identity_dossier_and_command() {
        let fixture = Fixture::new();
        let request = fixture.status('a');
        let selected = fixture.scope(&request);
        assert!(selected.matches(&request));
        let mut other = request.clone();
        other.session = Some(SessionId::new("different-actor").unwrap());
        assert!(!selected.matches(&other));
        let mut other = request.clone();
        other.project = Some(ProjectId::new("different-project").unwrap());
        assert!(!selected.matches(&other));
        let mut other = request.clone();
        other.request_id = id();
        assert!(!selected.matches(&other));
        let mut dynamic_identity = selected.clone();
        dynamic_identity.request_id = None;
        assert!(dynamic_identity.matches(&other));
        let mut other = request.clone();
        other.command = fixture.status('b').command;
        assert!(!selected.matches(&other));
        let mut other = request.clone();
        other.command = Command::CancelHandyHspPreparation {
            preparation_request_id: RequestId::new("original-preparation").unwrap(),
            owner_proof: fixture.owner_proof.clone(),
        };
        assert!(!selected.matches(&other));
        let mut bind_only = selected.clone();
        bind_only.target = crate::daemon::TestResponseGateTarget::BindHandyHspLive;
        assert!(!bind_only.matches(&request));
        assert!(ResponseGate::new(selected.clone(), Duration::ZERO).is_err());
        assert!(ResponseGate::new(selected, MAXIMUM_HOLD + Duration::from_nanos(1)).is_err());
    }

    #[test]
    fn framed_response_gate_expires_without_replacing_or_reissuing_the_reply() {
        let fixture = Fixture::new();
        let original = fixture.status('a');
        let mut dynamic_identity = fixture.scope(&original);
        dynamic_identity.request_id = None;
        let (mut endpoints, gate) = TestEndpoints::start_with_response_gate(
            fixture.engine.clone(),
            dynamic_identity,
            Duration::from_millis(25),
        )
        .unwrap();
        let delivered = fixture.call(&original).unwrap();
        assert_eq!(gate.wait_written(Duration::from_secs(1)), Some(true));
        let completed = gate.snapshot();
        assert_eq!(completed.request_id, Some(original.request_id));
        assert_eq!(completed.captures, 1);
        assert_eq!(completed.matching_handled_responses, 1);
        assert_eq!(completed.response_sha256, Some(digest(&delivered)));
        assert_eq!(completed.response_succeeded, Some(false));
        assert_eq!(completed.bind_response_classification, None);
        assert_eq!(
            completed.release,
            Some(TestResponseGateRelease::DeadlineReached)
        );
        assert!(!completed.waiting);
        endpoints.finish().unwrap();
    }

    fn assert_original_deadline_wins_after_mutex_delayed_release(explicit_before_deadline: bool) {
        let fixture = Fixture::new();
        let original = fixture.status('a');
        let (gate, handle) = ResponseGate::new(fixture.scope(&original), MAXIMUM_HOLD).unwrap();
        // Use the real Engine response and one ingress sample. The gate under
        // test is the actual after_handle implementation, without a socket
        // whose own three-second timeout would obscure this classification.
        let received_at = Instant::now();
        let response = fixture.engine.handle(original.clone());
        let expected_digest = digest(&response);
        let deadline = received_at.checked_add(MAXIMUM_HOLD).unwrap();
        let completed = thread::scope(|scope| {
            let _release = ReleaseOnDrop(gate.clone());
            let held = scope.spawn(|| {
                gate.after_handle(
                    &fixture.engine,
                    original.request_id.clone(),
                    &response,
                    received_at,
                )
            });
            assert!(handle.wait_held(Duration::from_secs(2)));
            let mut state = gate.state.lock().unwrap();
            assert!(state.waiting && state.release.is_none());
            assert!(
                Instant::now() < deadline,
                "private-mutex rendezvous must precede the original ingress deadline"
            );
            if explicit_before_deadline {
                gate.release_locked(&mut state, TestResponseGateRelease::Explicit);
            }
            // Keep the real waiter excluded until the original deadline has
            // elapsed. Sleep only advances real time; mutex ownership fixes
            // the ordering, rather than relying on a racing sleep and release.
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                thread::sleep(remaining);
            }
            assert!(Instant::now() >= deadline);
            assert!(!held.is_finished());
            if !explicit_before_deadline {
                gate.release_locked(&mut state, TestResponseGateRelease::Explicit);
            }
            assert_eq!(state.release, Some(TestResponseGateRelease::Explicit));
            drop(state);
            let captured = held
                .join()
                .unwrap()
                .expect("one original response captured");
            // No socket write occurred in this gate-level regression. Do not
            // attribute a synthetic write acknowledgment to the captured token.
            drop(captured);
            handle.snapshot()
        });
        // Join the actual waiter and stop the Engine before semantic assertions
        // can fail, including on the intentionally failing red version.
        drop(handle);
        drop(gate);
        drop(fixture);
        assert_eq!(completed.request_id, Some(original.request_id.clone()));
        assert_eq!(
            completed.last_matching_request_id,
            Some(original.request_id)
        );
        assert_eq!(completed.captures, 1);
        assert_eq!(completed.matching_handled_responses, 1);
        assert_eq!(completed.response_sha256, Some(expected_digest));
        assert_eq!(completed.response_succeeded, Some(false));
        assert!(!completed.waiting);
        assert!(!completed.captured_write_attempted);
        assert_eq!(completed.captured_write_succeeded, None);
        assert_eq!(
            completed.release,
            Some(TestResponseGateRelease::DeadlineReached),
            "a release observed after the original ingress deadline cannot claim Explicit"
        );
    }

    #[test]
    fn response_gate_late_explicit_release_cannot_bypass_original_deadline() {
        assert_original_deadline_wins_after_mutex_delayed_release(false);
    }

    #[test]
    fn response_gate_early_explicit_with_late_wakeup_cannot_bypass_original_deadline() {
        assert_original_deadline_wins_after_mutex_delayed_release(true);
    }

    #[test]
    fn framed_response_gate_drop_endpoint_finish_and_engine_shutdown_release_workers() {
        for teardown in 0..3 {
            let fixture = Fixture::new();
            let original = fixture.status('a');
            let (mut endpoints, gate) = TestEndpoints::start_with_response_gate(
                fixture.engine.clone(),
                fixture.scope(&original),
                MAXIMUM_HOLD,
            )
            .unwrap();
            let probe = gate.0.clone();
            thread::scope(|scope| {
                let _release = ReleaseOnDrop(probe.clone());
                let held = scope.spawn(|| fixture.call(&original));
                assert!(gate.wait_held(Duration::from_secs(2)));
                match teardown {
                    0 => drop(gate),
                    1 => {
                        assert!(!fixture.engine.is_shutting_down());
                        endpoints.finish().unwrap();
                        assert!(!fixture.engine.is_shutting_down());
                    }
                    2 => {
                        fixture.engine.request_shutdown();
                        endpoints.finish().unwrap();
                    }
                    _ => unreachable!(),
                }
                let result = held.join().unwrap();
                endpoints.finish().unwrap();
                let completed = probe.state.lock().unwrap().clone();
                assert!(completed.captured_write_attempted);
                assert!(completed.captured_write_succeeded.is_some());
                if let Ok(response) = &result {
                    assert_eq!(completed.captured_write_succeeded, Some(true));
                    assert_eq!(Some(digest(response)), completed.response_sha256);
                }
                assert_eq!(completed.captures, 1);
                assert!(!completed.waiting);
                assert_ne!(
                    completed.release,
                    Some(TestResponseGateRelease::DeadlineReached)
                );
                match teardown {
                    0 => {
                        let response = result.unwrap();
                        assert_eq!(Some(digest(&response)), completed.response_sha256);
                        assert_eq!(
                            completed.release,
                            Some(TestResponseGateRelease::HandleDropped)
                        );
                    }
                    1 => assert_eq!(
                        completed.release,
                        Some(TestResponseGateRelease::EndpointStopping)
                    ),
                    2 => assert!(matches!(
                        completed.release,
                        Some(
                            TestResponseGateRelease::EngineStopping
                                | TestResponseGateRelease::EndpointStopping
                        )
                    )),
                    _ => unreachable!(),
                }
            });
            endpoints.finish().unwrap();
            assert!(endpoints.0.is_empty());
        }
    }
    #[test]
    fn bind_response_classification_cannot_credit_actual_error_or_other_success() {
        let fixture = Fixture::new();
        let refused = fixture.engine.handle(fixture.status('a'));
        assert!(refused.result.is_err());
        assert_eq!(classify_bind_response(&refused), "outer_error");
        let mut create = fixture.request(Command::CreateProject {
            name: "actual non-Bind successful response".into(),
        });
        create.project = None;
        let project = fixture.engine.handle(create);
        assert!(project.result.is_ok());
        assert_eq!(classify_bind_response(&project), "unexpected_body");
        // Positive admitted-live classification is asserted by the actual
        // rendered delayed-Bind campaign, never by fabricated acceptance here.
    }
}
