mod inference_equivalence_lifecycle {
    use super::*;
    mod authored {
        include!("inference_equivalence_fixture.rs");
    }

    fn import_request(f: &Fixture) -> Request {
        let (manifest, payloads) = authored::synthetic_fixture();
        import_request_with(f, manifest, payloads)
    }
    fn import_request_with(
        f: &Fixture,
        manifest: InferenceEquivalenceManifest,
        payloads: std::collections::BTreeMap<InferenceEquivalenceEvidenceKind, Vec<u8>>,
    ) -> Request {
        let mut evidence = Vec::new();
        for (kind, bytes) in payloads {
            let identity = manifest.evidence_identity(kind).unwrap();
            let path = f.dir.path().join(format!("{}.json", identity.sha256));
            fs::write(&path, bytes).unwrap();
            evidence.push(InferenceEquivalenceInputEvidence {
                kind,
                path: path.to_str().unwrap().into(),
                sha256: identity.sha256.clone(),
                byte_len: identity.byte_len,
            });
        }
        let input = InferenceEquivalenceInput {
            subject: manifest.subject,
            evidence,
        };
        input.validate().unwrap();
        let bytes = serde_json::to_vec(&input).unwrap();
        let path = f.dir.path().join("equivalence-input.json");
        fs::write(&path, &bytes).unwrap();
        request(
            &f.session,
            None,
            None,
            Command::ImportInferenceEquivalence {
                path: path.to_str().unwrap().into(),
                manifest_sha256: format!("{:x}", Sha256::digest(&bytes)),
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        )
    }
    fn receipt(body: ResponseBody) -> InferenceEquivalenceReceipt {
        let ResponseBody::InferenceEquivalenceSaved(receipt) = body else {
            panic!("equivalence receipt expected")
        };
        receipt.validate().unwrap();
        *receipt
    }
    fn status(f: &Fixture, id: &ArtifactId) -> InferenceEquivalenceStatus {
        let response = f
            .engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::InferenceEquivalenceStatus {
                    dossier_id: id.clone(),
                    owner_proof: f.engine.model_pack_owner_token.clone(),
                },
            ))
            .result
            .unwrap();
        let ResponseBody::InferenceEquivalenceStatus(status) = response else {
            panic!("equivalence status expected")
        };
        status.validate().unwrap();
        *status
    }
    fn withdrawal(f: &Fixture, report: &InferenceEquivalenceReport) -> Request {
        request(
            &f.session,
            None,
            None,
            Command::WithdrawInferenceEquivalence {
                dossier_id: report.dossier_id.clone(),
                expected_record_revision: report.revision,
                reason: "withdraw authored synthetic test dossier".into(),
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        )
    }
    fn imported(f: &Fixture) -> InferenceEquivalenceReceipt {
        receipt(f.engine.handle(import_request(f)).result.unwrap())
    }

    #[test]
    fn inference_equivalence_import_auth_replay_and_public_synthetic_refusal() {
        let f = fixture();
        let req = import_request(&f);
        let mut forbidden = req.clone();
        let Command::ImportInferenceEquivalence { owner_proof, .. } = &mut forbidden.command else {
            unreachable!()
        };
        *owner_proof = "0".repeat(64);
        assert_eq!(
            f.engine.handle(forbidden).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let first = receipt(f.engine.handle(req.clone()).result.unwrap());
        assert_eq!(first.report.state, InferenceEquivalenceState::Pending);
        assert_eq!(first.report.revision, 1);
        assert!(first.report.decision.is_none());
        let accept = request(
            &f.session,
            None,
            None,
            Command::AcceptInferenceEquivalence {
                dossier_id: first.report.dossier_id.clone(),
                expected_record_revision: 1,
                operator: "authored test operator".into(),
                acknowledge_measured_equivalence: true,
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        );
        assert!(f.engine.handle(accept).result.is_err());
        assert_eq!(
            status(&f, &first.report.dossier_id).report.state,
            InferenceEquivalenceState::Pending
        );
        fs::remove_file(f.dir.path().join("equivalence-input.json")).unwrap();
        assert_eq!(
            encode(&receipt(f.engine.handle(req.clone()).result.unwrap())).unwrap(),
            encode(&first).unwrap()
        );
        let mut fresh = req;
        fresh.request_id = id!(RequestId);
        assert!(f.engine.handle(fresh).result.is_err());
        let db = f.engine.db.lock().unwrap();
        assert_ne!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            35
        );
        inference_equivalence_store::validate(&db).unwrap();
    }

    #[test]
    fn inference_equivalence_withdrawal_survives_receipt_failure_and_reuses_original_id() {
        let f = fixture();
        let first = imported(&f);
        let req = withdrawal(&f, &first.report);
        {
            let db = f.engine.db.lock().unwrap();
            db.execute_batch("CREATE TEMP TRIGGER reject_equivalence_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(FAIL,'injected receipt failure'); END").unwrap();
        }
        let error = f.engine.handle(req.clone()).result.unwrap_err();
        assert!(error.message.contains("injected receipt failure"));
        let pending = status(&f, &first.report.dossier_id);
        assert!(pending.withdrawal_pending);
        assert_eq!(pending.report.revision, 1);
        assert_eq!(pending.report.state, InferenceEquivalenceState::Pending);
        {
            let db = f.engine.db.lock().unwrap();
            db.execute_batch("DROP TRIGGER reject_equivalence_receipt")
                .unwrap();
            assert_eq!(
                db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                35
            );
            inference_equivalence_store::validate(&db).unwrap();
        }
        let mut collision = req.clone();
        collision.command = Command::CreateProject {
            name: "must never be created".into(),
        };
        assert_eq!(
            f.engine.handle(collision).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let done = receipt(f.engine.handle(req.clone()).result.unwrap());
        assert_eq!(done.report.state, InferenceEquivalenceState::Withdrawn);
        assert_eq!(done.report.revision, 2);
        assert!(!status(&f, &first.report.dossier_id).withdrawal_pending);
        assert_eq!(
            encode(&receipt(f.engine.handle(req).result.unwrap())).unwrap(),
            encode(&done).unwrap()
        );
        let again = imported(&f);
        assert_eq!(
            encode(&again.report).unwrap(),
            encode(&done.report).unwrap()
        );
        inference_equivalence_store::validate(&f.engine.db.lock().unwrap()).unwrap();
    }

    #[test]
    fn inference_equivalence_pending_withdrawal_recovers_after_engine_restart() {
        let f = fixture();
        let first = imported(&f);
        let req = withdrawal(&f, &first.report);
        let fingerprint = format!("{:x}", Sha256::digest(request_fingerprint(&req).unwrap()));
        let Command::WithdrawInferenceEquivalence {
            dossier_id,
            expected_record_revision,
            reason,
            ..
        } = &req.command
        else {
            unreachable!()
        };
        {
            let mut db = f.engine.db.lock().unwrap();
            let tx = db.transaction().unwrap();
            inference_equivalence_store::begin_withdrawal(
                &tx,
                dossier_id,
                *expected_record_revision,
                &f.session,
                &req,
                &fingerprint,
                reason,
            )
            .unwrap();
            tx.commit().unwrap();
        }
        let Fixture {
            engine,
            session,
            project,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        let f = Fixture {
            engine: Engine::open(config).unwrap(),
            session,
            project,
            dir,
        };
        assert!(status(&f, &first.report.dossier_id).withdrawal_pending);
        let done = receipt(f.engine.handle(req).result.unwrap());
        assert_eq!(done.report.state, InferenceEquivalenceState::Withdrawn);
        assert!(!status(&f, &first.report.dossier_id).withdrawal_pending);
        let Fixture {
            engine,
            session,
            project,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        let f = Fixture {
            engine: Engine::open(config).unwrap(),
            session,
            project,
            dir,
        };
        assert_eq!(
            status(&f, &first.report.dossier_id).report.state,
            InferenceEquivalenceState::Withdrawn
        );
    }

    #[test]
    fn inference_equivalence_import_receipt_failure_rolls_back_all_auxiliary_state() {
        let f = fixture();
        let req = import_request(&f);
        {
            let db = f.engine.db.lock().unwrap();
            db.execute_batch("CREATE TEMP TRIGGER reject_equivalence_receipt BEFORE INSERT ON requests BEGIN SELECT RAISE(FAIL,'injected import receipt failure'); END").unwrap();
        }
        assert!(f.engine.handle(req.clone()).result.is_err());
        {
            let db = f.engine.db.lock().unwrap();
            db.execute_batch("DROP TRIGGER reject_equivalence_receipt")
                .unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name GLOB 'inference_equivalence*'",
                    [],
                    |r| r.get::<_, u64>(0)
                )
                .unwrap(),
                0
            );
        }
        let imported = receipt(f.engine.handle(req).result.unwrap());
        assert_eq!(imported.report.revision, 1);
    }
    #[test]
    fn inference_equivalence_semantic_mismatch_with_resealed_hashes_cannot_import() {
        let f = fixture();
        let (manifest, bytes) = authored::synthetic_fixture();
        let mut documents = authored::fixture_documents(&bytes);
        let InferenceEquivalenceEvidenceDetails::FromExecution(execution) = documents
            .get_mut(&InferenceEquivalenceEvidenceKind::FromExecution)
            .unwrap()
        else {
            unreachable!()
        };
        execution.cases[0].target_id = "different-target".into();
        let (manifest, bytes) = authored::seal_synthetic(manifest.subject, documents);
        let req = import_request_with(&f, manifest, bytes);
        assert!(f.engine.handle(req).result.is_err());
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name GLOB 'inference_equivalence*'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn inference_equivalence_corrupt_evidence_does_not_prevent_withdrawal() {
        let f = fixture();
        let first = imported(&f);
        {
            let db = f.engine.db.lock().unwrap();
            db.execute("UPDATE inference_equivalence_evidence SET data=CAST('corrupted' AS BLOB) WHERE dossier=?1", [first.report.dossier_id.as_str()]).unwrap();
        }
        let done = receipt(
            f.engine
                .handle(withdrawal(&f, &first.report))
                .result
                .unwrap(),
        );
        assert_eq!(done.report.state, InferenceEquivalenceState::Withdrawn);
        let db = f.engine.db.lock().unwrap();
        assert_eq!(
            inference_equivalence_store::report(&db, &first.report.dossier_id)
                .unwrap()
                .state,
            InferenceEquivalenceState::Withdrawn
        );
        assert!(inference_equivalence_store::verify_evidence(&db, &done.report).is_err());
        assert!(inference_equivalence_store::validate(&db).is_err());
    }
}
