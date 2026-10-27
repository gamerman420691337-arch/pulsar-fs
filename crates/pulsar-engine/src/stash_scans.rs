//! Durable remote scan dispatch. Never resubmit an unknown acknowledgement.
use super::*;

impl Engine {
    pub(super) fn stash_scan_control(
        &self,
        request: &Request,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let _admission = self.pool.reserve_memory(16 * 1024 * 1024)?;
        let session = request.session.as_ref().expect("authenticated");
        match &request.command {
            Command::StashSubmitScan {
                connection,
                publication_request_id,
                ..
            } => {
                let publication = {
                    let db = self.db.lock().map_err(internal)?;
                    self.authorize(&db, request, session)?;
                    if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
                        return Ok(body);
                    }
                    let existing:Option<String>=db.query_row("SELECT receipt FROM stash_scans WHERE session=?1 AND (request=?2 OR publication=?3)",params![session.as_str(),request.request_id.as_str(),publication_request_id.as_str()],|r|r.get(0)).optional().map_err(internal)?;
                    if let Some(raw) = existing {
                        let receipt: StashScanReceipt = decode(&raw)?;
                        if receipt.request_id != request.request_id {
                            return Err(ProtocolError::new(ErrorCode::RequestConflict,"publication already has a scan dispatch; reconcile its recorded request ID"));
                        }
                        return Ok(ResponseBody::StashScan((receipt).into()));
                    }
                    check_revision(&db, request)?;
                    let raw:Option<String>=db.query_row("SELECT r.response FROM requests r JOIN exports e ON e.session=r.session AND e.request=r.request WHERE r.session=?1 AND r.request=?2 AND e.project=?3 AND e.state='complete'",params![session.as_str(),publication_request_id.as_str(),project(request)?.as_str()],|r|r.get(0)).optional().map_err(internal)?;
                    let body:ResponseBody=decode(&raw.ok_or_else(||ProtocolError::new(ErrorCode::NotFound,"successful Stash publication receipt is absent for this session/project"))?)?;
                    let ResponseBody::StashScriptExported(publication) = body else {
                        return Err(ProtocolError::invalid(
                            "scan requires a Stash publication receipt",
                        ));
                    };
                    publication
                };
                check_origin(connection, &publication)?;
                crate::stash_files::verify_published(&publication)?;
                let scene = crate::stash::scene(connection, &publication.source.scene_id)?;
                if !same_scene(&scene, &publication) {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "Stash selected file changed before scan",
                    ));
                }
                let mut receipt = StashScanReceipt {
                    request_id: request.request_id.clone(),
                    publication_request_id: publication_request_id.clone(),
                    project_id: project(request)?.clone(),
                    publication: *publication,
                    remote_job_id: None,
                    state: StashScanState::DispatchUnknown,
                    attachment: None,
                    release_qualified: false,
                };
                {
                    let mut db = self.db.lock().map_err(internal)?;
                    let tx = db.transaction().map_err(internal)?;
                    self.authorize(&tx, request, session)?;
                    check_revision(&tx, request)?;
                    if let Some(body) = replay(&tx, session.as_str(), request, fingerprint)? {
                        return Ok(body);
                    }
                    claim_request_identity(&tx, session.as_str(), request, fingerprint)?;
                    let inserted=tx.execute("INSERT OR IGNORE INTO stash_scans(session,request,publication,project,receipt) VALUES(?1,?2,?3,?4,?5)",params![session.as_str(),request.request_id.as_str(),publication_request_id.as_str(),project(request)?.as_str(),encode(&receipt)?]).map_err(internal)?;
                    if inserted != 1 {
                        return Err(ProtocolError::new(
                            ErrorCode::RequestConflict,
                            "scan dispatch already exists; reconcile before resubmission",
                        ));
                    }
                    // Commit is the dispatch authorization point, just as for file export.
                    tx.commit().map_err(internal)?;
                }
                if let Ok(job) =
                    crate::stash::submit_scan(connection, &receipt.publication.source.server_path)
                {
                    receipt.remote_job_id = Some(job);
                    receipt.state = StashScanState::Submitted;
                }
                // Every transport/GraphQL/acknowledgement failure stays unknown:
                // the remote server may have accepted the mutation.
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db.transaction().map_err(internal)?;
                // A concurrent query-only observation may have completed while
                // the mutation acknowledgement was in flight. Preserve it.
                let current: String = tx
                    .query_row(
                        "SELECT receipt FROM stash_scans WHERE session=?1 AND request=?2",
                        params![session.as_str(), request.request_id.as_str()],
                        |r| r.get(0),
                    )
                    .map_err(internal)?;
                receipt.attachment = decode::<StashScanReceipt>(&current)?.attachment;
                tx.execute(
                    "UPDATE stash_scans SET receipt=?3 WHERE session=?1 AND request=?2",
                    params![
                        session.as_str(),
                        request.request_id.as_str(),
                        encode(&receipt)?
                    ],
                )
                .map_err(internal)?;
                let body = ResponseBody::StashScan((receipt).into());
                remember(&tx, session.as_str(), request, fingerprint, &body)?;
                tx.commit().map_err(internal)?;
                Ok(body)
            }
            Command::StashScanStatus {
                connection,
                scan_request_id,
                ..
            }
            | Command::StashReconcileAttachment {
                connection,
                scan_request_id,
                ..
            } => {
                let raw = {
                    let db = self.db.lock().map_err(internal)?;
                    self.authorize(&db, request, session)?;
                    db.query_row("SELECT receipt FROM stash_scans WHERE session=?1 AND request=?2 AND project=?3",params![session.as_str(),scan_request_id.as_str(),project(request)?.as_str()],|r|r.get::<_,String>(0)).optional().map_err(internal)?.ok_or_else(||ProtocolError::new(ErrorCode::NotFound,"scan dispatch is absent for this session/project"))?
                };
                let mut receipt: StashScanReceipt = decode(&raw)?;
                check_origin(connection, &receipt.publication)?;
                if matches!(&request.command, Command::StashReconcileAttachment { .. }) {
                    let scene =
                        crate::stash::scene(connection, &receipt.publication.source.scene_id)?;
                    let state = if !same_scene(&scene, &receipt.publication) {
                        StashAttachmentState::IdentityChanged
                    } else if crate::stash_files::verify_published(&receipt.publication).is_err() {
                        StashAttachmentState::LocalPublicationUnverified
                    } else if scene.interactive {
                        StashAttachmentState::ObservedLinked
                    } else {
                        StashAttachmentState::Unattached
                    };
                    if state != StashAttachmentState::ObservedLinked
                        && receipt.state == StashScanState::VerifiedLinked
                    {
                        receipt.state = StashScanState::FinishedUnattached;
                    }
                    receipt.attachment = Some(observation(state)?);
                    // Observation cannot recover a job or prove scan completion.
                    // A negative observation invalidates earlier linked status.
                } else {
                    let Some(job) = &receipt.remote_job_id else {
                        return Ok(ResponseBody::StashScan((receipt).into()));
                    };
                    receipt.state = crate::stash::scan_state(connection, job)?;
                    if receipt.state == StashScanState::FinishedUnattached {
                        let scene =
                            crate::stash::scene(connection, &receipt.publication.source.scene_id)?;
                        if scene.interactive && same_scene(&scene, &receipt.publication) {
                            crate::stash_files::verify_published(&receipt.publication)?;
                            receipt.state = StashScanState::VerifiedLinked;
                            receipt.attachment =
                                Some(observation(StashAttachmentState::ObservedLinked)?);
                        } else {
                            receipt.attachment =
                                Some(observation(if same_scene(&scene, &receipt.publication) {
                                    StashAttachmentState::Unattached
                                } else {
                                    StashAttachmentState::IdentityChanged
                                })?);
                        }
                    }
                }
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db.transaction().map_err(internal)?;
                self.authorize(&tx, request, session)?;
                // An older concurrent observation cannot replace a newer receipt.
                let changed=tx.execute("UPDATE stash_scans SET receipt=?4 WHERE session=?1 AND request=?2 AND receipt=?3",params![session.as_str(),scan_request_id.as_str(),raw,encode(&receipt)?]).map_err(internal)?;
                if changed == 0 {
                    let current: String = tx
                        .query_row(
                            "SELECT receipt FROM stash_scans WHERE session=?1 AND request=?2",
                            params![session.as_str(), scan_request_id.as_str()],
                            |r| r.get(0),
                        )
                        .map_err(internal)?;
                    receipt = decode(&current)?;
                }
                tx.commit().map_err(internal)?;
                Ok(ResponseBody::StashScan((receipt).into()))
            }
            _ => Err(ProtocolError::invalid("unexpected Stash scan command")),
        }
    }
}
fn check_origin(connection: &StashConnection, publication: &StashScriptExport) -> PResult<()> {
    if publication.source.endpoint.as_deref()
        != Some(crate::stash::endpoint_identity(connection)?.as_str())
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "scan origin differs from publication",
        ));
    }
    Ok(())
}
fn same_scene(scene: &StashScene, publication: &StashScriptExport) -> bool {
    scene.id == publication.source.scene_id
        && scene.files.len() == 1
        && scene.files[0].id == publication.source.file_id
        && scene.files[0].path == publication.source.server_path
}

fn observation(state: StashAttachmentState) -> PResult<StashAttachmentObservation> {
    let observed_at_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(internal)?
        .as_millis()
        .try_into()
        .map_err(internal)?;
    Ok(StashAttachmentObservation {
        state,
        observed_at_unix_ms,
        remote_script_hash_verified: false,
    })
}
