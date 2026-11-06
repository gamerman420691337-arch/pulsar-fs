//! Atomic accepted-attempt discovery or permanent original-ID withdrawal.
use super::*;
fn reservation(project: &ProjectId, reference: &WasmStartReference) -> PResult<String> {
    Ok(hash(
        &serde_json::to_vec(&(
            "pulsar-unpublished-wasm-start-withdrawal-v1",
            project,
            reference,
        ))
        .map_err(internal)?,
    ))
}
impl Engine {
    pub(super) fn resolve_wasm_start(
        &self,
        request: &Request,
        fingerprint: &str,
        owner: &SessionId,
    ) -> PResult<ResponseBody> {
        let Command::ResolveWasmStart { reference } = &request.command else {
            return Err(ProtocolError::invalid("not a start resolution"));
        };
        reference.validate()?;
        if reference.original_request_id == request.request_id {
            return Err(ProtocolError::invalid(
                "original and resolution IDs must differ",
            ));
        }
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        let present: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM revisions WHERE project=?1 AND revision=?2)",
                params![
                    project(request)?.as_str(),
                    rev_sql(reference.base_revision)?
                ],
                |r| r.get(0),
            )
            .map_err(internal)?;
        if !present {
            return Err(ProtocolError::new(
                ErrorCode::NotFound,
                "referenced project revision is absent",
            ));
        }
        let id: Option<String> = tx
            .query_row(
                "SELECT id FROM wasm_edits WHERE session=?1 AND request_id=?2 AND project=?3",
                params![
                    owner.as_str(),
                    reference.original_request_id.as_str(),
                    project(request)?.as_str()
                ],
                |r| r.get(0),
            )
            .optional()
            .map_err(internal)?;
        let outcome = if let Some(id) = id {
            let plan = read_plan(&tx, &AttemptId::new(id).map_err(internal)?)?;
            let observed = WasmStartReference {
                original_request_id: plan.request_id.clone(),
                base_revision: plan.base,
                manifest: plan.manifest.clone(),
                worker: plan.worker.clone(),
                guest_memory_bytes: plan.guest_memory_bytes,
                label: plan.label.clone(),
            };
            if !reference.matches(&observed)? {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "observed start metadata differs from retained invocation",
                ));
            }
            WasmStartOutcome::Accepted {
                report: (report(&tx, &plan)?).into(),
            }
        } else {
            let reserved = reservation(project(request)?, reference)?;
            let old: Option<String> = tx
                .query_row(
                    "SELECT fingerprint FROM request_identities WHERE session=?1 AND request=?2",
                    params![owner.as_str(), reference.original_request_id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(internal)?;
            if old.as_ref().is_some_and(|fp| fp != &reserved) {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "original request namespace is occupied; absence does not establish withdrawal",
                ));
            }
            // Reuse the global actor request namespace: existing readers already refuse changed fingerprints.
            // A distinct domain from typed request fingerprints prevents interpreting this as execution approval.
            tx.execute(
                "INSERT OR IGNORE INTO request_identities VALUES(?1,?2,?3)",
                params![
                    owner.as_str(),
                    reference.original_request_id.as_str(),
                    reserved
                ],
            )
            .map_err(internal)?;
            WasmStartOutcome::Withdrawn
        };
        let resolution = WasmStartResolution {
            project_id: project(request)?.clone(),
            reference: reference.clone(),
            outcome,
        };
        resolution.validate_for(request)?;
        let body = ResponseBody::WasmStartResolution((resolution).into());
        remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn withdrawal_is_atomic_namespace_bound_and_preserved_without_worker_or_edit() {
        let dir = tempfile::tempdir().unwrap();
        let config = EngineConfig::new(dir.path().join("state"), std::env::current_exe().unwrap());
        let engine = Engine::open(config.clone()).unwrap();
        let ResponseBody::Paired {
            session,
            auth_token,
        } = engine
            .handle(Request::new(
                id!(RequestId),
                Command::Pair {
                    client_name: "unpublished start fixture".into(),
                    pairing_token: engine.token.clone(),
                },
            ))
            .result
            .unwrap()
        else {
            panic!("pair");
        };
        let make = |command, project: Option<ProjectId>| Request {
            version: PROTOCOL_VERSION,
            request_id: id!(RequestId),
            session: Some(session.clone()),
            auth_token: Some(auth_token.clone()),
            project,
            expected_revision: None,
            command,
        };
        let create = make(
            Command::CreateProject {
                name: "withdrawal".into(),
            },
            None,
        );
        let ResponseBody::Project(before) = engine.handle(create.clone()).result.unwrap() else {
            panic!("project");
        };
        let project = before.project_id;
        let module = b"\0asm\x01\0\0\0".to_vec();
        let spec = WasmEditSpec {
            manifest: ExtensionManifest {
                schema_version: PROTOCOL_VERSION,
                name: "authored reference".into(),
                role: ExtensionRole::Editor,
                artifact: ArtifactIdentity {
                    artifact_id: ArtifactId::new("module").unwrap(),
                    sha256: hash(&module),
                    byte_len: module.len() as u64,
                },
                abi: WASM_PROCESSING_ABI.into(),
                requested_scopes: vec![],
                budget: ResourceBudget {
                    memory_bytes: 256 * 1024 * 1024,
                    output_bytes: WASM_BUFFER_MAX_BYTES as u64,
                    wall_time_ms: 5000,
                    cpu_threads: 2,
                },
                confinement: ConfinementRequirement::Wasm,
            },
            module,
            guest_memory_bytes: 65536,
            expected_worker: engine.binary_identity.clone(),
            approve_execution: true,
            label: "withdrawal fixture".into(),
        };
        let reference = WasmStartReference {
            original_request_id: id!(RequestId),
            base_revision: RevisionId::new(0),
            manifest: spec.manifest.clone(),
            worker: spec.expected_worker.clone(),
            guest_memory_bytes: spec.guest_memory_bytes,
            label: spec.label.clone(),
        };
        let resolve = make(
            Command::ResolveWasmStart {
                reference: reference.clone(),
            },
            Some(project.clone()),
        );
        engine.db.lock().unwrap().execute_batch(&format!("CREATE TRIGGER deny_resolution BEFORE INSERT ON requests WHEN NEW.request='{}' BEGIN SELECT RAISE(ABORT,'injected acknowledgement failure'); END;",resolve.request_id.as_str())).unwrap();
        assert!(engine.handle(resolve.clone()).result.is_err());
        let count: i64 = engine
            .db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM request_identities WHERE session=?1 AND request=?2",
                params![session.as_str(), reference.original_request_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        engine
            .db
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER deny_resolution")
            .unwrap();
        let ResponseBody::WasmStartResolution(resolution) =
            engine.handle(resolve.clone()).result.unwrap()
        else {
            panic!("resolve");
        };
        assert!(matches!(resolution.outcome, WasmStartOutcome::Withdrawn));
        let mut original = make(Command::StartWasmEdit { spec }, Some(project.clone()));
        original.request_id = reference.original_request_id.clone();
        original.expected_revision = Some(RevisionId::new(0));
        assert_eq!(
            engine.handle(original.clone()).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        let mut occupied = reference.clone();
        occupied.original_request_id = create.request_id;
        assert_eq!(
            engine
                .handle(make(
                    Command::ResolveWasmStart {
                        reference: occupied
                    },
                    Some(project.clone())
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        let mut changed = reference;
        changed.label = "changed".into();
        assert_eq!(
            engine
                .handle(make(
                    Command::ResolveWasmStart { reference: changed },
                    Some(project.clone())
                ))
                .result
                .unwrap_err()
                .code,
            ErrorCode::RequestConflict
        );
        assert!(engine.workers.lock().unwrap().is_empty());
        let count: i64 = engine
            .db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM wasm_edits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let snapshot = make(Command::GetSnapshot, Some(project));
        drop(engine);
        let reopened = Engine::open(config).unwrap();
        assert_eq!(
            reopened.handle(original).result.unwrap_err().code,
            ErrorCode::RequestConflict
        );
        assert!(matches!(
            reopened.handle(resolve).result.unwrap(),
            ResponseBody::WasmStartResolution(value) if matches!(value.outcome, WasmStartOutcome::Withdrawn)
        ));
        let ResponseBody::Project(after) = reopened.handle(snapshot).result.unwrap() else {
            panic!("snapshot");
        };
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.motion.program, before.motion.program);
    }
}
