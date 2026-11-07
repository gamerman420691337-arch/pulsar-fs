// Included inside authority::tests. Metadata below is authored structural test
// input only: no accepted dossier, native retirement or live Lease is fabricated.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
mod inference_fallback_controller {
    use super::super::inference_fallback_store::{
        self as store, FallbackAdmission, FallbackAdmissionKind,
    };
    use super::*;

    fn metadata_identity(n: u8) -> ArtifactIdentity {
        let sha256 = format!("{n:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 1,
        }
    }
    fn selected_policy() -> InferenceFallbackSelection {
        InferenceFallbackSelection {
            schema_version: 1,
            equivalents: vec![InferenceFallbackReference {
                dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                expected_revision: 1,
                expected_report_sha256: "b".repeat(64),
            }],
            maximum_fallbacks: 1,
            source_policy: InferenceFallbackSourcePolicy {
                schema_version: 1,
                kind: InferenceFallbackSourceKind::DecodedMedia,
                sources: None,
                independent_audio: InferenceFallbackIndependentAudio::Any,
            },
            view_policy: InferenceFallbackViewPolicy {
                schema_version: 1,
                applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
            },
        }
    }
    fn seed_fallback(f: &Fixture) -> WorkerRequest {
        let (mut manifest, _) = staged_manifest(f);
        let WorkerOperation::Generate {
            settings, model, ..
        } = &mut manifest.operation
        else {
            unreachable!()
        };
        let runtime = metadata_identity(41);
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: model.as_ref().unwrap().identity.clone(),
            runtime: runtime.clone(),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        };
        settings.model_input = Some(profile.preprocessing.input.clone());
        settings.inference_profile = Some(profile);
        manifest.tools.onnx_runtime = Some(ArtifactReference {
            identity: runtime.clone(),
            path: "/nonexistent/fallback-controller/runtime".into(),
        });
        let selection = selected_policy();
        manifest.dependencies.extend([
            runtime.clone(),
            f.engine.binary_identity.clone(),
            selection.identity().unwrap(),
        ]);
        manifest.validate().unwrap();
        let admission = FallbackAdmission {
            schema_version: 1,
            kind: FallbackAdmissionKind::Initial,
            ordinal: 0,
            predecessor: None,
            reference: selection.equivalents[0].clone(),
            observed_environment: InferenceEquivalenceEnvironment {
                engine: metadata_identity(42),
                worker: f.engine.binary_identity.clone(),
                os: "linux".into(),
                architecture: "x86_64".into(),
                platform_build: metadata_identity(43),
                hardware: metadata_identity(44),
                dependencies: vec![runtime],
            },
            admitted_at_unix_ms: 10,
            expires_at_unix_ms: 100,
        };
        let raw = encode(&manifest).unwrap();
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        tx.execute(
            "UPDATE jobs SET manifest=?1 WHERE id=?2",
            params![raw, manifest.job_id.as_str()],
        )
        .unwrap();
        tx.execute(
            "UPDATE attempts SET manifest=?1 WHERE id=?2",
            params![raw, manifest.attempt_id.as_str()],
        )
        .unwrap();
        store::declare_initial(&tx, &manifest, &selection, &admission).unwrap();
        tx.commit().unwrap();
        manifest
    }
    fn seed_pending(f: &Fixture) -> WorkerRequest {
        let manifest = seed_fallback(f);
        let WorkerOperation::Generate { settings, .. } = &manifest.operation else {
            unreachable!()
        };
        let profile = settings.inference_profile.as_ref().unwrap();
        let error = JobAttemptDiagnostic::from_error(&ProtocolError::new(
            ErrorCode::Unavailable,
            "authored original provider-unavailable diagnostic",
        ));
        let outcome = JobAttemptOutcome {
            schema_version: 1,
            manifest_sha256: format!(
                "{:x}",
                Sha256::digest(encode(&manifest).unwrap().as_bytes())
            ),
            engine_epoch: AttemptId::new(f.engine.engine_epoch.clone()).unwrap(),
            validation_error: Some(error.clone()),
            worker_retirement: Some(WorkerRetirementObservation {
                worker_error: Some(error),
                provider_unavailable: Some(InferenceProviderUnavailable {
                    schema_version: 1,
                    provider: profile.provider,
                    profile_sha256: format!(
                        "{:x}",
                        Sha256::digest(encode(profile).unwrap().as_bytes())
                    ),
                    check: InferenceProviderAvailabilityCheck::NotListedByRuntime,
                }),
            }),
        };
        outcome.validate_for(&manifest).unwrap();
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        tx.execute(
            "UPDATE jobs SET state='failed',error='authored predecessor failure' WHERE id=?1",
            [manifest.job_id.as_str()],
        )
        .unwrap();
        tx.execute(
            "UPDATE attempts SET state='failed' WHERE id=?1",
            [manifest.attempt_id.as_str()],
        )
        .unwrap();
        // This is authored structural history, never a claimed native receipt.
        crate::job_attempt_history::retain(&tx, &manifest, &outcome).unwrap();
        assert!(store::mark_pending(&tx, &manifest, &outcome.engine_epoch).unwrap());
        tx.commit().unwrap();
        manifest
    }
    fn immutable_attempt(engine: &Engine, manifest: &WorkerRequest) -> (String, String, String) {
        engine.db.lock().unwrap().query_row("SELECT a.state,a.manifest,o.record FROM attempts a JOIN job_attempt_outcomes o ON o.attempt=a.id WHERE a.id=?1",[manifest.attempt_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).unwrap()
    }
    fn assert_no_successor(engine: &Engine, manifest: &WorkerRequest) {
        let db = engine.db.lock().unwrap();
        let row:(String,i64,i64,i64)=db.query_row("SELECT attempt,attempts,(SELECT count(*) FROM attempts WHERE job=jobs.id),(SELECT count(*) FROM inference_fallback_pending WHERE job=jobs.id) FROM jobs WHERE id=?1",[manifest.job_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        assert_eq!(row, (manifest.attempt_id.to_string(), 1, 1, 0));
        assert!(engine.workers.lock().unwrap().is_empty());
        assert!(!engine.worker_fence.has_pending_worker_custody().unwrap());
        assert_eq!(engine.pool.reserved_storage(), 0);
    }
    fn status(engine: &Arc<Engine>, session: &SessionId, manifest: &WorkerRequest) -> JobSnapshot {
        let ResponseBody::Job(status) = engine
            .handle(request(
                session,
                Some(&manifest.project_id),
                None,
                Command::JobStatus {
                    job_id: manifest.job_id.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("job status")
        };
        status
    }

    #[test]
    fn fallback_capture_requires_original_live_preparation_and_current_grant() {
        let f = fixture();
        let manifest = seed_pending(&f);
        let before = immutable_attempt(&f.engine, &manifest);
        let cancel = Arc::new(AtomicBool::new(false));
        let check = |request: &WorkerRequest, owner: &SessionId, flag: &Arc<AtomicBool>| {
            f.engine.require_fallback_capture_admission(
                &f.engine.db.lock().unwrap(),
                request,
                owner,
                flag,
            )
        };
        assert!(check(&manifest, &f.session, &cancel).is_err());
        f.engine
            .workers
            .lock()
            .unwrap()
            .insert(manifest.job_id.to_string(), cancel.clone());
        assert_eq!(
            check(&manifest, &f.session, &cancel).unwrap(),
            Some(manifest.job_id.clone())
        );
        assert!(check(
            &manifest,
            &SessionId::new("different-owner").unwrap(),
            &cancel
        )
        .is_err());
        assert!(check(&manifest, &f.session, &Arc::new(AtomicBool::new(false))).is_err());
        let mut altered = manifest.clone();
        altered.base_revision = altered.base_revision.checked_next().unwrap();
        assert!(check(&altered, &f.session, &cancel).is_err());
        cancel.store(true, Ordering::Release);
        assert_eq!(
            check(&manifest, &f.session, &cancel).unwrap_err().code,
            ErrorCode::Cancelled
        );
        cancel.store(false, Ordering::Release);
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
        assert_eq!(
            check(&manifest, &f.session, &cancel).unwrap_err().code,
            ErrorCode::Forbidden
        );
        f.engine
            .workers
            .lock()
            .unwrap()
            .remove(manifest.job_id.as_str());
        assert_eq!(immutable_attempt(&f.engine, &manifest), before);
        assert_eq!(f.engine.pool.reserved_storage(), 0);
        assert!(!f.engine.worker_fence.has_pending_worker_custody().unwrap());
    }

    #[test]
    fn public_fallback_requires_import_scope_before_model_snapshot() {
        let f = fixture();
        let manifest = seed_fallback(&f);
        let limited = pair(&f.engine, "fallback generator without import authority");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: limited.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Generate],
                },
            ))
            .result
            .unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            require_grant(&db, &limited, &f.project, Scope::Generate).unwrap();
            assert_eq!(
                require_grant(&db, &limited, &f.project, Scope::ImportSource)
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        let retained = || {
            let db = f.engine.db.lock().unwrap();
            let header: i64 = db
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            let tables = [
                "projects",
                "sources",
                "jobs",
                "attempts",
                "candidates",
                "requests",
                "inference_fallback_jobs",
                "inference_fallback_attempts",
                "inference_fallback_pending",
            ]
            .map(|table| {
                let mut statement = db
                    .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                    .unwrap();
                let columns = statement.column_count();
                let rows = statement
                    .query_map([], |row| {
                        (0..columns)
                            .map(|index| row.get::<_, rusqlite::types::Value>(index))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })
                    .unwrap();
                rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
            });
            let snapshots = fs::read_dir(f.engine.config.state_dir.join("snapshots"))
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    assert!(entry.file_type().unwrap().is_file());
                    (entry.file_name(), fs::read(entry.path()).unwrap())
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            (header, tables, snapshots)
        };
        let before = retained();
        let WorkerOperation::Generate {
            preset, settings, ..
        } = manifest.operation.clone()
        else {
            panic!("generation fixture");
        };
        let missing_model = f.dir.path().join("absent-fallback-model.onnx");
        assert!(!missing_model.exists());
        let submission = request(
            &limited,
            Some(&f.project),
            Some(0),
            Command::GenerateWithInferenceFallback {
                source_version: manifest.source.source_version.clone(),
                preset,
                settings,
                model: InferenceFallbackModel::Path {
                    path: missing_model.clone(),
                },
                selection: Box::new(selected_policy()),
            },
        );
        submission.validate().unwrap();
        let error = f.engine.handle(submission).result.unwrap_err();
        // Snapshotting the absent model would report Unavailable. The exact
        // capability refusal must precede model preparation and derived dispatch.
        assert_eq!(error.code, ErrorCode::Forbidden, "{error}");
        assert_eq!(error.message, "client lacks required project capability");
        assert!(!missing_model.exists());
        assert!(
            retained() == before,
            "denied fallback changed retained state"
        );
        assert_no_successor(&f.engine, &manifest);
        assert!(!f.engine.worker_fence.has_pending_storage_custody().unwrap());
    }

    #[test]
    fn pending_fallback_does_not_authenticate_a_public_tokenless_request() {
        let f = fixture();
        let manifest = seed_pending(&f);
        let before = immutable_attempt(&f.engine, &manifest);
        let mut forged = request(
            &f.session,
            Some(&f.project),
            None,
            Command::ResumeJob {
                job_id: manifest.job_id.clone(),
            },
        );
        forged.auth_token = None;
        assert_eq!(
            f.engine.handle(forged).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        assert_eq!(immutable_attempt(&f.engine, &manifest), before);
        assert_eq!(
            status(&f.engine, &f.session, &manifest).state,
            "fallback_pending"
        );
        assert!(f.engine.workers.lock().unwrap().is_empty());
    }

    #[test]
    fn public_cancel_pending_preserves_failed_predecessor_and_exact_outcome() {
        let f = fixture();
        let manifest = seed_pending(&f);
        let before = immutable_attempt(&f.engine, &manifest);
        assert_eq!(
            status(&f.engine, &f.session, &manifest).state,
            "fallback_pending"
        );
        let cancellation = request(
            &f.session,
            Some(&f.project),
            None,
            Command::CancelJob {
                job_id: manifest.job_id.clone(),
            },
        );
        let first = f.engine.handle(cancellation.clone()).result.unwrap();
        let repeated = f.engine.handle(cancellation).result.unwrap();
        assert_eq!(encode(&first).unwrap(), encode(&repeated).unwrap());
        let after = status(&f.engine, &f.session, &manifest);
        assert_eq!(after.state, "cancelled");
        assert!(after
            .error
            .as_deref()
            .unwrap()
            .contains("cancelled by authorized client"));
        assert_eq!(immutable_attempt(&f.engine, &manifest), before);
        assert_eq!(before.0, "failed");
        assert_no_successor(&f.engine, &manifest);
        store::validate(&f.engine.db.lock().unwrap()).unwrap();
    }

    #[test]
    fn public_revoke_cancels_only_the_current_attempt_and_preserves_resume_history() {
        let f = fixture();
        let previous = seed_fallback(&f);
        let mut next = previous.clone();
        next.attempt_id = id!(AttemptId);
        next.output_dir = previous
            .output_dir
            .parent()
            .unwrap()
            .join(next.attempt_id.as_str());
        next.validate().unwrap();
        {
            let mut db = f.engine.db.lock().unwrap();
            let mut admission = store::retained(&db, &previous).unwrap().unwrap().admission;
            admission.kind = FallbackAdmissionKind::Resume;
            admission.predecessor = Some(previous.attempt_id.clone());
            admission.admitted_at_unix_ms += 1;
            let tx = db.transaction().unwrap();
            tx.execute(
                "UPDATE attempts SET state='assistant_paused' WHERE id=?1",
                [previous.attempt_id.as_str()],
            )
            .unwrap();
            let raw = encode(&next).unwrap();
            tx.execute(
                "INSERT INTO attempts(id,job,manifest,state) VALUES(?1,?2,?3,'queued')",
                params![next.attempt_id.as_str(), next.job_id.as_str(), raw],
            )
            .unwrap();
            tx.execute("UPDATE jobs SET attempt=?1,manifest=?2,state='queued',attempts=attempts+1 WHERE id=?3",
                params![next.attempt_id.as_str(),raw,next.job_id.as_str()]).unwrap();
            store::record_resume(&tx, &previous, &next, &admission).unwrap();
            tx.commit().unwrap();
        }
        let admin = pair(&f.engine, "resume history grant administrator");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: admin.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::ManageGrants],
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(status(&f.engine, &admin, &next).state, "cancelled");
        let db = f.engine.db.lock().unwrap();
        let state: String = db
            .query_row(
                "SELECT state FROM attempts WHERE id=?1",
                [previous.attempt_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            state, "assistant_paused",
            "revocation must not rewrite a consumed historical pause"
        );
        let current: String = db
            .query_row(
                "SELECT state FROM attempts WHERE id=?1",
                [next.attempt_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(current, "cancelled");
        store::validate(&db).unwrap();
        assert!(store::retained(&db, &next).unwrap().is_some());
        drop(db);
        let ResponseBody::JobAttempts(page) = f
            .engine
            .handle(request(
                &admin,
                Some(&f.project),
                None,
                Command::JobAttempts {
                    job_id: next.job_id.clone(),
                    after_attempt: None,
                    limit: 16,
                },
            ))
            .result
            .unwrap()
        else {
            panic!("attempt history")
        };
        page.validate().unwrap();
        assert!(page.history_only);
        assert_eq!(page.current_attempt, next.attempt_id);
        assert_eq!(page.attempts.len(), 2);
        let prior = page
            .attempts
            .iter()
            .find(|a| a.attempt_id == previous.attempt_id)
            .unwrap();
        assert_eq!(prior.state, "assistant_paused");
        let resumed = page
            .attempts
            .iter()
            .find(|a| a.attempt_id == next.attempt_id)
            .unwrap();
        let trace = resumed.fallback.as_ref().unwrap();
        assert!(trace.history_only);
        assert_eq!(trace.predecessor, Some(previous.attempt_id));
        assert!(matches!(trace.kind, JobAttemptFallbackKind::Resume));
    }

    #[test]
    fn public_revoke_then_regrant_cannot_restore_pending_fallback() {
        let f = fixture();
        let manifest = seed_pending(&f);
        let before = immutable_attempt(&f.engine, &manifest);
        let admin = pair(&f.engine, "fallback grant administrator");
        f.engine
            .handle(request(
                &f.session,
                None,
                None,
                Command::Grant {
                    session: admin.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Generate, Scope::ManageGrants],
                },
            ))
            .result
            .unwrap();
        f.engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Revoke {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert_eq!(status(&f.engine, &admin, &manifest).state, "cancelled");
        f.engine
            .handle(request(
                &admin,
                None,
                None,
                Command::Grant {
                    session: f.session.clone(),
                    project_id: f.project.clone(),
                    scopes: vec![Scope::Read, Scope::Generate],
                },
            ))
            .result
            .unwrap();
        f.engine.recover_interrupted();
        assert_eq!(status(&f.engine, &f.session, &manifest).state, "cancelled");
        assert_eq!(immutable_attempt(&f.engine, &manifest), before);
        assert_no_successor(&f.engine, &manifest);
        assert!(f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ResumeJob {
                    job_id: manifest.job_id.clone()
                }
            ))
            .result
            .is_err());
        assert_no_successor(&f.engine, &manifest);
    }

    #[test]
    fn real_engine_reopen_retires_pending_without_restoring_attempt_or_permission() {
        let f = fixture();
        let manifest = seed_pending(&f);
        let before = immutable_attempt(&f.engine, &manifest);
        let Fixture {
            engine,
            session,
            project: _,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        let reopened = reopen_after_transient_leases(config).unwrap();
        let current = status(&reopened, &session, &manifest);
        assert_eq!(current.state, "failed");
        assert!(current
            .error
            .as_deref()
            .unwrap()
            .contains("fallback interrupted by Engine restart"));
        assert_eq!(immutable_attempt(&reopened, &manifest), before);
        assert_no_successor(&reopened, &manifest);
        let db = reopened.db.lock().unwrap();
        assert!(store::pending(&db, &manifest.job_id).unwrap().is_none());
        assert!(reopened
            .require_fallback_publication(&db, &manifest, None)
            .is_err());
        drop(db);
        Engine::retire_for_test(reopened);
        drop(dir);
    }

    #[test]
    fn real_engine_reopen_keeps_tagged_running_attempt_interrupted_without_recovery() {
        let f = fixture();
        let manifest = seed_fallback(&f);
        let raw = encode(&manifest).unwrap();
        fs::write(manifest.output_dir.join("initial-checkpoint.json"), &raw).unwrap();
        {
            let db = f.engine.db.lock().unwrap();
            db.execute(
                "UPDATE jobs SET state='running' WHERE id=?1",
                [manifest.job_id.as_str()],
            )
            .unwrap();
            db.execute(
                "UPDATE attempts SET state='running' WHERE id=?1",
                [manifest.attempt_id.as_str()],
            )
            .unwrap();
        }
        let Fixture {
            engine,
            session,
            project: _,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        let reopened = reopen_after_transient_leases(config).unwrap();
        assert_eq!(status(&reopened, &session, &manifest).state, "interrupted");
        let db = reopened.db.lock().unwrap();
        let attempt: (String, String) = db
            .query_row(
                "SELECT state,manifest FROM attempts WHERE id=?1",
                [manifest.attempt_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(attempt, ("interrupted".into(), raw));
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM job_attempt_outcomes WHERE attempt=?1",
                [manifest.attempt_id.as_str()],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(db);
        assert_no_successor(&reopened, &manifest);
        Engine::retire_for_test(reopened);
        drop(dir);
    }

    #[test]
    fn serialized_fallback_declaration_cannot_pass_publication_without_live_lease() {
        let f = fixture();
        let manifest = seed_fallback(&f);
        let db = f.engine.db.lock().unwrap();
        assert!(store::retained(&db, &manifest).unwrap().is_some());
        let error = f
            .engine
            .require_fallback_publication(&db, &manifest, None)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DependencyMismatch);
        assert!(error
            .message
            .contains("retained fallback history cannot authorize publication"));
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM candidates WHERE job=?1",
                [manifest.job_id.as_str()],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        drop(db);
        assert_eq!(status(&f.engine, &f.session, &manifest).state, "queued");
        assert_no_successor(&f.engine, &manifest);
    }

    #[test]
    fn failed_successor_admission_restores_job_failure_without_rewriting_predecessor() {
        let f = fixture();
        let manifest = seed_pending(&f);
        let before = immutable_attempt(&f.engine, &manifest);
        let refusal = ProtocolError::new(
            ErrorCode::Unsupported,
            "authored successor admission refused",
        );
        f.engine.fail_pending_fallback(&manifest, &refusal).unwrap();
        f.engine.fail_pending_fallback(&manifest, &refusal).unwrap();
        let current = status(&f.engine, &f.session, &manifest);
        assert_eq!(current.state, "failed");
        assert!(current
            .error
            .as_deref()
            .unwrap()
            .contains("automatic fallback refused: authored successor admission refused"));
        assert_eq!(immutable_attempt(&f.engine, &manifest), before);
        assert_no_successor(&f.engine, &manifest);
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    #[ignore = "requires actual Linux host observations and installed ffmpeg/ffprobe; hashes images but loads no native code"]
    fn public_missing_fallback_record_refuses_before_native_capture_or_job_creation() {
        use crate::cpu_owner_policy::{CpuOwnerPolicy, LoadedCpuOwnerPolicy};
        use std::os::unix::fs::PermissionsExt;

        struct PublicFallbackRefusalShutdown(Arc<Engine>);
        impl Drop for PublicFallbackRefusalShutdown {
            fn drop(&mut self) {
                self.0
                    .finish_shutdown()
                    .expect("join public fallback refusal fixture");
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let policy = CpuOwnerPolicy {
            schema_version: 1,
            native_dependencies: NativeDependencySpec {
                schema_version: 1,
                libraries: CPU_RUNTIME_REQUIRED_ALIASES
                    .iter()
                    .enumerate()
                    .map(|(index, alias)| {
                        // Authored identities only: these files are not executable libraries.
                        let bytes = format!("public refusal nonexecuted library {index}");
                        let path = dir.path().join(format!("library-{index}"));
                        fs::write(&path, &bytes).unwrap();
                        NativeLibrarySpec {
                            path,
                            sha256: format!("{:x}", Sha256::digest(bytes.as_bytes())),
                            byte_len: bytes.len() as u64,
                            names: vec![(*alias).into()],
                        }
                    })
                    .collect(),
            },
        };
        policy.validate().unwrap();
        let policy_path = dir.path().join("cpu-owner.json");
        fs::write(&policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
        fs::set_permissions(&policy_path, fs::Permissions::from_mode(0o600)).unwrap();
        let owner = LoadedCpuOwnerPolicy::load(&policy_path).unwrap();
        let worker = dir.path().join("nonexecuted-worker");
        fs::write(&worker, b"public refusal worker identity; never execute").unwrap();
        let mut config = EngineConfig::new(dir.path().join("state"), worker);
        config.cpu_owner_policy = Some(policy_path);
        config.max_ram_bytes = transfers::VALIDATION_MEMORY;
        let engine = Engine::open(config).unwrap();
        let _shutdown = PublicFallbackRefusalShutdown(engine.clone());
        let session = pair(&engine, "public fallback refusal fixture");
        let ResponseBody::Project(project) = engine
            .handle(request(
                &session,
                None,
                None,
                Command::CreateProject {
                    name: "public fallback refusal".into(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("project response")
        };
        let project = project.project_id;
        engine
            .handle(request(
                &session,
                None,
                None,
                Command::Grant {
                    session: session.clone(),
                    project_id: project.clone(),
                    scopes: vec![Scope::Generate],
                },
            ))
            .result
            .unwrap();
        let source_path = dir.path().join("source.fixture");
        fs::write(&source_path, b"public refusal media identity; never decode").unwrap();
        let ResponseBody::Source { source_version } = engine
            .handle(request(
                &session,
                Some(&project),
                None,
                Command::ImportSource { path: source_path },
            ))
            .result
            .unwrap()
        else {
            panic!("source response")
        };
        let model_path = dir.path().join("model.fixture");
        fs::write(&model_path, b"public refusal model identity; never load").unwrap();
        // Public preparation snapshots the model before examining evidence. Seed
        // that ordinary content snapshot; the assertions concern native capture.
        let model = engine.snapshot_model(&model_path).unwrap();
        let profile = DeclaredInferenceProfile {
            schema_version: 1,
            model: model.identity.clone(),
            runtime: owner.runtime_identity().unwrap(),
            preprocessing: InferencePreprocessing {
                input: ModelInputContract {
                    width: 64,
                    height: 64,
                    channels: 3,
                    layout: TensorLayout::Nchw,
                    class_count: 1,
                    decoder: DetectorDecoder::YoloChannelMajor,
                },
                resize: InferenceResize::Stretch,
                color: InferenceColor::Rgb,
                scale: vec![1.0 / 255.0; 3],
                bias: vec![0.0; 3],
                padding_byte: 0,
            },
            provider: InferenceProvider::Cpu,
            minimum_memory_bytes: 1,
            minimum_output_bytes: 1,
            cpu_threads: 1,
            cuda_resources: None,
        };
        let settings = GenerationSettings {
            model_input: Some(profile.preprocessing.input.clone()),
            inference_profile: Some(profile),
            ..GenerationSettings::default()
        };
        let selection = InferenceFallbackSelection {
            schema_version: 1,
            equivalents: vec![InferenceFallbackReference {
                dossier_id: ArtifactId::new("f".repeat(64)).unwrap(),
                expected_revision: 1,
                expected_report_sha256: "a".repeat(64),
            }],
            maximum_fallbacks: 1,
            source_policy: InferenceFallbackSourcePolicy {
                schema_version: 1,
                kind: InferenceFallbackSourceKind::DecodedMedia,
                sources: None,
                independent_audio: InferenceFallbackIndependentAudio::Any,
            },
            view_policy: InferenceFallbackViewPolicy {
                schema_version: 1,
                applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
            },
        };
        let snapshot = || {
            let db = engine.db.lock().unwrap();
            let header: i64 = db
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            let counts: (i64, i64, i64, i64) = db.query_row(
            "SELECT (SELECT count(*) FROM jobs),(SELECT count(*) FROM attempts),(SELECT count(*) FROM requests),(SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('cpu_runtime_reader','cpu_runtime_attempts','inference_fallback_reader','inference_fallback_jobs','inference_fallback_attempts','inference_fallback_pending','worker_fence_binding'))",
            [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
        ).unwrap();
            (header, counts)
        };
        let before = snapshot();
        assert_eq!((before.1 .0, before.1 .1), (0, 0));
        let fence = engine
            .worker_fence
            .staging_parent()
            .join("worker-fence.json");
        let fence_before = fs::read(&fence).unwrap();
        let error = engine
            .handle(request(
                &session,
                Some(&project),
                Some(0),
                Command::GenerateWithInferenceFallback {
                    source_version,
                    preset: "default".into(),
                    settings: Box::new(settings),
                    model: InferenceFallbackModel::Path { path: model.path },
                    selection: Box::new(selection),
                },
            ))
            .result
            .expect_err("missing equivalence cannot authorize generation");
        // NotFound proves observation completed and the actual registry was read;
        // an earlier tool/host observation error must fail this test, not satisfy it.
        assert_eq!(error.code, ErrorCode::NotFound);
        assert!(error.message.contains("equivalence dossier not found"));
        assert_eq!(snapshot(), before);
        assert_eq!(fs::read(&fence).unwrap(), fence_before);
        assert!(engine.workers.lock().unwrap().is_empty());
        assert!(!engine.worker_fence.has_pending_worker_custody().unwrap());
        assert!(!engine.worker_fence.has_pending_storage_custody().unwrap());
        assert_eq!(engine.pool.reserved_storage(), 0);
        let released_observation_memory = engine
            .pool
            .reserve_memory(transfers::VALIDATION_MEMORY)
            .unwrap();
        drop(released_observation_memory);
    }
}
