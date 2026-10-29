//! In-memory authored record shapes only. No media, measurement, reviewer,
//! operator acceptance or native execution occurs. Measured/Accepted enum values
//! below exercise hostile/restored storage representations, never a public API.
use super::*;
mod authored {
    include!("inference_equivalence_fixture.rs");
}
use InferenceEquivalenceEvidenceDetails as D;
use InferenceEquivalenceEvidenceKind as K;

fn fixture_bytes(value: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn fixture_hash(bytes: &[u8]) -> String {
    digest(bytes)
}
fn fixture_artifact(label: &str) -> ArtifactIdentity {
    let sha256 = digest(label.as_bytes());
    ArtifactIdentity {
        artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
        sha256,
        byte_len: label.len() as u64,
    }
}

fn seal_authored_shape(
    subject: InferenceEquivalenceSubject,
    mut details: BTreeMap<InferenceEquivalenceEvidenceKind, InferenceEquivalenceEvidenceDetails>,
) -> (
    InferenceEquivalenceManifest,
    BTreeMap<InferenceEquivalenceEvidenceKind, Vec<u8>>,
) {
    use InferenceEquivalenceEvidenceDetails as D;
    use InferenceEquivalenceEvidenceKind as K;
    let subject_sha256 = subject.subject_sha256().unwrap();
    let mut bytes: BTreeMap<K, Vec<u8>> = BTreeMap::new();
    for kind in K::ALL {
        let hashes = |role| fixture_hash(&bytes[&role]);
        let value = details.get_mut(&kind).unwrap();
        match value {
            D::Protocol(v) => {
                v.scope_sha256 = subject.scope_sha256().unwrap();
            }
            D::FromExecution(v) | D::ToExecution(v) => {
                v.scope_sha256 = subject.scope_sha256().unwrap();
            }
            D::QualityComparison(v) => {
                v.corpus_sha256 = hashes(K::Corpus);
                v.protocol_sha256 = hashes(K::Protocol);
                v.from_execution_sha256 = hashes(K::FromExecution);
                v.to_execution_sha256 = hashes(K::ToExecution);
            }
            D::ProviderPlacement(v) => {
                v.from_execution_sha256 = hashes(K::FromExecution);
                v.to_execution_sha256 = hashes(K::ToExecution);
            }
            D::Resources(v) => {
                v.from_execution_sha256 = hashes(K::FromExecution);
                v.to_execution_sha256 = hashes(K::ToExecution);
            }
            D::Review(v) => {
                v.reviewed_evidence = K::ALL
                    .into_iter()
                    .filter(|role| *role != K::Review)
                    .map(|role| InferenceEquivalenceReviewedEvidence {
                        kind: role,
                        sha256: hashes(role),
                    })
                    .collect();
            }
            _ => {}
        }
        bytes.insert(
            kind,
            fixture_bytes(&InferenceEquivalenceEvidenceDocument {
                schema_version: 1,
                origin: subject.origin,
                subject_sha256: subject_sha256.clone(),
                kind,
                details: value.clone(),
            }),
        );
    }
    let evidence = bytes
        .iter()
        .map(|(kind, raw)| {
            let sha256 = fixture_hash(raw);
            InferenceEquivalenceEvidence {
                kind: *kind,
                identity: ArtifactIdentity {
                    artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
                    sha256,
                    byte_len: raw.len() as u64,
                },
            }
        })
        .collect();
    let manifest = InferenceEquivalenceManifest { subject, evidence };
    manifest.validate().unwrap();
    (manifest, bytes)
}

fn database() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON;PRAGMA user_version=20;CREATE TABLE requests(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,response TEXT NOT NULL,PRIMARY KEY(session,request));CREATE TABLE request_identities(session TEXT NOT NULL,request TEXT NOT NULL,fingerprint TEXT NOT NULL,PRIMARY KEY(session,request));").unwrap();
    db
}

struct AuthoredRecord {
    db: Connection,
    report: InferenceEquivalenceReport,
    hash: String,
}
impl AuthoredRecord {
    fn read(&self, now: u64) -> PResult<CurrentAcceptedEquivalence> {
        read_current_accepted(
            &self.db,
            &self.report.dossier_id,
            self.report.revision,
            &self.hash,
            now,
        )
    }
}

/// Deliberately inserts a privately authored accepted-shaped record. It does not
/// call Engine acceptance or represent verified measurement/reviewer approval.
fn authored_record_with(mutate: impl FnOnce(&mut BTreeMap<K, D>)) -> AuthoredRecord {
    authored_record_scoped(|_| {}, mutate)
}
fn authored_record_scoped(
    scope: impl FnOnce(&mut InferenceEquivalenceSubject),
    mutate: impl FnOnce(&mut BTreeMap<K, D>),
) -> AuthoredRecord {
    let (mut manifest, documents) = authored::synthetic_fixture();
    let mut details = authored::fixture_documents(&documents);
    manifest.subject.origin = InferenceEquivalenceOrigin::Measured;
    if let D::Corpus(corpus) = details.get_mut(&K::Corpus).unwrap() {
        corpus.split = InferenceEquivalenceSplit::Heldout;
        corpus.heldout_freeze = Some(fixture_artifact(
            "unit-only freeze declaration, not real evidence",
        ));
        for case in &mut corpus.cases {
            case.independent_annotations = true;
        }
    }
    if let D::Review(review) = details.get_mut(&K::Review).unwrap() {
        review.conclusion = InferenceEquivalenceConclusion::EquivalentWithinScope;
        review.reviewer = "UNIT TEST ONLY - no real supplied review".into();
    }
    mutate(&mut details);
    scope(&mut manifest.subject);
    let (manifest, docs) = seal_authored_shape(manifest.subject, details);
    let mut db = database();
    let tx = db.transaction().unwrap();
    let docs = docs.into_iter().collect::<Vec<_>>();
    let mut report = import_pending(&tx, &manifest, &docs).unwrap();
    report.revision = 2;
    report.state = InferenceEquivalenceState::Accepted;
    let actor = SessionId::new("unit-only-authored-actor").unwrap();
    let request = RequestId::new("unit-only-authored-record").unwrap();
    report.decision = Some(InferenceEquivalenceDecision {
        revision: 2,
        state: InferenceEquivalenceState::Accepted,
        request_id: request.clone(),
        actor_session: actor.clone(),
        operator: Some("UNIT TEST ONLY - no real operator acceptance".into()),
        note: String::new(),
        recorded_at_unix_ms: Some(3000),
        review_identity: Some(
            report
                .manifest
                .evidence_identity(K::Review)
                .unwrap()
                .clone(),
        ),
    });
    write_decision(&tx, &report, 1).unwrap();
    let fingerprint = digest(b"unit-only authored storage identity, not an API authorization");
    tx.execute(
        "INSERT INTO request_identities VALUES(?1,?2,?3)",
        params![actor.as_str(), request.as_str(), fingerprint],
    )
    .unwrap();
    let receipt = ResponseBody::InferenceEquivalenceSaved(Box::new(InferenceEquivalenceReceipt {
        request_id: request,
        input_sha256: None,
        report: report.clone(),
    }));
    tx.execute(
        "INSERT INTO requests VALUES(?1,?2,?3,?4)",
        params![
            actor.as_str(),
            report.decision.as_ref().unwrap().request_id.as_str(),
            fingerprint,
            encode(&receipt).unwrap()
        ],
    )
    .unwrap();
    tx.execute(
        "UPDATE inference_equivalence_schema SET clock_floor_unix_ms=3000",
        [],
    )
    .unwrap();
    tx.commit().unwrap();
    validate(&db).unwrap();
    let hash = digest(bounded_report(&report).unwrap().as_bytes());
    AuthoredRecord { db, report, hash }
}
fn authored_record() -> AuthoredRecord {
    authored_record_with(|_| {})
}

#[test]
fn archival_original_decision_survives_withdrawal_without_restoring_current_acceptance() {
    let mut fixture = authored_record();
    let reference = InferenceFallbackReference {
        dossier_id: fixture.report.dossier_id.clone(),
        expected_revision: 2,
        expected_report_sha256: fixture.hash.clone(),
    };
    let mut remaining = 64 * 1024 * 1024;
    let (original, documents) = archival_decision(&fixture.db, &reference, &mut remaining).unwrap();
    assert_eq!(original.revision, 2);
    assert!(original.report_only);
    assert_eq!(documents.len(), 8);
    let withdrawn_actor = SessionId::new("unit-only-withdrawal").unwrap();
    let request = Request::new(
        RequestId::new("unit-only-withdrawal").unwrap(),
        Command::WithdrawInferenceEquivalence {
            dossier_id: reference.dossier_id.clone(),
            expected_record_revision: 2,
            reason: "unit-only archival test".into(),
            owner_proof: "a".repeat(64),
        },
    )
    .with_session(withdrawn_actor.clone())
    .with_auth_token("unit-only".into());
    let fingerprint = digest(&request_fingerprint(&request).unwrap());
    let tx = fixture.db.transaction().unwrap();
    let intent = begin_withdrawal(
        &tx,
        &reference.dossier_id,
        2,
        &withdrawn_actor,
        &request,
        &fingerprint,
        "unit-only archival test",
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(fixture.read(4000).is_err());
    archival_decision(&fixture.db, &reference, &mut (64 * 1024 * 1024)).unwrap();
    let tx = fixture.db.transaction().unwrap();
    let withdrawn = finish_withdrawal(&tx, &intent, Some(4000)).unwrap();
    let receipt = ResponseBody::InferenceEquivalenceSaved(Box::new(InferenceEquivalenceReceipt {
        request_id: request.request_id.clone(),
        input_sha256: None,
        report: withdrawn,
    }));
    remember(
        &tx,
        withdrawn_actor.as_str(),
        &request,
        &fingerprint,
        &receipt,
    )
    .unwrap();
    tx.commit().unwrap();
    fixture.db.execute_batch("PRAGMA query_only=ON").unwrap();
    let before: i64 = fixture
        .db
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    let (restored, raw) =
        archival_decision(&fixture.db, &reference, &mut (64 * 1024 * 1024)).unwrap();
    assert_eq!(encode(&restored).unwrap(), encode(&original).unwrap());
    assert_eq!(raw, documents);
    assert!(fixture.read(4000).is_err());
    assert!(fixture.read(20000).is_err());
    assert_eq!(
        fixture
            .db
            .query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        before
    );
}

#[test]
fn archival_decision_refuses_changed_reference_evidence_and_exhausted_allocation() {
    let fixture = authored_record();
    let reference = InferenceFallbackReference {
        dossier_id: fixture.report.dossier_id.clone(),
        expected_revision: 2,
        expected_report_sha256: fixture.hash.clone(),
    };
    let mut wrong = reference.clone();
    wrong.expected_report_sha256 = "f".repeat(64);
    assert!(archival_decision(&fixture.db, &wrong, &mut (64 * 1024 * 1024)).is_err());
    wrong = reference.clone();
    wrong.expected_revision = 3;
    assert!(archival_decision(&fixture.db, &wrong, &mut (64 * 1024 * 1024)).is_err());
    assert!(archival_decision(&fixture.db, &reference, &mut 0).is_err());
    fixture
        .db
        .execute(
            "UPDATE inference_equivalence_evidence SET data=x'00' WHERE dossier=?1",
            [reference.dossier_id.as_str()],
        )
        .unwrap();
    assert!(archival_decision(&fixture.db, &reference, &mut (64 * 1024 * 1024)).is_err());
}

fn payload_fixture_record() -> (
    AuthoredRecord,
    super::super::portable_fallback_payload::Payload,
    WorkerRequest,
) {
    let source = InferenceFallbackSourcePolicy {
        schema_version: 1,
        kind: InferenceFallbackSourceKind::DecodedMedia,
        sources: None,
        independent_audio: InferenceFallbackIndependentAudio::Forbidden,
    };
    let view = InferenceFallbackViewPolicy {
        schema_version: 1,
        applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
    };
    let fixture = authored_record_scoped(
        |subject| {
            subject.scope.source_policy = source.identity().unwrap();
            subject.scope.view_policy = view.identity().unwrap();
        },
        |_| {},
    );
    let reference = InferenceFallbackReference {
        dossier_id: fixture.report.dossier_id.clone(),
        expected_revision: 2,
        expected_report_sha256: fixture.hash.clone(),
    };
    let (report, documents) =
        archival_decision(&fixture.db, &reference, &mut (64 * 1024 * 1024)).unwrap();
    let raw = &documents[&report
        .manifest
        .evidence_identity(K::FromExecution)
        .unwrap()
        .sha256];
    let doc: InferenceEquivalenceEvidenceDocument = serde_json::from_slice(raw).unwrap();
    let D::FromExecution(execution) = doc.details else {
        panic!("execution fixture")
    };
    let mut request = execution.cases[0].manifest.clone();
    let selection = InferenceFallbackSelection {
        schema_version: 1,
        equivalents: vec![reference.clone()],
        maximum_fallbacks: 1,
        source_policy: source,
        view_policy: view,
    };
    let identity = selection.identity().unwrap();
    request
        .dependencies
        .push(report.manifest.subject.from_environment.worker.clone());
    request.dependencies.push(identity.clone());
    request.validate().unwrap();
    let environment = report.manifest.subject.from_environment.clone();
    let history = InferenceFallbackHistory {
        schema_version: 1,
        history_only: true,
        project_id: request.project_id.clone(),
        job_id: request.job_id.clone(),
        base_revision: request.base_revision,
        source_version: request.source.source_version.clone(),
        initial_attempt: request.attempt_id.clone(),
        current_attempt: request.attempt_id.clone(),
        selection,
        attempts: vec![ArchivedInferenceFallbackAttempt {
            attempt_id: request.attempt_id.clone(),
            manifest_sha256: digest(encode(&request).unwrap().as_bytes()),
            profile_sha256: digest(
                encode(&report.manifest.subject.from_profile)
                    .unwrap()
                    .as_bytes(),
            ),
            state: "interrupted".into(),
            trace: JobAttemptFallbackTrace {
                schema_version: 1,
                selection: identity,
                kind: JobAttemptFallbackKind::Initial,
                ordinal: 0,
                predecessor: None,
                reference,
                observed_environment_sha256: environment.identity_sha256().unwrap(),
                admitted_at_unix_ms: 4000,
                expires_at_unix_ms: 10000,
                history_only: true,
            },
            observed_environment: environment,
            outcome: None,
        }],
    };
    let payload = super::super::portable_fallback_payload::Payload {
        header: super::super::portable_fallback_payload::Header {
            schema_version: 1,
            history,
            job_state: "interrupted".into(),
            pending: None,
            reports: vec![report],
        },
        documents,
    };
    (fixture, payload, request)
}

fn payload_fixture() -> (
    super::super::portable_fallback_payload::Payload,
    WorkerRequest,
) {
    let (_, payload, request) = payload_fixture_record();
    (payload, request)
}

/// Private authored shapes only, not a measurement or public acceptance.
pub(in crate::authority) fn seed_package_history(
    db: &mut Connection,
    session: &SessionId,
    project: &ProjectId,
    source: SourceArtifact,
    root: &Path,
) -> WorkerRequest {
    use super::super::inference_fallback_store::{
        self as history, FallbackAdmission, FallbackAdmissionKind,
    };
    let (_, mut payload, mut request) = payload_fixture_record();
    request.project_id = project.clone();
    request.job_id = JobId::new(uuid::Uuid::new_v4().to_string()).unwrap();
    request.attempt_id = AttemptId::new(uuid::Uuid::new_v4().to_string()).unwrap();
    request.source = source;
    request.output_dir = root.join("attempts").join(request.attempt_id.as_str());
    let report = &mut payload.header.reports[0];
    let decision = report.decision.as_mut().unwrap();
    decision.actor_session = session.clone();
    decision.request_id = RequestId::new(uuid::Uuid::new_v4().to_string()).unwrap();
    let decision_request = decision.request_id.clone();
    let reference = InferenceFallbackReference {
        dossier_id: report.dossier_id.clone(),
        expected_revision: 2,
        expected_report_sha256: digest(encode(report).unwrap().as_bytes()),
    };
    let mut selection = payload.header.history.selection.clone();
    selection.equivalents = vec![reference.clone()];
    request.dependencies.retain(|d| {
        !d.artifact_id
            .as_str()
            .starts_with(INFERENCE_FALLBACK_SELECTION_ID_PREFIX)
    });
    request.dependencies.push(selection.identity().unwrap());
    request.validate().unwrap();
    let tx = db.transaction().unwrap();
    let docs: Vec<_> = report
        .manifest
        .evidence
        .iter()
        .map(|e| (e.kind, payload.documents[&e.identity.sha256].clone()))
        .collect();
    import_pending(&tx, &report.manifest, &docs).unwrap();
    write_decision(&tx, report, 1).unwrap();
    let fp = digest(b"UNIT TEST ONLY - storage identity, not an acceptance command");
    tx.execute(
        "INSERT INTO request_identities(session,request,fingerprint) VALUES(?1,?2,?3)",
        params![session.as_str(), decision_request.as_str(), fp],
    )
    .unwrap();
    let receipt = ResponseBody::InferenceEquivalenceSaved(Box::new(InferenceEquivalenceReceipt {
        request_id: decision_request.clone(),
        input_sha256: None,
        report: report.clone(),
    }));
    tx.execute(
        "INSERT INTO requests(session,request,fingerprint,response) VALUES(?1,?2,?3,?4)",
        params![
            session.as_str(),
            decision_request.as_str(),
            fp,
            encode(&receipt).unwrap()
        ],
    )
    .unwrap();
    tx.execute(
        "UPDATE inference_equivalence_schema SET clock_floor_unix_ms=MAX(clock_floor_unix_ms,3000)",
        [],
    )
    .unwrap();
    let raw = encode(&request).unwrap();
    tx.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest) VALUES(?1,?2,?3,0,?4,?5,'queued',?6)",params![request.job_id.as_str(),request.attempt_id.as_str(),project.as_str(),session.as_str(),request.source.source_version.as_str(),raw]).unwrap();
    tx.execute(
        "INSERT INTO attempts(id,job,manifest,state) VALUES(?1,?2,?3,'queued')",
        params![request.attempt_id.as_str(), request.job_id.as_str(), raw],
    )
    .unwrap();
    history::declare_initial(
        &tx,
        &request,
        &selection,
        &FallbackAdmission {
            schema_version: 1,
            kind: FallbackAdmissionKind::Initial,
            ordinal: 0,
            predecessor: None,
            reference,
            observed_environment: report.manifest.subject.from_environment.clone(),
            admitted_at_unix_ms: 4000,
            expires_at_unix_ms: 10000,
        },
    )
    .unwrap();
    tx.execute(
        "UPDATE jobs SET state='interrupted' WHERE id=?1",
        [request.job_id.as_str()],
    )
    .unwrap();
    tx.execute(
        "UPDATE attempts SET state='interrupted' WHERE id=?1",
        [request.attempt_id.as_str()],
    )
    .unwrap();
    tx.commit().unwrap();
    request
}

#[test]
fn fallback_payload_capture_owns_original_sql_snapshot_without_filesystem_access() {
    use super::super::inference_fallback_store::{
        self as history, FallbackAdmission, FallbackAdmissionKind,
    };
    let (mut fixture, payload, request) = payload_fixture_record();
    fixture.db.execute_batch("CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT NOT NULL,project TEXT NOT NULL,base_revision INTEGER NOT NULL,session TEXT NOT NULL,source TEXT NOT NULL,state TEXT NOT NULL,manifest TEXT NOT NULL,candidate TEXT,error TEXT,attempts INTEGER NOT NULL DEFAULT 1);CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT NOT NULL REFERENCES jobs(id),manifest TEXT NOT NULL,state TEXT NOT NULL)").unwrap();
    let raw = encode(&request).unwrap();
    fixture.db.execute("INSERT INTO jobs(id,attempt,project,base_revision,session,source,state,manifest,attempts) VALUES(?1,?2,?3,?4,'unit-only-owner',?5,'queued',?6,1)",params![request.job_id.as_str(),request.attempt_id.as_str(),request.project_id.as_str(),request.base_revision.value(),request.source.source_version.as_str(),raw]).unwrap();
    fixture
        .db
        .execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,'queued')",
            params![request.attempt_id.as_str(), request.job_id.as_str(), raw],
        )
        .unwrap();
    let tx = fixture.db.transaction().unwrap();
    history::declare_initial(
        &tx,
        &request,
        &payload.header.history.selection,
        &FallbackAdmission {
            schema_version: 1,
            kind: FallbackAdmissionKind::Initial,
            ordinal: 0,
            predecessor: None,
            reference: payload.header.history.attempts[0].trace.reference.clone(),
            observed_environment: payload.header.history.attempts[0]
                .observed_environment
                .clone(),
            admitted_at_unix_ms: 4000,
            expires_at_unix_ms: 10000,
        },
    )
    .unwrap();
    tx.commit().unwrap();
    fixture.db.execute_batch("UPDATE jobs SET state='interrupted';UPDATE attempts SET state='interrupted';PRAGMA query_only=ON").unwrap();
    let tx = fixture.db.transaction().unwrap();
    let bytes = super::super::portable_fallback_payload::capture(
        &tx,
        &request.project_id,
        &request.job_id,
        64 * 1024 * 1024,
    )
    .unwrap()
    .unwrap();
    assert_eq!(bytes, payload.encode().unwrap());
    assert_eq!(
        super::super::portable_fallback_payload::capture(
            &tx,
            &request.project_id,
            &request.job_id,
            bytes.len() - 1
        )
        .unwrap_err()
        .code,
        ErrorCode::ResourceExhausted
    );
    assert!(super::super::portable_fallback_payload::capture(
        &tx,
        &request.project_id,
        &request.job_id,
        0
    )
    .is_err());
    tx.rollback().unwrap();
    drop(fixture);
    let restored = super::super::portable_fallback_payload::Payload::decode(&bytes).unwrap();
    restored
        .validate_with_manifests(|_| Ok(request.clone()))
        .unwrap();
}

#[test]
fn inert_payload_roundtrip_retains_raw_evidence_and_exact_original_worker() {
    let (payload, request) = payload_fixture();
    let raw = payload.encode().unwrap();
    let restored = super::super::portable_fallback_payload::Payload::decode(&raw).unwrap();
    assert_eq!(restored.encode().unwrap(), raw);
    assert_eq!(restored.documents, payload.documents);
    restored
        .validate_with_manifests(|_| Ok(request.clone()))
        .unwrap();
    let mut changed = request.clone();
    changed.source.path = PathBuf::from("/nonexistent/changed-source");
    assert!(restored
        .validate_with_manifests(|_| Ok(changed.clone()))
        .is_err());
    let mut reads = 0;
    assert!(restored
        .validate_with_manifests(|_| {
            reads += 1;
            Ok(if reads == 1 {
                request.clone()
            } else {
                changed.clone()
            })
        })
        .is_err());
    assert!(restored.header.history.history_only);
    assert!(restored.header.reports[0].report_only);
}

#[test]
fn inert_pending_observation_requires_exact_original_failed_outcome_and_epoch() {
    let (mut payload, request) = payload_fixture();
    let error = JobAttemptDiagnostic::from_error(&ProtocolError::new(
        ErrorCode::Unavailable,
        "UNIT TEST ONLY - authored provider-absence shape, no native observation",
    ));
    let entry = &mut payload.header.history.attempts[0];
    entry.state = "failed".into();
    let epoch = AttemptId::new("unit-only-original-engine").unwrap();
    let outcome = JobAttemptOutcome {
        schema_version: 1,
        manifest_sha256: entry.manifest_sha256.clone(),
        engine_epoch: epoch.clone(),
        validation_error: Some(error.clone()),
        worker_retirement: Some(WorkerRetirementObservation {
            worker_error: Some(error),
            provider_unavailable: Some(InferenceProviderUnavailable {
                schema_version: 1,
                provider: payload.header.reports[0]
                    .manifest
                    .subject
                    .from_profile
                    .provider,
                profile_sha256: entry.profile_sha256.clone(),
                check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
            }),
        }),
    };
    outcome.validate_for(&request).unwrap();
    entry.outcome = Some(outcome.clone());
    payload.header.job_state = "fallback_pending".into();
    payload.header.pending = Some(
        super::super::portable_fallback_payload::PendingObservation {
            predecessor: request.attempt_id.clone(),
            engine_epoch: epoch,
            outcome_sha256: digest(encode(&outcome).unwrap().as_bytes()),
        },
    );
    payload
        .validate_with_manifests(|_| Ok(request.clone()))
        .unwrap();
    let raw = payload.encode().unwrap();
    let restored = super::super::portable_fallback_payload::Payload::decode(&raw).unwrap();
    assert!(restored.header.history.history_only);
    assert_eq!(restored.header.job_state, "fallback_pending");
    for mutation in 0..4 {
        let mut header = payload.header.clone();
        match mutation {
            0 => header.pending = None,
            1 => {
                header.pending.as_mut().unwrap().engine_epoch =
                    AttemptId::new("replacement").unwrap()
            }
            2 => header.pending.as_mut().unwrap().outcome_sha256 = "f".repeat(64),
            _ => header.job_state = "queued".into(),
        }
        assert!(super::super::portable_fallback_payload::Payload {
            header,
            documents: payload.documents.clone()
        }
        .validate()
        .is_err());
    }
}

#[test]
fn inert_payload_refuses_truncation_trailing_mutated_and_noncanonical_frames() {
    let (payload, _) = payload_fixture();
    let raw = payload.encode().unwrap();
    for length in [0, 7, 11, 12, raw.len() - 1] {
        assert!(super::super::portable_fallback_payload::Payload::decode(&raw[..length]).is_err());
    }
    let mut bad = raw.clone();
    bad.push(0);
    assert!(super::super::portable_fallback_payload::Payload::decode(&bad).is_err());
    let mut bad = raw.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(super::super::portable_fallback_payload::Payload::decode(&bad).is_err());
    let mut bad = raw.clone();
    bad[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(super::super::portable_fallback_payload::Payload::decode(&bad).is_err());
    let mut unknown = serde_json::to_value(&payload.header).unwrap();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("execution_authority".into(), serde_json::json!(true));
    let header = serde_json::to_vec(&unknown).unwrap();
    let mut bad = b"PULSFH1\0".to_vec();
    bad.extend_from_slice(&(header.len() as u32).to_le_bytes());
    bad.extend_from_slice(&header);
    for data in payload.documents.values() {
        bad.extend_from_slice(data);
    }
    assert!(super::super::portable_fallback_payload::Payload::decode(&bad).is_err());
}

#[test]
fn fallback_payload_node_requires_exact_job_attempt_state_and_object_membership() {
    let (payload, request) = payload_fixture();
    let raw = payload.encode().unwrap();
    let worker = encode(&request).unwrap().into_bytes();
    let reference = PackageObjectRef {
        sha256: digest(&raw),
        byte_len: raw.len() as u64,
    };
    let worker_ref = PackageObjectRef {
        sha256: digest(&worker),
        byte_len: worker.len() as u64,
    };
    let motion = ProgramDescriptor {
        artifact_id: ArtifactId::new("unit-only-placeholder").unwrap(),
        sha256: "a".repeat(64),
        byte_len: 1,
        codec: MotionCodec::MotionProgramJsonV1,
        axes: vec![],
    };
    // Node correspondence only; container and ordinary project closure have
    // separate validators. No model, source path or executable is opened.
    let manifest:PortableProjectManifest=serde_json::from_value(serde_json::json!({
        "format_version":16,"origin_project_id":request.project_id,"captured_revision":request.base_revision,"captured_event_cursor":0,
        "completeness":PackageCompleteness::default(),"counts":PackageRecordCounts::default(),
        "head":{"name":"unit-only node","revision":request.base_revision,"history_cursor":0,"motion":motion,"protected":[]},
        "revisions":[],"edit_states":[],"revision_lineage":[],"candidates":[],"authored_lineage":[],"legacy_cells":[],"export_receipts":[],"imported_origins":[],"placements":null,"retiming_receipts":[],
        "sources":[{"source_version":request.source.source_version,"kind":SourceKind::Media,"label":"unit-only","identity":request.source.identity,"pinned":false,"evicted":false}],
        "generated_origins":[{"job_id":request.job_id,"attempt_id":request.attempt_id,"base_revision":request.base_revision,"source_version":request.source.source_version,"manifest":worker_ref,"receipt":null,"program":null,"state":"interrupted","fallback_history":reference}],
        "objects":[{"sha256":reference.sha256,"byte_len":reference.byte_len,"roles":[PackageObjectRole::InferenceFallbackHistory]},{"sha256":worker_ref.sha256,"byte_len":worker_ref.byte_len,"roles":[PackageObjectRole::WorkerManifest]}]
    })).unwrap();
    let read = |r: &PackageObjectRef| {
        if *r == reference {
            Ok(raw.clone())
        } else if *r == worker_ref {
            Ok(worker.clone())
        } else {
            Err(ProtocolError::invalid("unresolved unit-only object"))
        }
    };
    let verified = super::super::portable_fallback_payload::validate_node(&manifest, read).unwrap();
    assert!(verified.contains(&(worker_ref.sha256.clone(), worker_ref.byte_len)));
    let mut original = manifest.clone();
    original.candidates.push(PackageCandidate {
        rebased_from: None,
        project_interval: None,
        candidate_id: CandidateId::new("unit-only-candidate").unwrap(),
        base_revision: request.base_revision,
        motion: original.head.motion.clone(),
        job_origin: Some(request.job_id.clone()),
        lineage: vec![payload.header.history.selection.identity().unwrap()],
        committed_revision: None,
        review: vec![],
    });
    let mut imported = original.clone();
    imported.origin_project_id = ProjectId::new("unit-only-clone").unwrap();
    imported.generated_origins.clear();
    imported.candidates[0].job_origin = None;
    let mut graph = super::super::portable_fallback_payload::GraphIndex::default();
    graph.observe_node(&imported, read).unwrap();
    assert!(graph.finish().is_err());
    graph.observe_node(&original, read).unwrap();
    graph.finish().unwrap();
    graph.check_payload(&reference).unwrap();
    graph.check_worker(&worker_ref, &request).unwrap();
    let mut foreign = request.clone();
    foreign.attempt_id = AttemptId::new("foreign").unwrap();
    let foreign_raw = encode(&foreign).unwrap();
    assert!(graph
        .check_worker(
            &PackageObjectRef {
                sha256: digest(foreign_raw.as_bytes()),
                byte_len: foreign_raw.len() as u64
            },
            &foreign
        )
        .is_err());
    assert!(graph.check_payload(&worker_ref).is_err());
    let mut wrong = original.clone();
    wrong.candidates[0].lineage.clear();
    assert!(
        super::super::portable_fallback_payload::GraphIndex::default()
            .observe_node(&wrong, read)
            .is_err()
    );
    for changed in 0..7 {
        let mut bad = manifest.clone();
        match changed {
            0 => bad.generated_origins[0].fallback_history = None,
            1 => bad.generated_origins[0].state = "completed".into(),
            2 => bad.origin_project_id = ProjectId::new("foreign").unwrap(),
            3 => bad.generated_origins[0].job_id = JobId::new("foreign").unwrap(),
            4 => bad.generated_origins[0].attempt_id = AttemptId::new("foreign").unwrap(),
            5 => bad.objects[0].roles = vec![PackageObjectRole::DependencyEvidence],
            _ => bad.generated_origins.push(bad.generated_origins[0].clone()),
        }
        assert!(
            super::super::portable_fallback_payload::validate_node(&bad, read).is_err(),
            "mutation {changed}"
        );
    }
    let mut changed = worker.clone();
    changed.push(b' ');
    assert!(
        super::super::portable_fallback_payload::validate_node(
            &manifest,
            |r| if *r == worker_ref {
                Ok(changed.clone())
            } else {
                read(r)
            }
        )
        .is_err()
    );
}

#[test]
fn archival_decision_rejects_noncanonical_original_receipt() {
    let fixture = authored_record();
    let reference = InferenceFallbackReference {
        dossier_id: fixture.report.dossier_id.clone(),
        expected_revision: 2,
        expected_report_sha256: fixture.hash.clone(),
    };
    let raw: String = fixture
        .db
        .query_row(
            "SELECT response FROM requests WHERE session='unit-only-authored-actor'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    fixture
        .db
        .execute(
            "UPDATE requests SET response=?1 WHERE session='unit-only-authored-actor'",
            [format!("{raw}\n")],
        )
        .unwrap();
    assert!(archival_decision(&fixture.db, &reference, &mut (64 * 1024 * 1024)).is_err());
}

#[test]
fn authored_storage_shape_reads_exact_current_report_without_mutation_or_authority() {
    let fixture = authored_record();
    fixture.db.execute_batch("PRAGMA query_only=ON").unwrap();
    let changes: i64 = fixture
        .db
        .query_row("SELECT total_changes()", [], |row| row.get(0))
        .unwrap();
    let observed = fixture.read(4000).unwrap();
    assert_eq!(observed.report().dossier_id, fixture.report.dossier_id);
    assert_eq!(
        observed.assessment().manifest_id(),
        &fixture.report.dossier_id
    );
    assert!(observed.assessment().declared_criteria_satisfied());
    assert_eq!(observed.report_sha256(), fixture.hash);
    assert_eq!(observed.checked_at_unix_ms(), 4000);
    assert!(observed.report().report_only);
    assert_eq!(format!("{observed:?}"), "CurrentAcceptedEquivalence");
    assert_eq!(clock_floor(&fixture.db).unwrap(), Some(3000));
    assert_eq!(
        fixture
            .db
            .query_row("SELECT total_changes()", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        changes
    );
}

#[test]
fn exact_revision_and_canonical_report_hash_are_required() {
    let fixture = authored_record();
    assert_eq!(
        read_current_accepted(
            &fixture.db,
            &fixture.report.dossier_id,
            3,
            &fixture.hash,
            4000
        )
        .unwrap_err()
        .code,
        ErrorCode::RevisionConflict
    );
    assert_eq!(
        read_current_accepted(
            &fixture.db,
            &fixture.report.dossier_id,
            2,
            &"f".repeat(64),
            4000
        )
        .unwrap_err()
        .code,
        ErrorCode::DependencyMismatch
    );
    assert!(read_current_accepted(
        &fixture.db,
        &fixture.report.dossier_id,
        0,
        &fixture.hash,
        4000
    )
    .is_err());
    assert!(read_current_accepted(
        &fixture.db,
        &fixture.report.dossier_id,
        2,
        "not-a-hash",
        4000
    )
    .is_err());
    fixture
        .db
        .execute(
            "UPDATE inference_equivalence_records SET report=' '||report",
            [],
        )
        .unwrap();
    assert!(fixture.read(4000).is_err());
}

#[test]
fn expired_unavailable_and_backwards_clocks_refuse_without_advancing_floor() {
    let mut fixture = authored_record();
    for now in [0, 2999, 10_000, i64::MAX as u64 + 1] {
        assert!(fixture.read(now).is_err());
        let tx = fixture.db.transaction().unwrap();
        assert!(
            resolve_current_accepted(&tx, &fixture.report.dossier_id, 2, &fixture.hash, now)
                .is_err()
        );
        tx.commit().unwrap();
        assert_eq!(clock_floor(&fixture.db).unwrap(), Some(3000));
    }
    fixture.read(9999).unwrap();
}

#[test]
fn execution_floor_commits_and_rolls_back_with_actual_admission_transaction() {
    let mut fixture = authored_record();
    {
        let tx = fixture.db.transaction().unwrap();
        resolve_current_accepted(&tx, &fixture.report.dossier_id, 2, &fixture.hash, 5000).unwrap();
        assert_eq!(clock_floor(&tx).unwrap(), Some(5000));
    }
    assert_eq!(clock_floor(&fixture.db).unwrap(), Some(3000));
    {
        let tx = fixture.db.transaction().unwrap();
        resolve_current_accepted(&tx, &fixture.report.dossier_id, 2, &fixture.hash, 5000).unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(clock_floor(&fixture.db).unwrap(), Some(5000));
    assert!(fixture.read(4999).is_err());
    fixture.read(5000).unwrap();
}

#[test]
fn execution_floor_preserves_nested_cpu_and_equivalence_reader_headers() {
    let mut fixture = authored_record();
    {
        let tx = fixture.db.transaction().unwrap();
        crate::cpu_runtime_reader::arm(&tx).unwrap();
        tx.commit().unwrap();
    }
    let outer: i64 = fixture
        .db
        .query_row("PRAGMA main.user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(outer, 36);
    assert_eq!(crate::cpu_runtime_reader::version(&fixture.db).unwrap(), 35);
    let inner = crate::inference_equivalence_reader::version(&fixture.db).unwrap();
    let tx = fixture.db.transaction().unwrap();
    resolve_current_accepted(&tx, &fixture.report.dossier_id, 2, &fixture.hash, 5000).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        fixture
            .db
            .query_row("PRAGMA main.user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        36
    );
    assert_eq!(crate::cpu_runtime_reader::version(&fixture.db).unwrap(), 35);
    assert_eq!(
        crate::inference_equivalence_reader::version(&fixture.db).unwrap(),
        inner
    );
}

#[test]
fn changed_evidence_receipt_identity_or_removed_history_refuses() {
    for sql in [
        "UPDATE inference_equivalence_evidence SET data=CAST(data||' ' AS BLOB) WHERE rowid=(SELECT min(rowid) FROM inference_equivalence_evidence)",
        "UPDATE requests SET response=' '||response",
        "UPDATE requests SET fingerprint=printf('%064d',0)",
        "UPDATE inference_equivalence_decisions SET report_sha256=printf('%064d',0)",
        "UPDATE inference_equivalence_schema SET clock_floor_unix_ms=2999",
        "DELETE FROM requests",
    ] {
        let fixture=authored_record();
        fixture.db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        fixture.db.execute_batch(sql).unwrap();
        assert!(fixture.read(4000).is_err(),"accepted-shaped corruption must refuse");
    }
}

#[test]
fn revalidated_declared_criteria_and_supplied_review_must_support_acceptance() {
    for mutation in 0..4 {
        let fixture = authored_record_with(|details| match mutation {
            0 => {
                if let D::Review(review) = details.get_mut(&K::Review).unwrap() {
                    review.conclusion = InferenceEquivalenceConclusion::Inconclusive;
                }
            }
            1 => {
                if let D::Corpus(corpus) = details.get_mut(&K::Corpus).unwrap() {
                    corpus.split = InferenceEquivalenceSplit::Development;
                    corpus.heldout_freeze = None;
                }
            }
            2 => {
                if let D::QualityComparison(quality) =
                    details.get_mut(&K::QualityComparison).unwrap()
                {
                    quality.cases[0].values[0].to_value = 0.99;
                }
            }
            _ => {
                if let D::Review(review) = details.get_mut(&K::Review).unwrap() {
                    review.reviewed_at_unix_ms = 3500;
                }
            }
        });
        // Complete hashes and receipts cannot turn these authored declarations
        // into criteria-ready evidence; even historical review timing is checked.
        assert!(fixture.read(4000).is_err());
    }
}

#[test]
fn synthetic_pending_records_never_resolve_as_current_acceptance() {
    let mut db = database();
    let (manifest, docs) = authored::synthetic_fixture();
    let tx = db.transaction().unwrap();
    let report = import_pending(&tx, &manifest, &docs.into_iter().collect::<Vec<_>>()).unwrap();
    tx.commit().unwrap();
    let hash = digest(bounded_report(&report).unwrap().as_bytes());
    assert!(read_current_accepted(&db, &report.dossier_id, report.revision, &hash, 4000).is_err());
    assert_eq!(clock_floor(&db).unwrap(), None);
}

#[test]
fn pending_withdrawal_and_completed_withdrawal_both_fence_current_resolution() {
    let mut fixture = authored_record();
    let actor = SessionId::new("unit-withdrawal-actor").unwrap();
    let request = Request::new(
        RequestId::new("unit-withdrawal-request").unwrap(),
        Command::WithdrawInferenceEquivalence {
            dossier_id: fixture.report.dossier_id.clone(),
            expected_record_revision: 2,
            reason: "unit-only withdrawal of authored storage shape".into(),
            owner_proof: "a".repeat(64),
        },
    )
    .with_session(actor.clone())
    .with_auth_token("unit-only-token".into());
    let fingerprint = digest(&request_fingerprint(&request).unwrap());
    let intent;
    {
        let tx = fixture.db.transaction().unwrap();
        intent = begin_withdrawal(
            &tx,
            &fixture.report.dossier_id,
            2,
            &actor,
            &request,
            &fingerprint,
            "unit-only withdrawal of authored storage shape",
        )
        .unwrap();
        tx.commit().unwrap();
    }
    assert!(fixture
        .read(4000)
        .unwrap_err()
        .message
        .contains("withdrawal is pending"));
    let withdrawn;
    {
        let tx = fixture.db.transaction().unwrap();
        withdrawn = finish_withdrawal(&tx, &intent, Some(4000)).unwrap();
        let receipt =
            ResponseBody::InferenceEquivalenceSaved(Box::new(InferenceEquivalenceReceipt {
                request_id: request.request_id.clone(),
                input_sha256: None,
                report: withdrawn.clone(),
            }));
        remember(&tx, actor.as_str(), &request, &fingerprint, &receipt).unwrap();
        tx.commit().unwrap();
    }
    assert!(fixture.read(4000).is_err());
    let hash = digest(bounded_report(&withdrawn).unwrap().as_bytes());
    assert!(read_current_accepted(
        &fixture.db,
        &withdrawn.dossier_id,
        withdrawn.revision,
        &hash,
        4000
    )
    .is_err());
}
