//! Separate durable creation/attachment outcomes. Never replace a scene tag set.
use super::*;
use crate::stash::TagBroker;

impl Engine {
    pub(super) fn stash_tag_control(
        &self,
        request: &Request,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        let _admission = self.pool.reserve_memory(16 * 1024 * 1024)?;
        let session = request.session.as_ref().expect("authenticated");
        match &request.command {
            Command::StashAddTag {
                connection,
                scan_request_id,
                tag_name,
                create,
                ..
            } => {
                let scan = {
                    let db = self.db.lock().map_err(internal)?;
                    self.authorize(&db, request, session)?;
                    if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
                        return Ok(body);
                    }
                    let old:Option<String>=db.query_row("SELECT receipt FROM stash_tags WHERE session=?1 AND (request=?2 OR (scan=?3 AND tag_name=?4))",params![session.as_str(),request.request_id.as_str(),scan_request_id.as_str(),tag_name],|r|r.get(0)).optional().map_err(internal)?;
                    if let Some(raw) = old {
                        let receipt: StashTagReceipt = decode(&raw)?;
                        if receipt.request_id != request.request_id {
                            return Err(ProtocolError::new(
                                ErrorCode::RequestConflict,
                                "tag dispatch exists; reconcile or resume its recorded request ID",
                            ));
                        }
                        return Ok(ResponseBody::StashTag((receipt).into()));
                    }
                    check_revision(&db, request)?;
                    let raw:String=db.query_row("SELECT receipt FROM stash_scans WHERE session=?1 AND request=?2 AND project=?3",params![session.as_str(),scan_request_id.as_str(),project(request)?.as_str()],|r|r.get(0)).optional().map_err(internal)?.ok_or_else(||ProtocolError::new(ErrorCode::NotFound,"scan receipt is absent for this session/project"))?;
                    decode::<StashScanReceipt>(&raw)?
                };
                if scan.state != StashScanState::VerifiedLinked {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "tagging requires independently verified scan attachment",
                    ));
                }
                let job = scan.remote_job_id.ok_or_else(|| {
                    ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "verified scan lacks remote job identity",
                    )
                })?;
                let broker = TagBroker::new(connection)?;
                verify_link(connection, &broker, &scan.publication, &job)?;
                let tag_id = broker.lookup(tag_name)?;
                if tag_id.is_none() && !create {
                    return Err(ProtocolError::new(
                        ErrorCode::NotFound,
                        "exact Stash tag is absent; creation was not requested",
                    ));
                }
                let mut receipt = StashTagReceipt {
                    request_id: request.request_id.clone(),
                    scan_request_id: scan_request_id.clone(),
                    project_id: project(request)?.clone(),
                    publication: scan.publication,
                    remote_job_id: job,
                    tag_name: tag_name.clone(),
                    state: if tag_id.is_some() {
                        StashTagState::Resolved
                    } else {
                        StashTagState::CreationUnknown
                    },
                    tag_id,
                    add_dispatched: false,
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
                    let inserted=tx.execute("INSERT OR IGNORE INTO stash_tags(session,request,project,scan,tag_name,receipt) VALUES(?1,?2,?3,?4,?5,?6)",params![session.as_str(),request.request_id.as_str(),project(request)?.as_str(),scan_request_id.as_str(),tag_name,encode(&receipt)?]).map_err(internal)?;
                    if inserted != 1 {
                        return Err(ProtocolError::new(
                            ErrorCode::RequestConflict,
                            "tag dispatch already exists; reconcile before another request",
                        ));
                    }
                    tx.commit().map_err(internal)?;
                }
                if receipt.tag_id.is_none() {
                    // Once creation is dispatched, never create again on recovery.
                    let created = broker.create(&receipt.tag_name).ok();
                    receipt.tag_id = created.clone();
                    if let Ok(Some(found)) = broker.lookup(&receipt.tag_name) {
                        if created.as_ref().is_none_or(|id| id == &found) {
                            receipt.tag_id = Some(found);
                            receipt.state = StashTagState::Resolved;
                        }
                    }
                    self.store_tag(session, &receipt)?;
                }
                if receipt.state == StashTagState::Resolved {
                    self.attach_tag(request, connection, &broker, &mut receipt)?;
                }
                self.finish_tag(request, fingerprint, &receipt)
            }
            Command::StashTagStatus {
                connection,
                tag_request_id,
                ..
            } => {
                let (raw, mut receipt) = self.load_tag(request, tag_request_id)?;
                let broker = TagBroker::new(connection)?;
                verify_link(
                    connection,
                    &broker,
                    &receipt.publication,
                    &receipt.remote_job_id,
                )?;
                if let Some(found) = broker.lookup(&receipt.tag_name)? {
                    if receipt.tag_id.as_ref().is_some_and(|id| id != &found) {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "exact tag identity changed since dispatch",
                        ));
                    }
                    receipt.tag_id = Some(found.clone());
                    let scene = broker.scene(&receipt.publication.source.scene_id)?;
                    if !scene.scene.interactive || !same_scene(&scene.scene, &receipt.publication) {
                        return Err(ProtocolError::new(
                            ErrorCode::DependencyMismatch,
                            "scene attachment changed during tag reconciliation",
                        ));
                    }
                    crate::stash_files::verify_published(&receipt.publication)?;
                    receipt.state = if scene.tags.contains(&found) {
                        StashTagState::VerifiedAdded
                    } else if !receipt.add_dispatched {
                        StashTagState::Resolved
                    } else if matches!(
                        receipt.state,
                        StashTagState::VerifiedAdded | StashTagState::TagMissing
                    ) {
                        StashTagState::AttachedUnverified
                    } else {
                        receipt.state
                    };
                } else if receipt.tag_id.is_some() {
                    receipt.state = StashTagState::TagMissing;
                }
                // Status is read-only remotely. It may resolve creation/ADD outcomes
                // but never initiates creation or attachment.
                let mut db = self.db.lock().map_err(internal)?;
                let tx = db.transaction().map_err(internal)?;
                self.authorize(&tx, request, session)?;
                let changed=tx.execute("UPDATE stash_tags SET receipt=?4 WHERE session=?1 AND request=?2 AND receipt=?3",params![session.as_str(),tag_request_id.as_str(),raw,encode(&receipt)?]).map_err(internal)?;
                if changed == 0 {
                    let current: String = tx
                        .query_row(
                            "SELECT receipt FROM stash_tags WHERE session=?1 AND request=?2",
                            params![session.as_str(), tag_request_id.as_str()],
                            |r| r.get(0),
                        )
                        .map_err(internal)?;
                    receipt = decode(&current)?;
                }
                tx.commit().map_err(internal)?;
                Ok(ResponseBody::StashTag((receipt).into()))
            }
            Command::StashResumeTag {
                connection,
                tag_request_id,
                ..
            } => {
                {
                    let db = self.db.lock().map_err(internal)?;
                    check_revision(&db, request)?;
                }
                let (_, mut receipt) = self.load_tag(request, tag_request_id)?;
                if receipt.state != StashTagState::Resolved || receipt.add_dispatched {
                    return Err(ProtocolError::new(ErrorCode::DependencyMismatch,"only a resolved tag with no ADD dispatch can resume; reconcile unknown outcomes first"));
                }
                let broker = TagBroker::new(connection)?;
                verify_link(
                    connection,
                    &broker,
                    &receipt.publication,
                    &receipt.remote_job_id,
                )?;
                if broker.lookup(&receipt.tag_name)? != receipt.tag_id {
                    return Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "resolved exact tag identity changed",
                    ));
                }
                self.attach_tag(request, connection, &broker, &mut receipt)?;
                self.finish_tag(request, fingerprint, &receipt)
            }
            _ => Err(ProtocolError::invalid("unexpected Stash tag command")),
        }
    }
    fn load_tag(&self, request: &Request, id: &RequestId) -> PResult<(String, StashTagReceipt)> {
        let db = self.db.lock().map_err(internal)?;
        let session = request.session.as_ref().expect("authenticated");
        self.authorize(&db, request, session)?;
        let raw: String = db
            .query_row(
                "SELECT receipt FROM stash_tags WHERE session=?1 AND request=?2 AND project=?3",
                params![session.as_str(), id.as_str(), project(request)?.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(internal)?
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::NotFound,
                    "tag dispatch is absent for this session/project",
                )
            })?;
        let receipt = decode(&raw)?;
        Ok((raw, receipt))
    }
    fn store_tag(&self, session: &SessionId, receipt: &StashTagReceipt) -> PResult<()> {
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        let raw: String = tx
            .query_row(
                "SELECT receipt FROM stash_tags WHERE session=?1 AND request=?2",
                params![session.as_str(), receipt.request_id.as_str()],
                |r| r.get(0),
            )
            .map_err(internal)?;
        let current: StashTagReceipt = decode(&raw)?;
        if current.add_dispatched && !receipt.add_dispatched {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "concurrent ADD dispatch cannot be replaced by an earlier creation observation",
            ));
        }
        if current
            .tag_id
            .as_ref()
            .is_some_and(|id| receipt.tag_id.as_ref() != Some(id))
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "concurrent tag identity changed",
            ));
        }
        tx.execute(
            "UPDATE stash_tags SET receipt=?3 WHERE session=?1 AND request=?2",
            params![
                session.as_str(),
                receipt.request_id.as_str(),
                encode(receipt)?
            ],
        )
        .map_err(internal)?;
        tx.commit().map_err(internal)?;
        Ok(())
    }
    fn attach_tag(
        &self,
        request: &Request,
        connection: &StashConnection,
        broker: &TagBroker,
        receipt: &mut StashTagReceipt,
    ) -> PResult<()> {
        let session = request.session.as_ref().expect("authenticated");
        verify_link(
            connection,
            broker,
            &receipt.publication,
            &receipt.remote_job_id,
        )?;
        let id = receipt.tag_id.clone().ok_or_else(|| {
            ProtocolError::new(ErrorCode::DependencyMismatch, "tag has not been resolved")
        })?;
        let scene = broker.scene(&receipt.publication.source.scene_id)?;
        if scene.tags.contains(&id) {
            receipt.state = StashTagState::VerifiedAdded;
            self.store_tag(session, receipt)?;
            return Ok(());
        }
        {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            self.authorize(&tx, request, session)?;
            check_revision(&tx, request)?;
            let mut pending = receipt.clone();
            pending.state = StashTagState::AttachUnknown;
            pending.add_dispatched = true;
            // CAS prevents two resume requests from dispatching ADD together.
            let changed=tx.execute("UPDATE stash_tags SET receipt=?4 WHERE session=?1 AND request=?2 AND receipt=?3",params![session.as_str(),receipt.request_id.as_str(),encode(receipt)?,encode(&pending)?]).map_err(internal)?;
            if changed != 1 {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "tag phase changed before ADD dispatch",
                ));
            }
            claim_request_identity(
                &tx,
                session.as_str(),
                request,
                &format!(
                    "{:x}",
                    Sha256::digest(request_fingerprint(request).map_err(internal)?)
                ),
            )?;
            tx.commit().map_err(internal)?;
            *receipt = pending;
        }
        if broker
            .add(&receipt.publication.source.scene_id, &id)
            .is_ok()
        {
            receipt.state = StashTagState::AttachedUnverified;
        }
        // Even a lost ADD acknowledgement can be resolved by independent lookup.
        if let Ok(scene) = broker.scene(&receipt.publication.source.scene_id) {
            if scene.scene.interactive
                && same_scene(&scene.scene, &receipt.publication)
                && scene.tags.contains(&id)
                && crate::stash_files::verify_published(&receipt.publication).is_ok()
            {
                receipt.state = StashTagState::VerifiedAdded;
            }
        }
        self.store_tag(session, receipt)
    }
    fn finish_tag(
        &self,
        request: &Request,
        fingerprint: &str,
        receipt: &StashTagReceipt,
    ) -> PResult<ResponseBody> {
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        let body = ResponseBody::StashTag((receipt.clone()).into());
        remember(
            &tx,
            request.session.as_ref().expect("authenticated").as_str(),
            request,
            fingerprint,
            &body,
        )?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }
}
fn same_scene(scene: &StashScene, publication: &StashScriptExport) -> bool {
    scene.id == publication.source.scene_id
        && scene.files.len() == 1
        && scene.files[0].id == publication.source.file_id
        && scene.files[0].path == publication.source.server_path
}
fn verify_link(
    connection: &StashConnection,
    broker: &TagBroker,
    publication: &StashScriptExport,
    job: &str,
) -> PResult<()> {
    if publication.source.endpoint.as_deref()
        != Some(crate::stash::endpoint_identity(connection)?.as_str())
    {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "tag endpoint differs from publication",
        ));
    }
    if crate::stash::scan_state(connection, job)? != StashScanState::FinishedUnattached {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "tagging requires the recorded scan job to remain FINISHED",
        ));
    }
    let scene = broker.scene(&publication.source.scene_id)?;
    if !scene.scene.interactive || !same_scene(&scene.scene, publication) {
        return Err(ProtocolError::new(
            ErrorCode::DependencyMismatch,
            "tagging requires the exact verified scene attachment",
        ));
    }
    crate::stash_files::verify_published(publication)
}
