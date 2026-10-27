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
