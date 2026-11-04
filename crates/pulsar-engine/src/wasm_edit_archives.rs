//! Catalog visibility is separate from immutable completed invocation custody.
use super::*;
const SCHEMA:&str="CREATE TABLE IF NOT EXISTS wasm_edit_archives(attempt TEXT PRIMARY KEY NOT NULL REFERENCES wasm_edits(id))";
fn exists(db: &Connection) -> PResult<bool> {
    db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='wasm_edit_archives')",[],|r|r.get(0)).map_err(internal)
}
pub(super) fn initialize(db: &Connection) -> PResult<()> {
    db.execute_batch(SCHEMA).map_err(internal)
}
pub(super) fn operational_count(db: &Connection) -> PResult<i64> {
    let sql = if exists(db)? {
        "SELECT COUNT(*) FROM wasm_edits WHERE NOT EXISTS(SELECT 1 FROM wasm_edit_archives WHERE attempt=wasm_edits.id)"
    } else {
        "SELECT COUNT(*) FROM wasm_edits"
    };
    db.query_row(sql, [], |r| r.get(0)).map_err(internal)
}
pub(super) fn validate(db: &Connection) -> PResult<()> {
    crate::wasm_archive_db::version(db).map_err(internal)?;
    if !exists(db)? {
        let raw: i64 = crate::native_preparation_db::version(db).map_err(internal)?;
        if raw == 26 {
            return Err(internal("WASM archive catalog missing"));
        }
        return Ok(());
    }
    let reference = Connection::open_in_memory().map_err(internal)?;
    reference.execute_batch(SCHEMA).map_err(internal)?;
    let declaration = |db: &Connection| {
        db.query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='wasm_edit_archives'",
            [],
            |r| r.get::<_, String>(0),
        )
    };
    if declaration(db).map_err(internal)? != declaration(&reference).map_err(internal)? {
        return Err(internal("WASM archive schema requires explicit recovery"));
    }
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM wasm_edit_archives", [], |r| r.get(0))
        .map_err(internal)?;
    if count > MAX_RETAINED_INVOCATIONS {
        return Err(internal("WASM archived invocation bound exceeded"));
    }
    let raw: i64 = crate::native_preparation_db::version(db).map_err(internal)?;
    if count > 0 && raw != 26 {
        return Err(internal(
            "WASM archive metadata requires its schema-26 barrier",
        ));
    }
    let mut q = db
        .prepare("SELECT attempt FROM wasm_edit_archives")
        .map_err(internal)?;
    let ids = q
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(internal)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(internal)?;
    for id in ids {
        let plan = read_plan(db, &AttemptId::new(id).map_err(internal)?)?;
        if report(db, &plan)?.state != WasmEditState::Completed {
            return Err(internal("archived WASM invocation is not completed"));
        }
    }
    Ok(())
}
impl Engine {
    pub(super) fn list_archived_wasm(
        &self,
        request: &Request,
        owner: &SessionId,
    ) -> PResult<ResponseBody> {
        let Command::ListArchivedWasmEdits { after, limit } = &request.command else {
            return Err(ProtocolError::invalid("not an archive history query"));
        };
        validate_wasm_archive_limit(*limit)?;
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db.transaction().map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        // One read transaction observes membership, ordering and receipts together.
        let mut statement = tx.prepare("SELECT w.id FROM wasm_edits w JOIN wasm_edit_archives a ON a.attempt=w.id WHERE w.project=?1 AND w.session=?2 AND (?3 IS NULL OR w.id>?3) ORDER BY w.id COLLATE BINARY LIMIT ?4").map_err(internal)?;
        let mut ids = statement
            .query_map(
                params![
                    project(request)?.as_str(),
                    owner.as_str(),
                    after.as_ref().map(AttemptId::as_str),
                    i64::from(*limit) + 1
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(internal)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(internal)?;
        drop(statement);
        let more = ids.len() > usize::from(*limit);
        if more {
            ids.pop();
        }
        let operations = ids
            .into_iter()
            .map(|id| {
                let plan = read_plan(&tx, &AttemptId::new(id).map_err(internal)?)?;
                report(&tx, &plan)
            })
            .collect::<PResult<Vec<_>>>()?;
        let next = more.then(|| {
            operations
                .last()
                .expect("validated positive limit")
                .attempt_id
                .clone()
        });
        let page = WasmArchivePage {
            project_id: project(request)?.clone(),
            after: after.clone(),
            limit: *limit,
            operations,
            next,
        };
        page.validate_for(request)?;
        Ok(ResponseBody::WasmArchivePage(page))
    }
    pub(super) fn archive_completed_wasm(
        &self,
        request: &Request,
        fingerprint: &str,
        owner: &SessionId,
    ) -> PResult<ResponseBody> {
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, request, owner)?;
        if let Some(body) = replay(&tx, owner.as_str(), request, fingerprint)? {
            return Ok(body);
        }
        let Command::ArchiveCompletedWasmEdit {
            attempt_id,
            original_request_id,
        } = &request.command
        else {
            return Err(ProtocolError::invalid("not an archive operation"));
        };
        let plan = read_plan(&tx, attempt_id)?;
        if &plan.owner != owner || &plan.project != project(request)? {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "WASM invocation belongs to another session or project",
            ));
        }
        let old = report(&tx, &plan)?;
        if &old.request_id != original_request_id || old.state != WasmEditState::Completed {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "Archival requires the observed completed invocation and original request",
            ));
        }
        if self
            .workers
            .lock()
            .map_err(internal)?
            .contains_key(&key(&plan))
        {
            return Err(ProtocolError::new(
                ErrorCode::RequestConflict,
                "WASM worker retirement is still pending",
            ));
        }
        #[cfg(target_os = "linux")]
        if self
            .worker_fence
            .has_pending_worker_custody()
            .map_err(internal)?
        {
            return Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "worker custody retirement or creation is still pending",
            ));
        }
        old.validate_archive_for(request)?;
        crate::wasm_archive_db::arm(&tx).map_err(internal)?;
        tx.execute(
            "INSERT OR IGNORE INTO wasm_edit_archives(attempt) VALUES(?1)",
            [attempt_id.as_str()],
        )
        .map_err(internal)?;
        let body = ResponseBody::WasmEditArchived((old).into());
        remember(&tx, owner.as_str(), request, fingerprint, &body)?;
        tx.commit().map_err(internal)?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_schema_and_stripped_reader_are_refused_without_mutation() {
        let mut db = Connection::open_in_memory().unwrap();
        super::super::initialize(&mut db).unwrap();
        validate(&db).unwrap();
        db.execute_batch("ALTER TABLE wasm_edit_archives RENAME TO previous_archives;CREATE TABLE wasm_edit_archives(attempt TEXT PRIMARY KEY)").unwrap();
        let before: i64 = db
            .query_row("PRAGMA schema_version", [], |r| r.get(0))
            .unwrap();
        assert!(validate(&db).is_err());
        assert_eq!(
            db.query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            before
        );
    }
}
