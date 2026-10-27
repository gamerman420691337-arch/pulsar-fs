//! Exact-content disclosure with durable uncertainty and native-only locations.
use super::*;
use crate::handy_hosting_wire::{UnknownHostingOutcome, VerifiedHosted};
use std::io::Read;
type Key = (SessionId, RequestId);
struct Entry {
    inflight: bool,
    verified: Option<VerifiedHosted>,
}
#[derive(Default)]
pub(super) struct Registry {
    entries: HashMap<Key, Entry>,
}
struct Inflight<'a> {
    registry: &'a Mutex<Registry>,
    key: Key,
}
impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if registry.entries.get(&self.key).is_some_and(|e| e.inflight) {
            registry.entries.remove(&self.key);
        }
    }
}
pub(super) fn is_command(command: &Command) -> bool {
    matches!(
        command,
        Command::HostHandyStrokeScript { .. }
            | Command::HandyHostingStatus { .. }
            | Command::ForgetHandyHosting { .. }
    )
}
fn load(
    db: &Connection,
    session: &SessionId,
    id: &RequestId,
    project: &ProjectId,
) -> PResult<HandyHostingReport> {
    let raw: Option<String> = db
        .query_row(
            "SELECT response FROM requests WHERE session=?1 AND request=?2",
            params![session.as_str(), id.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(internal)?;
    let ResponseBody::HandyHosting(report) = decode(&raw.ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, "Handy hosting request not found")
    })?)?
    else {
        return Err(ProtocolError::invalid(
            "request is not Handy hosting history",
        ));
    };
    report.validate()?;
    if report.request_id != *id
        || report.project_id != *project
        || report.link_available
        || report.upload_in_flight
    {
        return Err(ProtocolError::invalid(
            "Handy hosting history binding mismatch",
        ));
    }
    Ok(*report)
}
fn replace(tx: &Transaction<'_>, session: &SessionId, report: &HandyHostingReport) -> PResult<()> {
    report.validate()?;
    if report.link_available || report.upload_in_flight {
        return Err(ProtocolError::invalid(
            "live hosting facts cannot be persisted",
        ));
    }
    let count = tx
        .execute(
            "UPDATE requests SET response=?1 WHERE session=?2 AND request=?3",
            params![
                encode(&ResponseBody::HandyHosting(Box::new(report.clone())))?,
                session.as_str(),
                report.request_id.as_str()
            ],
        )
        .map_err(internal)?;
    if count != 1 {
        return Err(ProtocolError::invalid("hosting history disappeared"));
    }
    Ok(())
}
impl Engine {
    /// Native reference capture, not setup/admission authority. Caller holds the DB
    /// guard and authenticates the specific online-inspection request.
    pub(super) fn capture_handy_hosted(
        &self,
        db: &Connection,
        session: &SessionId,
        project: &ProjectId,
        revision: RevisionId,
        id: &RequestId,
    ) -> PResult<(HandyHostingReport, VerifiedHosted)> {
        let report = load(db, session, id, project)?;
        let current = project_snapshot(db, project)?;
        if report.retired
            || report.outcome != HandyHostingOutcome::Verified
            || report.engine_epoch.as_str() != self.engine_epoch
            || report.revision != revision
            || current.revision != revision
            || current.motion.program.sha256 != report.master_sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "hosting is not current for setup inspection",
            ));
        }
        let registry = self.handy_hosting.lock().map_err(internal)?;
        let entry = registry
            .entries
            .get(&(session.clone(), id.clone()))
            .filter(|e| !e.inflight)
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "native hosting location is unavailable",
                )
            })?;
        let verified = entry
            .verified
            .as_ref()
            .filter(|v| v.matches(&report))
            .ok_or_else(|| {
                ProtocolError::new(
                    ErrorCode::Unavailable,
                    "verified hosting location is unavailable",
                )
            })?;
        Ok((report, verified.clone()))
    }
    fn hosting_view(
        &self,
        db: &Connection,
        registry: &Registry,
        session: &SessionId,
        mut report: HandyHostingReport,
    ) -> PResult<ResponseBody> {
        if let Some(entry) = registry
            .entries
            .get(&(session.clone(), report.request_id.clone()))
        {
            report.upload_in_flight = entry.inflight;
            let current = project_snapshot(db, &report.project_id)?;
            report.link_available = !report.retired
                && !entry.inflight
                && report.engine_epoch.as_str() == self.engine_epoch
                && current.revision == report.revision
                && current.motion.program.sha256 == report.master_sha256
                && entry.verified.as_ref().is_some_and(|v| v.matches(&report));
        }
        report.validate()?;
        Ok(ResponseBody::HandyHosting(Box::new(report)))
    }
    pub(super) fn handy_hosting_control(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
    ) -> PResult<ResponseBody> {
        self.handy_hosting_with(
            request,
            session,
            fingerprint,
            crate::handy_hosting_wire::host,
        )
    }
    pub(super) fn handy_hosting_with<F>(
        &self,
        request: &Request,
        session: &SessionId,
        fingerprint: &str,
        host: F,
    ) -> PResult<ResponseBody>
    where
        F: FnOnce(&[u8]) -> Result<VerifiedHosted, UnknownHostingOutcome>,
    {
        request.validate()?;
        let mut db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, session)?;
        check_request_identity(&db, session.as_str(), request, fingerprint)?;
        let project = project(request)?;
        let mut registry = self.handy_hosting.lock().map_err(internal)?;
        if let Command::HandyHostingStatus {
            upload_request_id, ..
        } = &request.command
        {
            return self.hosting_view(
                &db,
                &registry,
                session,
                load(&db, session, upload_request_id, project)?,
            );
        }
        if let Some(body) = replay(&db, session.as_str(), request, fingerprint)? {
            let ResponseBody::HandyHosting(report) = body else {
                return Err(ProtocolError::invalid("hosting replay type mismatch"));
            };
            report.validate()?;
            if report.project_id != *project {
                return Err(ProtocolError::invalid("hosting replay project mismatch"));
            }
            match &request.command {
                Command::HostHandyStrokeScript {
                    preparation_request_id,
                    script_sha256,
                    engine_epoch,
                    ..
                } if report.request_id == request.request_id
                    && report.preparation_request_id == *preparation_request_id
                    && report.script.sha256 == *script_sha256
                    && report.engine_epoch == *engine_epoch
                    && Some(report.revision) == request.expected_revision =>
                {
                    return self.hosting_view(
                        &db,
                        &registry,
                        session,
                        load(&db, session, &request.request_id, project)?,
                    );
                }
                Command::ForgetHandyHosting {
                    upload_request_id, ..
                } if report.request_id == *upload_request_id && report.retired => {
                    let current = load(&db, session, upload_request_id, project)?;
                    if !current.retired {
                        return Err(ProtocolError::invalid("hosting retirement history changed"));
                    }
                    return self.hosting_view(&db, &registry, session, current);
                }
                _ => {
                    return Err(ProtocolError::invalid(
                        "hosting replay correspondence mismatch",
                    ))
                }
            }
        }
        if let Command::ForgetHandyHosting {
            upload_request_id, ..
        } = &request.command
        {
            let mut report = load(&db, session, upload_request_id, project)?;
            report.retired = true;
            let tx = db.transaction().map_err(internal)?;
            replace(&tx, session, &report)?;
            let body = ResponseBody::HandyHosting(Box::new(report));
            remember(&tx, session.as_str(), request, fingerprint, &body)?;
            tx.commit().map_err(internal)?;
            self.retire_handy_preparations(handy_playback_preparation::Retirement::Hosting(
                session,
                upload_request_id,
            ));
            let key = (session.clone(), upload_request_id.clone());
            if registry.entries.get(&key).is_some_and(|e| !e.inflight) {
                registry.entries.remove(&key);
            }
            return self.hosting_view(
                &db,
                &registry,
                session,
                load(&db, session, upload_request_id, project)?,
            );
        }
        let Command::HostHandyStrokeScript {
            preparation_request_id,
            script_sha256,
            engine_epoch,
            ..
        } = &request.command
        else {
            return Err(ProtocolError::unsupported("Handy hosting operation"));
        };
        if engine_epoch.as_str() != self.engine_epoch {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "Handy hosting belongs to a retired Engine boot",
            ));
        }
        check_revision(&db, request)?;
        if registry.entries.len() >= 64 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "Forget an existing hosting link before admitting more work",
            ));
        }
        let raw:Option<String>=db.query_row("SELECT r.response FROM requests r JOIN exports e ON e.session=r.session AND e.request=r.request WHERE r.session=?1 AND r.request=?2 AND e.project=?3 AND e.state='complete' AND e.sha256=?4 AND e.axis='stroke'",params![session.as_str(),preparation_request_id.as_str(),project.as_str(),script_sha256],|r|r.get(0)).optional().map_err(internal)?;
        let ResponseBody::HandyStrokeScript(prepared) = decode(&raw.ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::NotFound,
                "completed Handy script preparation not found for this session",
            )
        })?)?
        else {
            return Err(ProtocolError::invalid(
                "original request did not prepare a Handy script",
            ));
        };
        prepared.validate()?;
        let current = project_snapshot(&db, project)?;
        if prepared.request_id != *preparation_request_id
            || prepared.project_id != *project
            || Some(prepared.revision) != request.expected_revision
            || prepared.master_sha256 != current.motion.program.sha256
            || prepared.identity.sha256 != *script_sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "Handy preparation binding changed",
            ));
        }
        let _memory = self.pool.reserve_memory(32 * 1024 * 1024)?;
        let file = artifacts::open_regular(&prepared.path).map_err(|_| {
            ProtocolError::new(
                ErrorCode::Unavailable,
                "prepared script file is unavailable",
            )
        })?;
        let mut bytes = Vec::new();
        file.take(HANDY_STROKE_SCRIPT_MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                ProtocolError::new(ErrorCode::Unavailable, "prepared script read failed")
            })?;
        if bytes.len() as u64 != prepared.identity.byte_len
            || format!("{:x}", Sha256::digest(&bytes)) != prepared.identity.sha256
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "prepared script bytes changed; no upload authorized",
            ));
        }
        let report = HandyHostingReport {
            request_id: request.request_id.clone(),
            preparation_request_id: preparation_request_id.clone(),
            project_id: project.clone(),
            revision: prepared.revision,
            engine_epoch: engine_epoch.clone(),
            master_sha256: prepared.master_sha256.clone(),
            script: prepared.identity.clone(),
            outcome: HandyHostingOutcome::Unknown,
            downloaded: None,
            retired: false,
            link_available: false,
            upload_in_flight: false,
        };
        report.validate()?;
        let tx = db.transaction().map_err(internal)?;
        remember(
            &tx,
            session.as_str(),
            request,
            fingerprint,
            &ResponseBody::HandyHosting(Box::new(report.clone())),
        )?;
        tx.commit().map_err(internal)?;
        let key = (session.clone(), request.request_id.clone());
        registry.entries.insert(
            key.clone(),
            Entry {
                inflight: true,
                verified: None,
            },
        );
        drop(registry);
        drop(db);
        let guard = Inflight {
            registry: &self.handy_hosting,
            key: key.clone(),
        };
        let observed = host(&bytes);
        let mut db = self.db.lock().map_err(internal)?;
        let fresh = self
            .authorize(&db, request, session)
            .and_then(|_| check_revision(&db, request))
            .and_then(|_| {
                if project_snapshot(&db, project)?.motion.program.sha256 != report.master_sha256 {
                    Err(ProtocolError::new(
                        ErrorCode::DependencyMismatch,
                        "hosting master binding changed",
                    ))
                } else {
                    Ok(())
                }
            });
        check_request_identity(&db, session.as_str(), request, fingerprint)?;
        let mut current = load(&db, session, &request.request_id, project)?;
        if current.master_sha256 != report.master_sha256
            || current.script != report.script
            || current.preparation_request_id != report.preparation_request_id
            || current.engine_epoch != report.engine_epoch
            || current.revision != report.revision
            || current.outcome != HandyHostingOutcome::Unknown
        {
            return Err(ProtocolError::invalid(
                "hosting observation lost its original correspondence",
            ));
        }
        let mut registry = self.handy_hosting.lock().map_err(internal)?;
        match observed {
            Ok(verified) => {
                current.outcome = HandyHostingOutcome::Verified;
                current.downloaded = Some(verified.downloaded.clone());
                current.retired |= fresh.is_err();
                current.validate()?;
                let tx = db.transaction().map_err(internal)?;
                replace(&tx, session, &current)?;
                tx.commit().map_err(internal)?;
                if !current.retired {
                    let entry = registry
                        .entries
                        .get_mut(&key)
                        .ok_or_else(|| ProtocolError::invalid("hosting reservation disappeared"))?;
                    entry.inflight = false;
                    entry.verified = Some(verified);
                } else {
                    registry.entries.remove(&key);
                }
            }
            Err(_) => {
                registry.entries.remove(&key);
            }
        }
        fresh?;
        let response = self.hosting_view(&db, &registry, session, current)?;
        drop(registry);
        drop(db);
        drop(guard);
        Ok(response)
    }
}
