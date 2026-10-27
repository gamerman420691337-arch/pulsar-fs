mod pack_withdrawal {
    use super::*;
    fn profile(model: ArtifactIdentity) -> DeclaredInferenceProfile {
        DeclaredInferenceProfile {
            schema_version: 1,
            model: model.clone(),
            runtime: model,
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    class_count: 1,
                    layout: TensorLayout::Nchw,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 134217728,
            minimum_output_bytes: 1048576,
            cpu_threads: 2,
            cuda_resources: None,
        }
    }
    fn select(f: &Fixture, sha: &str, model: ArtifactIdentity) -> ModelPackChoice {
        let mut db = f.engine.db.lock().unwrap();
        let old = crate::model_pack_selection::status(&db).unwrap();
        let req = request(
            &f.session,
            None,
            None,
            Command::SelectModelPack {
                manifest_sha256: Some(sha.into()),
                expected_selection_revision: old.selection_revision,
                approve_unqualified_native_execution: true,
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        );
        let tx = db.transaction().unwrap();
        let report = crate::model_pack_selection::select(
            &tx,
            old.selection_revision,
            Some((sha.into(), profile(model))),
            &f.session,
            &req.request_id,
        )
        .unwrap();
        let choice = report.choice.clone().unwrap();
        remember(
            &tx,
            f.session.as_str(),
            &req,
            "fixture-selection-fingerprint",
            &ResponseBody::ModelPackSelection(Box::new(report)),
        )
        .unwrap();
        tx.commit().unwrap();
        choice
    }
    fn annotate(
        f: &Fixture,
        manifest: &WorkerRequest,
        choice: Option<&ModelPackChoice>,
    ) -> Arc<AtomicBool> {
        let command = if let Some(choice) = choice {
            Command::GenerateFromModelPack {
                source_version: manifest.source.source_version.clone(),
                preset: "fast".into(),
                settings: None,
                choice: choice.clone(),
            }
        } else {
            Command::Generate {
                source_version: manifest.source.source_version.clone(),
                preset: "fast".into(),
                model_path: None,
                settings: None,
            }
        };
        let req = request(&f.session, Some(&f.project), None, command);
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        model_pack_withdrawal::record_admission(
            &tx,
            manifest,
            &req,
            &manifest.dependencies[manifest.dependencies.len() - 1..],
        )
        .unwrap();
        tx.commit().unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        f.engine
            .workers
            .lock()
            .unwrap()
            .insert(manifest.job_id.to_string(), flag.clone());
        flag
    }
    fn pack_manifest(f: &Fixture) -> WorkerRequest {
        let (mut manifest, _) = staged_manifest(f);
        manifest.dependencies.push(ArtifactIdentity {
            artifact_id: ArtifactId::new("fixture-approval").unwrap(),
            sha256: "b".repeat(64),
            byte_len: 100,
        });
        let db = f.engine.db.lock().unwrap();
        db.execute(
            "UPDATE jobs SET manifest=?2 WHERE id=?1",
            params![manifest.job_id.as_str(), encode(&manifest).unwrap()],
        )
        .unwrap();
        db.execute(
            "UPDATE attempts SET manifest=?2 WHERE id=?1 AND job=?3",
            params![
                manifest.attempt_id.as_str(),
                encode(&manifest).unwrap(),
                manifest.job_id.as_str()
            ],
        )
        .unwrap();
        manifest
    }
    fn model(m: &WorkerRequest) -> ArtifactIdentity {
        let WorkerOperation::Generate {
            model: Some(model), ..
        } = &m.operation
        else {
            panic!()
        };
        model.identity.clone()
    }
    fn withdraw(f: &Fixture, choice: &ModelPackChoice, include: bool) -> Request {
        let current = crate::model_pack_selection::status(&f.engine.db.lock().unwrap()).unwrap();
        request(
            &f.session,
            None,
            None,
            Command::WithdrawModelPackApproval {
                choice: choice.clone(),
                expected_selection_revision: current.selection_revision,
                include_unclassified_generation: include,
                owner_proof: f.engine.model_pack_owner_token.clone(),
            },
        )
    }
    #[test]
    fn historical_withdrawal_uses_admission_roles_and_requires_explicit_unclassified_scope() {
        let f = fixture();
        let a = pack_manifest(&f);
        let choice = select(&f, &"c".repeat(64), model(&a));
        let flag_a = annotate(&f, &a, Some(&choice));
        let b = pack_manifest(&f);
        let other = select(&f, &"d".repeat(64), model(&b));
        let flag_b = annotate(&f, &b, Some(&other));
        let direct = pack_manifest(&f);
        let flag_direct = annotate(&f, &direct, None);
        let unknown = pack_manifest(&f);
        let flag_unknown = Arc::new(AtomicBool::new(false));
        f.engine
            .workers
            .lock()
            .unwrap()
            .insert(unknown.job_id.to_string(), flag_unknown.clone());
        let refuse = withdraw(&f, &choice, false);
        assert_eq!(
            f.engine.handle(refuse).result.unwrap_err().code,
            ErrorCode::Unavailable
        );
        assert!(!flag_a.load(Ordering::Acquire));
        assert!(!flag_unknown.load(Ordering::Acquire));
        let req = withdraw(&f, &choice, true);
        let ResponseBody::ModelPackWithdrawal(receipt) =
            f.engine.handle(req.clone()).result.unwrap()
        else {
            panic!()
        };
        receipt.validate_reply(&req).unwrap();
        assert_eq!(receipt.cancelled_approved_jobs, 1);
        assert_eq!(receipt.cancelled_unclassified_jobs, 1);
        assert_eq!(receipt.selection.choice, Some(other));
        assert_eq!(receipt.selection.selection_revision, 2);
        assert!(flag_a.load(Ordering::Acquire));
        assert!(flag_unknown.load(Ordering::Acquire));
        assert!(!flag_b.load(Ordering::Acquire));
        assert!(!flag_direct.load(Ordering::Acquire));
        let replacement = Arc::new(AtomicBool::new(false));
        f.engine
            .workers
            .lock()
            .unwrap()
            .insert(a.job_id.to_string(), replacement.clone());
        f.engine.handle(req).result.unwrap();
        assert!(!replacement.load(Ordering::Acquire));
        f.engine
            .finish_job(&a, Err(ProtocolError::invalid("late result")))
            .unwrap();
        assert_eq!(
            job_snapshot(&f.engine.db.lock().unwrap(), &a.job_id, &f.project)
                .unwrap()
                .state,
            "cancelled"
        );
    }
    #[test]
    fn withdrawal_receipt_and_commit_refusal_roll_back_selection_jobs_and_signals() {
        for at_commit in [false, true] {
            let f = fixture();
            let m = pack_manifest(&f);
            let choice = select(&f, &"c".repeat(64), model(&m));
            let flag = annotate(&f, &m, Some(&choice));
            let req = withdraw(&f, &choice, false);
            {
                let db = f.engine.db.lock().unwrap();
                if at_commit {
                    db.execute_batch("CREATE TABLE withdrawal_parent(id INTEGER PRIMARY KEY); CREATE TABLE withdrawal_child(id INTEGER REFERENCES withdrawal_parent(id) DEFERRABLE INITIALLY DEFERRED); CREATE TEMP TRIGGER deny_withdrawal AFTER INSERT ON requests BEGIN INSERT INTO withdrawal_child VALUES(1); END;").unwrap();
                } else {
                    db.execute_batch("CREATE TEMP TRIGGER deny_withdrawal BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'withdrawal receipt refusal'); END;").unwrap();
                }
            }
            assert_eq!(
                f.engine.handle(req.clone()).result.unwrap_err().code,
                ErrorCode::Internal
            );
            assert!(!flag.load(Ordering::Acquire));
            {
                let db = f.engine.db.lock().unwrap();
                assert_eq!(
                    crate::model_pack_selection::status(&db)
                        .unwrap()
                        .choice
                        .as_ref(),
                    Some(&choice)
                );
                assert_eq!(
                    job_snapshot(&db, &m.job_id, &f.project).unwrap().state,
                    "queued"
                );
                db.execute_batch("DROP TRIGGER deny_withdrawal;").unwrap();
            }
            let ResponseBody::ModelPackWithdrawal(reply) =
                f.engine.handle(req.clone()).result.unwrap()
            else {
                panic!()
            };
            reply.validate_reply(&req).unwrap();
            assert!(reply.selection.choice.is_none());
            assert_eq!(reply.selection.selection_revision, 2);
            assert_eq!(reply.cancelled_approved_jobs, 1);
            assert!(flag.load(Ordering::Acquire));
        }
    }
    #[test]
    fn withdrawal_refuses_wrong_owner_stale_fence_unissued_approval_and_corrupt_admission() {
        let f = fixture();
        let m = pack_manifest(&f);
        let choice = select(&f, &"c".repeat(64), model(&m));
        let flag = annotate(&f, &m, Some(&choice));
        let mut req = withdraw(&f, &choice, false);
        if let Command::WithdrawModelPackApproval { owner_proof, .. } = &mut req.command {
            *owner_proof = "wrong".into();
        }
        assert_eq!(
            f.engine.handle(req).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        let mut req = withdraw(&f, &choice, false);
        if let Command::WithdrawModelPackApproval {
            expected_selection_revision,
            ..
        } = &mut req.command
        {
            *expected_selection_revision = 2;
        }
        assert_eq!(
            f.engine.handle(req).result.unwrap_err().code,
            ErrorCode::DependencyMismatch
        );
        let fake = ModelPackChoice {
            manifest_sha256: "d".repeat(64),
            selection_revision: 1,
        };
        assert_eq!(
            f.engine
                .handle(withdraw(&f, &fake, false))
                .result
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
        f.engine.db.lock().unwrap().execute("UPDATE requests SET response='{}' WHERE session='pulsar/generation-admission/v1' AND request=?1",[m.job_id.as_str()]).unwrap();
        assert_eq!(
            f.engine
                .handle(withdraw(&f, &choice, true))
                .result
                .unwrap_err()
                .code,
            ErrorCode::DependencyMismatch
        );
        assert!(!flag.load(Ordering::Acquire));
        assert!(
            crate::model_pack_selection::status(&f.engine.db.lock().unwrap())
                .unwrap()
                .choice
                .is_some()
        );
    }
    #[test]
    fn private_admission_namespace_is_not_a_session_and_direct_alias_cannot_gain_pack_role() {
        assert!(SessionId::new("pulsar/generation-admission/v1").is_err());
        let f = fixture();
        let m = pack_manifest(&f);
        let choice = select(&f, &"c".repeat(64), model(&m));
        let flag = annotate(&f, &m, None);
        let ResponseBody::ModelPackWithdrawal(report) = f
            .engine
            .handle(withdraw(&f, &choice, false))
            .result
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(report.cancelled_approved_jobs, 0);
        assert!(!flag.load(Ordering::Acquire));
    }
}
