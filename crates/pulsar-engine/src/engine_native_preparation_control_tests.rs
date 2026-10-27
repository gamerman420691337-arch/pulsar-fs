mod native_preparation_control {
    use super::super::native_preparations::Entry;
    use super::*;
    use crate::native_preparation_db as journal;
    use std::time::{Duration, Instant};
    fn seed(f: &Fixture) -> (Request, AttemptId) {
        seed_with_identity(f, None)
    }
    fn seed_with_identity(f: &Fixture, identity: Option<(String, u64)>) -> (Request, AttemptId) {
        let request = request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::StartNativeImagePreparation {
                engine_epoch: AttemptId::new(f.engine.engine_epoch.clone()).unwrap(),
                options: NativeImagePreparationOptions {
                    preparation_ms: 1000,
                    retention_ms: 1000,
                },
            },
        );
        let operation = AttemptId::new("controller-test-operation").unwrap();
        let intent = NativeImagePreparationIntent {
            operation_id: operation.clone(),
            engine_epoch: AttemptId::new(f.engine.engine_epoch.clone()).unwrap(),
            owner: f.session.clone(),
            project: f.project.clone(),
            revision: 0,
            original_request_id: request.request_id.clone(),
            options: NativeImagePreparationOptions {
                preparation_ms: 1000,
                retention_ms: 1000,
            },
            image_sha256: identity
                .as_ref()
                .map_or_else(|| "a".repeat(64), |i| i.0.clone()),
            owner_policy_sha256: "b".repeat(64),
            declared_bytes: identity.map_or(4096, |i| i.1),
        };
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(request_fingerprint(&request).unwrap())
        );
        let mut db = f.engine.db.lock().unwrap();
        let tx = db.transaction().unwrap();
        f.engine.arm_worker_fence(&tx).unwrap();
        let queued = journal::begin(&tx, intent, &fingerprint).unwrap();
        let body = ResponseBody::NativeImagePreparation(Box::new(NativeImagePreparationStatus {
            report: queued.clone(),
            available: false,
            producer_active: true,
            observed_elapsed_ms: Some(0),
        }));
        remember(&tx, f.session.as_str(), &request, &fingerprint, &body).unwrap();
        let mut preparing = queued;
        preparing.generation = 1;
        preparing.state = NativeImagePreparationState::Preparing;
        journal::advance(&tx, &preparing).unwrap();
        tx.commit().unwrap();
        drop(db);
        let origin = Instant::now();
        let entry = Entry {
            request: request.clone(),
            origin,
            deadline: origin + Duration::from_secs(1),
            cancel: Arc::new(AtomicBool::new(false)),
            result: Arc::new(Mutex::new(None)),
            producer: None,
            memory: None,
            ready: None,
            retained: None,
        };
        f.engine
            .native_preparations
            .lock()
            .unwrap()
            .entries
            .insert(operation.to_string(), entry);
        (request, operation)
    }
    fn body(f: &Fixture, command: Command) -> NativeImagePreparationStatus {
        let ResponseBody::NativeImagePreparation(value) = f
            .engine
            .handle(request(&f.session, Some(&f.project), Some(0), command))
            .result
            .unwrap()
        else {
            panic!("status")
        };
        value.validate().unwrap();
        *value
    }
    #[test]
    fn native_preparation_control_limits_require_auth_and_current_revision() {
        let f = fixture();
        let mut query = request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::NativeImagePreparationLimits,
        );
        query.auth_token = None;
        assert_eq!(
            f.engine.handle(query).result.unwrap_err().code,
            ErrorCode::Unauthorized
        );
        let stale = request(
            &f.session,
            Some(&f.project),
            Some(1),
            Command::NativeImagePreparationLimits,
        );
        assert!(f.engine.handle(stale).result.is_err());
        let ResponseBody::NativeImagePreparationLimits(value) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::NativeImagePreparationLimits,
            ))
            .result
            .unwrap()
        else {
            panic!("limits")
        };
        value.validate().unwrap();
        assert!(!value.supported);
        assert_eq!(value.maximum_active, 8);
    }
    #[test]
    fn native_preparation_control_resolves_lost_start_and_replays_frozen_ack() {
        let f = fixture();
        let (start, operation) = seed(&f);
        let resolved = body(
            &f,
            Command::ResolveNativeImagePreparation {
                original_request_id: start.request_id.clone(),
            },
        );
        assert_eq!(resolved.report.intent.operation_id, operation);
        assert_eq!(
            resolved.report.state,
            NativeImagePreparationState::Preparing
        );
        let ResponseBody::NativeImagePreparation(ack) = f.engine.handle(start).result.unwrap()
        else {
            panic!("replay")
        };
        assert_eq!(ack.report.state, NativeImagePreparationState::Queued);
        assert_eq!(
            f.engine.native_preparations.lock().unwrap().entries.len(),
            1
        );
    }
    #[test]
    fn native_preparation_control_cancel_commits_then_confirms_cleanup() {
        let f = fixture();
        let (_, operation) = seed(&f);
        let cancel = request(
            &f.session,
            Some(&f.project),
            Some(0),
            Command::CancelNativeImagePreparation {
                operation_id: operation.clone(),
            },
        );
        let ResponseBody::NativeImagePreparation(ack) =
            f.engine.handle(cancel.clone()).result.unwrap()
        else {
            panic!("cancel")
        };
        assert_eq!(
            ack.report.state,
            NativeImagePreparationState::CancelRequested
        );
        assert!(
            f.engine.native_preparations.lock().unwrap().entries[operation.as_str()]
                .cancel
                .load(Ordering::Acquire)
        );
        let current = body(
            &f,
            Command::NativeImagePreparationStatus {
                operation_id: operation,
            },
        );
        assert_eq!(current.report.state, NativeImagePreparationState::Cancelled);
        assert!(current.report.cleanup_confirmed);
        assert!(!current.available);
        let ResponseBody::NativeImagePreparation(replayed) =
            f.engine.handle(cancel).result.unwrap()
        else {
            panic!("replay")
        };
        assert_eq!(*replayed, *ack);
    }
    #[test]
    fn native_preparation_control_rejects_other_owner_and_conflicting_request() {
        let f = fixture();
        let (mut start, operation) = seed(&f);
        let other = pair(&f.engine, "other");
        // Even a Read grant cannot transfer ownership of the retained use.
        f.engine
            .db
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO grants VALUES(?1,?2,?3)",
                params![other.as_str(), f.project.as_str(), scope_name(Scope::Read)],
            )
            .unwrap();
        let query = request(
            &other,
            Some(&f.project),
            Some(0),
            Command::NativeImagePreparationStatus {
                operation_id: operation,
            },
        );
        assert_eq!(
            f.engine.handle(query).result.unwrap_err().code,
            ErrorCode::Forbidden
        );
        start.command = Command::NativeImagePreparationLimits;
        assert_eq!(
            f.engine.handle(start).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
    }
    #[test]
    fn native_preparation_control_deadline_failure_does_not_renew_origin() {
        let f = fixture();
        let (_, operation) = seed(&f);
        f.engine
            .native_preparations
            .lock()
            .unwrap()
            .entries
            .get_mut(operation.as_str())
            .unwrap()
            .deadline = Instant::now() - Duration::from_millis(1);
        let current = body(
            &f,
            Command::NativeImagePreparationStatus {
                operation_id: operation,
            },
        );
        assert_eq!(current.report.state, NativeImagePreparationState::Failed);
        assert!(current.report.error.unwrap().contains("deadline"));
        assert!(current.report.cleanup_confirmed);
    }
    #[test]
    fn native_preparation_control_grant_revocation_cancels_original_authority() {
        let f = fixture();
        let (_, operation) = seed(&f);
        f.engine
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
        let current = body(
            &f,
            Command::NativeImagePreparationStatus {
                operation_id: operation,
            },
        );
        assert_eq!(current.report.state, NativeImagePreparationState::Failed);
        assert!(current.report.error.unwrap().contains("authorization"));
    }
    #[test]
    fn native_preparation_control_retains_and_releases_actual_file_custody() {
        let f = fixture();
        let image =
            crate::native_capture::authored_preparation_fixture(f.dir.path(), &f.engine.pool);
        let admission = super::super::live_cuda_preparation::RetentionAdmission::reserve(
            &f.engine.pool,
            f.engine.native_retention_slots.clone(),
        )
        .unwrap();
        let prepared = super::super::live_cuda_preparation::PreparedLiveCudaImage::retain(
            image,
            AttemptId::new(f.engine.engine_epoch.clone()).unwrap(),
            1000,
            admission,
        )
        .unwrap();
        let (_, operation) = seed_with_identity(
            &f,
            Some((prepared.image_sha256().into(), prepared.declared_bytes())),
        );
        {
            let mut registry = f.engine.native_preparations.lock().unwrap();
            let entry = registry.entries.get_mut(operation.as_str()).unwrap();
            // Fixture capture precedes durable test setup; use its original clock, never extend it.
            entry.origin = prepared.retention_started() - Duration::from_millis(1);
            entry.deadline = entry.origin + Duration::from_secs(1);
            entry.ready = Some(Ok(prepared));
        }
        let retained = body(
            &f,
            Command::NativeImagePreparationStatus {
                operation_id: operation.clone(),
            },
        );
        assert_eq!(retained.report.state, NativeImagePreparationState::Retained);
        assert!(retained.available);
        assert!(!retained.report.cleanup_confirmed);
        assert!(f.engine.pool.reserved_storage() > 0);
        let release = body(
            &f,
            Command::ReleaseNativeImagePreparation {
                operation_id: operation.clone(),
            },
        );
        assert_eq!(release.report.state, NativeImagePreparationState::Releasing);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let current = body(
                &f,
                Command::NativeImagePreparationStatus {
                    operation_id: operation.clone(),
                },
            );
            if current.report.cleanup_confirmed {
                assert_eq!(current.report.state, NativeImagePreparationState::Released);
                assert!(!current.available);
                break;
            }
            assert!(Instant::now() < deadline, "owned file cleanup must settle");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(f.engine.pool.reserved_storage(), 0);
    }
    #[test]
    fn native_preparation_control_waits_for_join_and_unrelated_storage_custody() {
        let f = fixture();
        let (_, operation) = seed(&f);
        let storage = f
            .engine
            .pool
            .reserve_memory(1024)
            .unwrap()
            .with_storage(4096, 32 * 1024 * 1024)
            .unwrap();
        let gate = Arc::new(AtomicBool::new(false));
        let worker_gate = gate.clone();
        let producer = std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(3);
            while !worker_gate.load(Ordering::Acquire) {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        f.engine
            .native_preparations
            .lock()
            .unwrap()
            .entries
            .get_mut(operation.as_str())
            .unwrap()
            .producer = Some(producer);
        let cancelled = body(
            &f,
            Command::CancelNativeImagePreparation {
                operation_id: operation.clone(),
            },
        );
        assert_eq!(
            cancelled.report.state,
            NativeImagePreparationState::CancelRequested
        );
        let active = body(
            &f,
            Command::NativeImagePreparationStatus {
                operation_id: operation.clone(),
            },
        );
        assert!(active.producer_active);
        assert!(!active.report.cleanup_confirmed);
        gate.store(true, Ordering::Release);
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let current = body(
                &f,
                Command::NativeImagePreparationStatus {
                    operation_id: operation.clone(),
                },
            );
            if !current.producer_active {
                assert_eq!(current.report.state, NativeImagePreparationState::Cancelled);
                assert!(!current.report.cleanup_confirmed);
                break;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(storage);
        let settled = body(
            &f,
            Command::NativeImagePreparationStatus {
                operation_id: operation,
            },
        );
        assert!(settled.report.cleanup_confirmed);
    }
    #[test]
    fn native_preparation_control_background_revocation_releases_monitor_admission() {
        let f = fixture();
        let (_, operation) = seed(&f);
        {
            let mut registry = f.engine.native_preparations.lock().unwrap();
            f.engine.start_preparation_monitor(&mut registry).unwrap();
        }
        assert!(f
            .engine
            .pool
            .reserve_memory(f.engine.config.max_ram_bytes)
            .is_err());
        f.engine
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
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let current = journal::load(&f.engine.db.lock().unwrap(), operation.as_str())
                .unwrap()
                .unwrap();
            if current.cleanup_confirmed {
                assert_eq!(current.state, NativeImagePreparationState::Failed);
                break;
            }
            assert!(
                Instant::now() < end,
                "background revocation must progress without RPC polling"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        loop {
            if f.engine
                .pool
                .reserve_memory(f.engine.config.max_ram_bytes)
                .is_ok()
            {
                break;
            }
            assert!(Instant::now() < end, "idle monitor must release admission");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn native_preparation_control_cancellation_discards_late_retained_file() {
        let f = fixture();
        let (_, operation) = seed(&f);
        let requested = body(
            &f,
            Command::CancelNativeImagePreparation {
                operation_id: operation.clone(),
            },
        );
        assert_eq!(
            requested.report.state,
            NativeImagePreparationState::CancelRequested
        );
        let image =
            crate::native_capture::authored_preparation_fixture(f.dir.path(), &f.engine.pool);
        let admission = super::super::live_cuda_preparation::RetentionAdmission::reserve(
            &f.engine.pool,
            f.engine.native_retention_slots.clone(),
        )
        .unwrap();
        let prepared = super::super::live_cuda_preparation::PreparedLiveCudaImage::retain(
            image,
            AttemptId::new(f.engine.engine_epoch.clone()).unwrap(),
            1000,
            admission,
        )
        .unwrap();
        f.engine
            .native_preparations
            .lock()
            .unwrap()
            .entries
            .get_mut(operation.as_str())
            .unwrap()
            .ready = Some(Ok(prepared));
        let current = body(
            &f,
            Command::NativeImagePreparationStatus {
                operation_id: operation.clone(),
            },
        );
        assert_eq!(current.report.state, NativeImagePreparationState::Cancelled);
        assert!(!current.available);
        assert!(current.report.retained_at_elapsed_ms.is_none());
        assert!(!current.report.cleanup_confirmed);
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let current = body(
                &f,
                Command::NativeImagePreparationStatus {
                    operation_id: operation.clone(),
                },
            );
            if current.report.cleanup_confirmed {
                assert_eq!(current.report.state, NativeImagePreparationState::Cancelled);
                break;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
