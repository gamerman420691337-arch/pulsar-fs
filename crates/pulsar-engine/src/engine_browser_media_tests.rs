// Included in authority::tests. Synthetic bytes exercise custody and transport,
// not media decoding, source-clock calibration or physical synchronization.
#[cfg(unix)]
mod browser_media_engine_tests {
    use super::*;

    struct BsmFixture {
        f: Fixture,
        bytes: Vec<u8>,
        catalog: BrowserMediaCatalog,
        snapshot_path: PathBuf,
    }

    fn bsm_fixture() -> BsmFixture {
        let f = fixture();
        let bytes: Vec<u8> = (0..(2 * 65536 + 19)).map(|i| (i % 251) as u8).collect();
        let path = f.dir.path().join("browser-source.bin");
        fs::write(&path, &bytes).unwrap();
        let ResponseBody::Source { source_version } = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                None,
                Command::ImportSource { path },
            ))
            .result
            .unwrap()
        else {
            panic!("source import reply");
        };
        let spec = SourcePlacementSpec {
            source_version: source_version.clone(),
            source_start: SourceTimestamp::new(0, 1).unwrap(),
            source_end: SourceTimestamp::new(60, 1).unwrap(),
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            rate_numerator: 1,
            rate_denominator: 1,
        };
        f.engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(0),
                Command::SetSourcePlacement {
                    spec,
                    replace: None,
                },
            ))
            .result
            .unwrap();
        let ResponseBody::BrowserMediaCatalog(catalog) = f
            .engine
            .handle(request(
                &f.session,
                Some(&f.project),
                Some(1),
                Command::BrowserMediaCatalog,
            ))
            .result
            .unwrap()
        else {
            panic!("browser media catalog reply");
        };
        catalog.validate().unwrap();
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].selection.source_version, source_version);
        assert_eq!(
            catalog.entries[0].selection.source.byte_len,
            bytes.len() as u64
        );
        assert_eq!(
            catalog.entries[0].selection.source.sha256,
            format!("{:x}", Sha256::digest(&bytes))
        );
        let snapshot_path = f
            .engine
            .source(&f.engine.db.lock().unwrap(), &f.project, &source_version)
            .unwrap()
            .path;
        BsmFixture {
            f,
            bytes,
            catalog: *catalog,
            snapshot_path,
        }
    }

    fn bsm_prepare_request(
        x: &BsmFixture,
        actor: &SessionId,
        name: &str,
        lease_ms: u32,
    ) -> Request {
        let mut r = request(
            actor,
            Some(&x.f.project),
            Some(x.catalog.revision.value()),
            Command::PrepareBrowserMedia {
                engine_epoch: x.catalog.engine_epoch.clone(),
                selection: x.catalog.entries[0].selection.clone(),
                clock: DeclaredSourceClock {
                    presentation_anchor: SourceTimestamp::new(10, 1).unwrap(),
                    elapsed_anchor: SourceTimestamp::new(0, 1).unwrap(),
                },
                lease_ms,
                allow_declared_clock: true,
            },
        );
        r.request_id = RequestId::new(name).unwrap();
        r
    }

    fn bsm_report(body: ResponseBody) -> BrowserMediaPreparationReport {
        let ResponseBody::BrowserMediaPreparation(report) = body else {
            panic!("browser media preparation reply");
        };
        report.validate().unwrap();
        *report
    }

    fn bsm_status(x: &BsmFixture, original: &Request) -> BrowserMediaPreparationReport {
        let actor = original.session.as_ref().unwrap();
        bsm_report(
            x.f.engine
                .handle(request(
                    actor,
                    Some(&x.f.project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: original.request_id.clone(),
                    },
                ))
                .result
                .unwrap(),
        )
    }

    fn bsm_terminal(x: &BsmFixture, original: &Request) -> BrowserMediaPreparationReport {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let report = bsm_status(x, original);
            if report.state != BrowserMediaPreparationState::Pending {
                return report;
            }
            assert!(
                std::time::Instant::now() < end,
                "browser media preparation did not settle"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    fn bsm_ready(x: &BsmFixture, original: &Request) -> BrowserMediaBinding {
        let initial = bsm_report(x.f.engine.handle(original.clone()).result.unwrap());
        assert_eq!(initial.request_id, original.request_id);
        assert!(matches!(
            initial.state,
            BrowserMediaPreparationState::Pending | BrowserMediaPreparationState::Ready
        ));
        let report = if initial.state == BrowserMediaPreparationState::Ready {
            initial
        } else {
            bsm_terminal(x, original)
        };
        assert_eq!(
            report.state,
            BrowserMediaPreparationState::Ready,
            "{:?}",
            report.error
        );
        assert!(report.current);
        assert_eq!(report.verified_bytes, x.bytes.len() as u64);
        let binding = report.binding.unwrap();
        assert_eq!(binding.preparation_request_id, original.request_id);
        assert_eq!(binding.selection, x.catalog.entries[0].selection);
        binding
    }

    fn bsm_handshake(
        original: &Request,
        binding: &BrowserMediaBinding,
    ) -> SourceMediaBulkHandshake {
        SourceMediaBulkHandshake {
            version: PROTOCOL_VERSION,
            channel: SourceMediaBulkChannel::SourceMedia,
            session: original.session.clone().unwrap(),
            auth_token: original.auth_token.clone().unwrap(),
            lease_id: binding.lease_id.clone(),
            engine_epoch: binding.engine_epoch.clone(),
            preparation_request_id: binding.preparation_request_id.clone(),
            source_sha256: binding.selection.source.sha256.clone(),
        }
    }

    fn bsm_release(x: &BsmFixture, original: &Request) -> BrowserMediaPreparationReport {
        bsm_report(
            x.f.engine
                .handle(request(
                    original.session.as_ref().unwrap(),
                    Some(&x.f.project),
                    None,
                    Command::ReleaseBrowserMedia {
                        original_request_id: original.request_id.clone(),
                    },
                ))
                .result
                .unwrap(),
        )
    }

    fn bsm_grant(x: &BsmFixture, actor: &SessionId, scopes: Vec<Scope>) {
        x.f.engine
            .handle(request(
                &x.f.session,
                None,
                None,
                Command::Grant {
                    session: actor.clone(),
                    project_id: x.f.project.clone(),
                    scopes,
                },
            ))
            .result
            .unwrap();
    }

    fn bsm_wait(mut ready: impl FnMut() -> bool) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready() {
            assert!(
                std::time::Instant::now() < end,
                "browser media custody did not retire"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    fn bsm_capture_gate(x: &BsmFixture) -> (Arc<AtomicBool>, std::sync::mpsc::Receiver<()>) {
        let release = Arc::new(AtomicBool::new(false));
        let gate = release.clone();
        let (entered, receive) = std::sync::mpsc::sync_channel(4);
        x.f.engine
            .set_browser_media_capture_gate_for_test(Arc::new(move |cancel, deadline| {
                let _ = entered.try_send(());
                let end =
                    deadline.min(std::time::Instant::now() + std::time::Duration::from_secs(10));
                while !gate.load(Ordering::Acquire)
                    && !cancel.load(Ordering::Acquire)
                    && std::time::Instant::now() < end
                {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }));
        (release, receive)
    }

    #[test]
    fn browser_media_catalog_prepare_ranges_and_native_pin_hold_exact_source() {
        let x = bsm_fixture();
        let original = bsm_prepare_request(&x, &x.f.session, "bsm-read", 60_000);
        let binding = bsm_ready(&x, &original);
        let pin = x.f.engine.pin_browser_media(&original, &binding).unwrap();
        assert!(pin.current());
        let handshake = bsm_handshake(&original, &binding);
        let (mut connection, observed) = x.f.engine.source_media_bulk_open(&handshake).unwrap();
        assert_eq!(observed, binding);
        for (offset, length) in [(65530, 65536), (0, 19), (65530, 65536), (131072, 19)] {
            let bytes =
                x.f.engine
                    .source_media_bulk_read(&mut connection, &handshake, offset, length)
                    .unwrap();
            assert_eq!(
                bytes,
                x.bytes[offset as usize..offset as usize + length as usize]
            );
        }
        assert!(x
            .f
            .engine
            .package_holds
            .contains(&binding.selection.source.sha256)
            .unwrap());
        let released = bsm_release(&x, &original);
        assert!(!released.current);
        assert_eq!(released.state, BrowserMediaPreparationState::Ready);
        assert_eq!(released.binding.as_ref(), Some(&binding));
        assert!(!pin.current());
        assert!(x
            .f
            .engine
            .source_media_bulk_read(&mut connection, &handshake, 0, 1)
            .is_err());
        assert!(x
            .f
            .engine
            .package_holds
            .contains(&binding.selection.source.sha256)
            .unwrap());
        drop(connection);
        assert!(x
            .f
            .engine
            .package_holds
            .contains(&binding.selection.source.sha256)
            .unwrap());
        drop(pin);
        bsm_wait(|| {
            !x.f.engine
                .package_holds
                .contains(&binding.selection.source.sha256)
                .unwrap()
        });
        assert!(!bsm_status(&x, &original).current);
        assert!(!bsm_report(x.f.engine.handle(original).result.unwrap()).current);
        assert!(x.f.engine.source_media_bulk_open(&handshake).is_err());
    }

    #[test]
    fn browser_media_scope_authentication_and_revoke_regrant_cannot_revive_original_lease() {
        let x = bsm_fixture();
        let actor = pair(&x.f.engine, "browser media reader");
        bsm_grant(&x, &actor, vec![Scope::Read]);
        assert!(x
            .f
            .engine
            .handle(request(
                &actor,
                Some(&x.f.project),
                Some(1),
                Command::BrowserMediaCatalog
            ))
            .result
            .is_err());
        bsm_grant(&x, &actor, vec![Scope::Export]);
        let original = bsm_prepare_request(&x, &actor, "bsm-actor", 60_000);
        let binding = bsm_ready(&x, &original);
        let pin = x.f.engine.pin_browser_media(&original, &binding).unwrap();
        let handshake = bsm_handshake(&original, &binding);
        let (mut connection, _) = x.f.engine.source_media_bulk_open(&handshake).unwrap();
        let foreign = pair(&x.f.engine, "different authorized browser reader");
        bsm_grant(&x, &foreign, vec![Scope::Read, Scope::Export]);
        let credentials = request(&foreign, None, None, Command::Capabilities);
        let mut wrong = handshake.clone();
        wrong.session = foreign;
        wrong.auth_token = credentials.auth_token.unwrap();
        assert!(x.f.engine.source_media_bulk_open(&wrong).is_err());
        let mut wrong = handshake.clone();
        wrong.auth_token = "0".repeat(64);
        assert_eq!(
            x.f.engine
                .source_media_bulk_open(&wrong)
                .err()
                .unwrap()
                .code,
            ErrorCode::Unauthorized
        );
        x.f.engine
            .handle(request(
                &x.f.session,
                None,
                None,
                Command::Revoke {
                    session: actor.clone(),
                    project_id: x.f.project.clone(),
                },
            ))
            .result
            .unwrap();
        assert!(!pin.current());
        assert!(x
            .f
            .engine
            .source_media_bulk_read(&mut connection, &handshake, 0, 1)
            .is_err());
        bsm_grant(&x, &actor, vec![Scope::Read, Scope::Export]);
        assert!(!pin.current());
        assert!(x.f.engine.source_media_bulk_open(&handshake).is_err());
        assert!(!bsm_report(x.f.engine.handle(original.clone()).result.unwrap()).current);
        assert!(!bsm_status(&x, &original).current);
        let fresh = bsm_prepare_request(&x, &actor, "bsm-after-regrant", 60_000);
        assert_ne!(bsm_ready(&x, &fresh).lease_id, binding.lease_id);
    }

    #[test]
    fn browser_media_placement_replacement_retires_pin_and_refuses_stale_selection() {
        let x = bsm_fixture();
        let original = bsm_prepare_request(&x, &x.f.session, "bsm-placement", 60_000);
        let binding = bsm_ready(&x, &original);
        let pin = x.f.engine.pin_browser_media(&original, &binding).unwrap();
        let handshake = bsm_handshake(&original, &binding);
        let (mut connection, _) = x.f.engine.source_media_bulk_open(&handshake).unwrap();
        let prior = &binding.selection.placement;
        let mut spec = prior.spec.clone();
        spec.project_anchor = ProjectTime::from_nanos(1_000_000);
        x.f.engine
            .handle(request(
                &x.f.session,
                Some(&x.f.project),
                Some(1),
                Command::SetSourcePlacement {
                    spec,
                    replace: Some(prior.placement_id.clone()),
                },
            ))
            .result
            .unwrap();
        assert!(!pin.current());
        assert!(x
            .f
            .engine
            .source_media_bulk_read(&mut connection, &handshake, 0, 1)
            .is_err());
        assert!(!bsm_status(&x, &original).current);
        let mut stale = bsm_prepare_request(&x, &x.f.session, "bsm-stale-placement", 60_000);
        stale.expected_revision = Some(RevisionId::new(2));
        assert!(x.f.engine.handle(stale).result.is_err());
        let ResponseBody::BrowserMediaCatalog(next) =
            x.f.engine
                .handle(request(
                    &x.f.session,
                    Some(&x.f.project),
                    Some(2),
                    Command::BrowserMediaCatalog,
                ))
                .result
                .unwrap()
        else {
            panic!("replacement catalog");
        };
        assert_eq!(next.entries.len(), 1);
        assert_ne!(
            next.entries[0].selection.placement.placement_id,
            prior.placement_id
        );
        assert_eq!(next.entries[0].selection.source, binding.selection.source);
    }

    #[test]
    fn browser_media_changed_or_truncated_same_inode_is_refused_before_range_delivery() {
        use std::os::unix::fs::{FileExt, PermissionsExt};
        for change in 0..3 {
            let x = bsm_fixture();
            let original = bsm_prepare_request(&x, &x.f.session, "bsm-mutation", 60_000);
            let binding = bsm_ready(&x, &original);
            let handshake = bsm_handshake(&original, &binding);
            let (mut connection, _) = x.f.engine.source_media_bulk_open(&handshake).unwrap();
            fs::set_permissions(&x.snapshot_path, fs::Permissions::from_mode(0o600)).unwrap();
            let writer = fs::OpenOptions::new()
                .write(true)
                .open(&x.snapshot_path)
                .unwrap();
            let (offset, length) = match change {
                0 => {
                    writer.write_at(&[255], 20_000).unwrap();
                    (0, 1)
                }
                1 => {
                    writer.write_at(&[255], 65536 + 20_000).unwrap();
                    (65533, 6)
                }
                _ => {
                    writer.set_len(65536).unwrap();
                    (0, 1)
                }
            };
            let error =
                x.f.engine
                    .source_media_bulk_read(&mut connection, &handshake, offset, length)
                    .unwrap_err();
            assert_eq!(error.code, ErrorCode::DependencyMismatch);
            assert!(!error
                .message
                .contains(&x.snapshot_path.to_string_lossy().to_string()));
        }
    }

    #[test]
    fn browser_media_request_conflict_and_forged_selection_never_replace_preparation() {
        let x = bsm_fixture();
        let original = bsm_prepare_request(&x, &x.f.session, "bsm-exact-intent", 60_000);
        let binding = bsm_ready(&x, &original);
        let mut changed = original.clone();
        if let Command::PrepareBrowserMedia { lease_ms, .. } = &mut changed.command {
            *lease_ms += 1;
        }
        assert_eq!(
            x.f.engine.handle(changed).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        for change in 0..4 {
            let mut forged =
                bsm_prepare_request(&x, &x.f.session, &format!("bsm-forged-{change}"), 60_000);
            if let Command::PrepareBrowserMedia {
                engine_epoch,
                selection,
                allow_declared_clock,
                ..
            } = &mut forged.command
            {
                match change {
                    0 => selection.source.sha256 = "0".repeat(64),
                    1 => selection.source.byte_len += 1,
                    2 => *engine_epoch = AttemptId::new("different-engine-epoch").unwrap(),
                    _ => *allow_declared_clock = false,
                }
            }
            assert!(x.f.engine.handle(forged).result.is_err());
        }
        let report = bsm_status(&x, &original);
        assert!(report.current);
        assert_eq!(report.binding.as_ref(), Some(&binding));
    }

    #[test]
    fn browser_media_pending_cancel_and_capacity_are_bounded_without_hashing_races() {
        let x = bsm_fixture();
        let (gate, entered) = bsm_capture_gate(&x);
        let mut originals = Vec::new();
        for index in 0..4 {
            let original =
                bsm_prepare_request(&x, &x.f.session, &format!("bsm-pending-{index}"), 60_000);
            let report = bsm_report(x.f.engine.handle(original.clone()).result.unwrap());
            assert_eq!(report.state, BrowserMediaPreparationState::Pending);
            entered
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert_eq!(bsm_status(&x, &original).verified_bytes, 0);
            originals.push(original);
        }
        let excess = bsm_prepare_request(&x, &x.f.session, "bsm-pending-excess", 60_000);
        assert_eq!(
            x.f.engine.handle(excess).result.unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        for original in &originals {
            let report = bsm_release(&x, original);
            assert_eq!(report.state, BrowserMediaPreparationState::Cancelled);
            assert!(!report.current && report.binding.is_none());
        }
        gate.store(true, Ordering::Release);
        for original in originals {
            let report = bsm_terminal(&x, &original);
            assert_eq!(report.state, BrowserMediaPreparationState::Cancelled);
            let replay = bsm_report(x.f.engine.handle(original).result.unwrap());
            assert_eq!(replay.state, BrowserMediaPreparationState::Cancelled);
            assert!(!replay.current && replay.binding.is_none());
        }
        bsm_wait(|| {
            !x.f.engine
                .package_holds
                .contains(&x.catalog.entries[0].selection.source.sha256)
                .unwrap()
        });
    }

    #[test]
    fn browser_media_connection_and_live_lease_capacity_release_without_replacing_ids() {
        let x = bsm_fixture();
        let mut originals = Vec::new();
        let mut bindings = Vec::new();
        for index in 0..8 {
            let original =
                bsm_prepare_request(&x, &x.f.session, &format!("bsm-lease-{index}"), 60_000);
            bindings.push(bsm_ready(&x, &original));
            originals.push(original);
        }
        let excess = bsm_prepare_request(&x, &x.f.session, "bsm-lease-excess", 60_000);
        assert_eq!(
            x.f.engine.handle(excess).result.unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        let first = bsm_handshake(&originals[0], &bindings[0]);
        let (one, _) = x.f.engine.source_media_bulk_open(&first).unwrap();
        let (two, _) = x.f.engine.source_media_bulk_open(&first).unwrap();
        assert_eq!(
            x.f.engine
                .source_media_bulk_open(&first)
                .err()
                .unwrap()
                .code,
            ErrorCode::ResourceExhausted
        );
        let mut connections = vec![one, two];
        for index in 1..4 {
            let h = bsm_handshake(&originals[index], &bindings[index]);
            connections.push(x.f.engine.source_media_bulk_open(&h).unwrap().0);
            connections.push(x.f.engine.source_media_bulk_open(&h).unwrap().0);
        }
        let fifth = bsm_handshake(&originals[4], &bindings[4]);
        assert_eq!(
            x.f.engine
                .source_media_bulk_open(&fifth)
                .err()
                .unwrap()
                .code,
            ErrorCode::ResourceExhausted
        );
        connections.pop();
        let (replacement_connection, observed) = x.f.engine.source_media_bulk_open(&fifth).unwrap();
        assert_eq!(observed, bindings[4]);
        drop(replacement_connection);
        drop(connections);
        bsm_release(&x, &originals[0]);
        let replacement = bsm_prepare_request(&x, &x.f.session, "bsm-after-release", 60_000);
        assert_ne!(bsm_ready(&x, &replacement).lease_id, bindings[0].lease_id);
        assert!(x.f.engine.source_media_bulk_open(&first).is_err());
    }

    #[test]
    fn browser_media_exhausted_memory_cannot_publish_a_lease() {
        let x = bsm_fixture();
        let held =
            x.f.engine
                .pool
                .reserve_memory(x.f.engine.config.max_ram_bytes)
                .unwrap();
        let original = bsm_prepare_request(&x, &x.f.session, "bsm-memory", 60_000);
        match x.f.engine.handle(original.clone()).result {
            Err(error) => assert_eq!(error.code, ErrorCode::ResourceExhausted),
            Ok(body) => {
                let initial = bsm_report(body);
                assert!(!matches!(
                    initial.state,
                    BrowserMediaPreparationState::Ready
                ));
                let report = bsm_terminal(&x, &original);
                assert_eq!(report.state, BrowserMediaPreparationState::Failed);
                assert!(!report.current && report.binding.is_none());
            }
        }
        drop(held);
        let fresh = bsm_prepare_request(&x, &x.f.session, "bsm-memory-fresh", 60_000);
        bsm_ready(&x, &fresh);
    }

    #[test]
    fn browser_media_restart_preserves_history_but_never_reconstructs_native_lease() {
        let x = bsm_fixture();
        let original = bsm_prepare_request(&x, &x.f.session, "bsm-restart", 60_000);
        let binding = bsm_ready(&x, &original);
        let pin = x.f.engine.pin_browser_media(&original, &binding).unwrap();
        let handshake = bsm_handshake(&original, &binding);
        let BsmFixture {
            f,
            bytes: _,
            catalog: _,
            snapshot_path: _,
        } = x;
        let Fixture {
            engine,
            session,
            project,
            dir,
        } = f;
        let config = Engine::retire_for_test(engine);
        assert!(!pin.current());
        drop(pin);
        let engine = Engine::open(config).unwrap();
        let report = bsm_report(
            engine
                .handle(request(
                    &session,
                    Some(&project),
                    None,
                    Command::BrowserMediaPreparationStatus {
                        original_request_id: original.request_id.clone(),
                    },
                ))
                .result
                .unwrap(),
        );
        assert_eq!(report.state, BrowserMediaPreparationState::Ready);
        assert_eq!(report.binding.as_ref(), Some(&binding));
        assert!(!report.current);
        assert!(engine.source_media_bulk_open(&handshake).is_err());
        if let Ok(body) = engine.handle(original).result {
            assert!(!bsm_report(body).current);
        }
        assert!(engine.source_media_bulk_open(&handshake).is_err());
        Engine::retire_for_test(engine);
        drop(dir);
    }

    #[test]
    fn browser_media_absolute_expiry_does_not_refresh_through_status_or_original_replay() {
        let x = bsm_fixture();
        let original = bsm_prepare_request(&x, &x.f.session, "bsm-expiry", 1);
        bsm_report(x.f.engine.handle(original.clone()).result.unwrap());
        let ready = bsm_terminal(&x, &original);
        assert_eq!(ready.state, BrowserMediaPreparationState::Ready);
        let binding = ready.binding.unwrap();
        bsm_wait(|| !bsm_status(&x, &original).current);
        let replay = bsm_report(x.f.engine.handle(original.clone()).result.unwrap());
        assert!(!replay.current);
        assert_eq!(replay.binding.as_ref(), Some(&binding));
        assert!(x
            .f
            .engine
            .source_media_bulk_open(&bsm_handshake(&original, &binding))
            .is_err());
    }

    #[test]
    fn browser_media_verification_unwind_retires_pending_custody_and_preserves_failure() {
        let x = bsm_fixture();
        x.f.engine
            .set_browser_media_capture_gate_for_test(Arc::new(|_, _| {
                panic!("synthetic source-media verification unwind");
            }));
        let original = bsm_prepare_request(&x, &x.f.session, "bsm-verification-unwind", 60_000);
        x.f.engine.handle(original.clone()).result.unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let report = bsm_status(&x, &original);
            if report.state == BrowserMediaPreparationState::Failed {
                assert!(!report.current);
                assert!(report.binding.is_none());
                assert_eq!(
                    report.error.as_deref(),
                    Some(BROWSER_MEDIA_PREPARATION_ERROR)
                );
                break;
            }
            assert!(
                std::time::Instant::now() < end,
                "panicked verification remained Pending and retained its admission slot"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        bsm_wait(|| {
            !x.f.engine
                .package_holds
                .contains(&x.catalog.entries[0].selection.source.sha256)
                .unwrap()
        });
        // All four pending admissions remain usable after the failed task is
        // reaped. No replacement of its original request is required.
        let (gate, entered) = bsm_capture_gate(&x);
        let mut pending = Vec::new();
        for n in 0..4 {
            let request =
                bsm_prepare_request(&x, &x.f.session, &format!("bsm-after-unwind-{n}"), 60_000);
            assert_eq!(
                bsm_report(x.f.engine.handle(request.clone()).result.unwrap()).state,
                BrowserMediaPreparationState::Pending
            );
            entered
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            pending.push(request);
        }
        for request in &pending {
            assert_eq!(
                bsm_release(&x, request).state,
                BrowserMediaPreparationState::Cancelled
            );
        }
        gate.store(true, Ordering::Release);
        // Cleanup must retain the original worker-panic diagnostic for the
        // owned task group's final join instead of pretending the task passed.
        assert!(x.f.engine.finish_shutdown().is_err());
    }

    #[test]
    fn browser_media_failed_ready_write_repairs_history_without_installing_authority() {
        let x = bsm_fixture();
        let (gate, entered) = bsm_capture_gate(&x);
        x.f.engine
            .db
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER refuse_browser_media_ready BEFORE UPDATE ON requests
             WHEN NEW.request='bsm-ready-write-failure'
               AND json_extract(NEW.response,'$.kind')='browser_media_preparation'
               AND json_extract(NEW.response,'$.value.state')='ready'
             BEGIN SELECT RAISE(ABORT,'synthetic browser media receipt failure'); END;",
            )
            .unwrap();
        let original = bsm_prepare_request(&x, &x.f.session, "bsm-ready-write-failure", 60_000);
        let initial = bsm_report(x.f.engine.handle(original.clone()).result.unwrap());
        assert_eq!(initial.state, BrowserMediaPreparationState::Pending);
        entered
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        gate.store(true, Ordering::Release);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let result =
                x.f.engine
                    .handle(request(
                        &x.f.session,
                        Some(&x.f.project),
                        None,
                        Command::BrowserMediaPreparationStatus {
                            original_request_id: original.request_id.clone(),
                        },
                    ))
                    .result;
            match result {
                Ok(body) => {
                    let report = bsm_report(body);
                    assert_eq!(report.state, BrowserMediaPreparationState::Pending);
                    assert!(report.binding.is_none());
                    if !report.current {
                        break;
                    }
                }
                Err(error) => {
                    assert_eq!(error.code, ErrorCode::Unavailable);
                    break;
                }
            }
            assert!(
                std::time::Instant::now() < end,
                "verification did not reach the refused Ready receipt"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        bsm_wait(|| {
            !x.f.engine
                .package_holds
                .contains(&x.catalog.entries[0].selection.source.sha256)
                .unwrap()
        });
        {
            let db = x.f.engine.db.lock().unwrap();
            let response: String = db
                .query_row(
                    "SELECT response FROM requests WHERE session=?1 AND request=?2",
                    params![x.f.session.as_str(), original.request_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            let report = bsm_report(serde_json::from_str(&response).unwrap());
            assert_eq!(report.state, BrowserMediaPreparationState::Pending);
            assert!(report.binding.is_none());
            db.execute_batch("DROP TRIGGER refuse_browser_media_ready")
                .unwrap();
        }
        let repaired = bsm_terminal(&x, &original);
        assert_eq!(repaired.state, BrowserMediaPreparationState::Ready);
        assert!(!repaired.current);
        let binding = repaired.binding.unwrap();
        assert!(x.f.engine.pin_browser_media(&original, &binding).is_err());
        assert!(x
            .f
            .engine
            .source_media_bulk_open(&bsm_handshake(&original, &binding))
            .is_err());
        let replay = bsm_report(x.f.engine.handle(original).result.unwrap());
        assert!(!replay.current);
        assert_eq!(replay.binding.as_ref(), Some(&binding));
        let fresh = bsm_prepare_request(&x, &x.f.session, "bsm-after-ready-write-failure", 60_000);
        assert_ne!(bsm_ready(&x, &fresh).lease_id, binding.lease_id);
    }
    fn bsm_successor_request(
        x: &BsmFixture,
        predecessor: &BrowserMediaBinding,
        name: &str,
    ) -> Request {
        let mut r = request(
            &x.f.session,
            Some(&x.f.project),
            Some(predecessor.revision.value()),
            Command::PrepareBrowserMediaSuccessor {
                predecessor: predecessor.clone(),
                lease_ms: 60_000,
            },
        );
        r.request_id = RequestId::new(name).unwrap();
        r
    }
    #[test]
    fn browser_media_successor_is_exact_single_use_native_lineage_with_separate_retirement() {
        let x = bsm_fixture();
        let original = bsm_prepare_request(&x, &x.f.session, "media-lineage-a", 60_000);
        let old = bsm_ready(&x, &original);
        let child = bsm_successor_request(&x, &old, "media-lineage-b");
        let new = bsm_ready(&x, &child);
        assert_eq!(old.selection, new.selection);
        assert_eq!(old.clock, new.clock);
        assert_ne!(old.lease_id, new.lease_id);
        let proof =
            x.f.engine
                .capture_browser_media_successor_locked(
                    &x.f.engine.db.lock().unwrap(),
                    &child,
                    &old,
                    &new,
                )
                .unwrap();
        assert_eq!(proof.original_request_id(), &child.request_id);
        assert!(proof.current());
        let duplicate = bsm_successor_request(&x, &old, "media-lineage-fork");
        assert!(x.f.engine.handle(duplicate).result.is_err());
        let premature = bsm_successor_request(&x, &new, "media-lineage-premature-grandchild");
        assert!(x.f.engine.handle(premature).result.is_err());
        // A pending preparation or its Ready report never changes the parent.
        assert!(bsm_status(&x, &original).current);
        proof.commit().unwrap();
        assert!(proof.commit().is_err());
        assert!(x
            .f
            .engine
            .capture_browser_media_successor_locked(
                &x.f.engine.db.lock().unwrap(),
                &child,
                &old,
                &new
            )
            .is_err());
        let new_pin = proof.successor().clone();
        let old_pin = proof.predecessor().clone();
        bsm_release(&x, &original);
        assert!(!old_pin.current());
        assert!(new_pin.current());
        assert!(
            !proof.current(),
            "a transfer proof is not the successor's active lifetime"
        );
        let handshake = bsm_handshake(&child, &new);
        let (mut connection, actual) = x.f.engine.source_media_bulk_open(&handshake).unwrap();
        assert_eq!(actual, new);
        assert_eq!(
            x.f.engine
                .source_media_bulk_read(&mut connection, &handshake, 65530, 100)
                .unwrap(),
            x.bytes[65530..65630]
        );
        let next = bsm_successor_request(&x, &new, "media-lineage-c");
        assert!(bsm_ready(&x, &next).selection == old.selection);
        bsm_release(&x, &child);
        assert!(!new_pin.current());
        assert!(
            !bsm_status(&x, &next).current,
            "uncommitted grandchild cannot outlive its parent"
        );
    }
    #[test]
    fn browser_media_staged_successor_cannot_survive_parent_release_or_be_forged_from_ordinary_prepare(
    ) {
        let x = bsm_fixture();
        let original = bsm_prepare_request(&x, &x.f.session, "media-uncommitted-a", 60_000);
        let old = bsm_ready(&x, &original);
        let unrelated = bsm_prepare_request(&x, &x.f.session, "media-ordinary-b", 60_000);
        let ordinary = bsm_ready(&x, &unrelated);
        assert!(x
            .f
            .engine
            .capture_browser_media_successor_locked(
                &x.f.engine.db.lock().unwrap(),
                &unrelated,
                &old,
                &ordinary
            )
            .is_err());
        let child = bsm_successor_request(&x, &old, "media-uncommitted-b");
        let new = bsm_ready(&x, &child);
        let proof =
            x.f.engine
                .capture_browser_media_successor_locked(
                    &x.f.engine.db.lock().unwrap(),
                    &child,
                    &old,
                    &new,
                )
                .unwrap();
        let mut tampered = new.clone();
        tampered.clock.presentation_anchor = SourceTimestamp::new(11, 1).unwrap();
        assert!(x
            .f
            .engine
            .capture_browser_media_successor_locked(
                &x.f.engine.db.lock().unwrap(),
                &child,
                &old,
                &tampered
            )
            .is_err());
        bsm_release(&x, &original);
        assert!(!proof.successor().current());
        assert!(proof.commit().is_err());
        assert!(!bsm_report(x.f.engine.handle(child).result.unwrap()).current);
        assert!(x
            .f
            .engine
            .pin_browser_media(&unrelated, &ordinary)
            .unwrap()
            .current());
    }
}
