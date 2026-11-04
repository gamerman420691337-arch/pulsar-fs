//! Durable preparation intent, separately from image/worker storage custody.
//! Schema 27 wraps preceding reader layers; no retained use survives restart.
use anyhow::{ensure, Context, Result};
use pulsar_protocol::{
    NativeImagePreparationIntent, NativeImagePreparationReport, NativeImagePreparationState,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

const MARKER: &str = "CREATE TABLE native_preparation_reader(singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton=1),underlying_version INTEGER NOT NULL CHECK(underlying_version BETWEEN 20 AND 26))";
const CREATE: &str = "CREATE TABLE native_image_preparations(operation TEXT PRIMARY KEY NOT NULL,owner TEXT NOT NULL REFERENCES sessions(id),project TEXT NOT NULL REFERENCES projects(id),revision INTEGER NOT NULL CHECK(revision>=0),request TEXT NOT NULL,fingerprint TEXT NOT NULL,state TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>=0),report BLOB NOT NULL,UNIQUE(owner,request),FOREIGN KEY(project,revision) REFERENCES revisions(project,revision))";
const MAX_REPORT: usize = 16 * 1024;
const MAX_RECORDS: i64 = 4096;
const ADMITTED: &str = "typeof(operation)='text' AND length(CAST(operation AS BLOB)) BETWEEN 1 AND 128 AND typeof(owner)='text' AND length(CAST(owner AS BLOB)) BETWEEN 1 AND 128 AND typeof(project)='text' AND length(CAST(project AS BLOB)) BETWEEN 1 AND 128 AND typeof(revision)='integer' AND revision>=0 AND typeof(request)='text' AND length(CAST(request AS BLOB)) BETWEEN 1 AND 128 AND typeof(fingerprint)='text' AND length(CAST(fingerprint AS BLOB))=64 AND typeof(state)='text' AND length(CAST(state AS BLOB)) BETWEEN 1 AND 32 AND typeof(generation)='integer' AND generation>=0 AND typeof(report)='blob' AND length(report) BETWEEN 1 AND 16384";

fn exists(db: &Connection, name: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )?)
}
fn layout(db: &Connection, name: &str, expected: &str) -> Result<()> {
    let sql: Option<String> = db.query_row("SELECT sql FROM sqlite_master WHERE type='table' AND name=?1 AND typeof(sql)='text' AND length(CAST(sql AS BLOB))<=4096", [name], |r| r.get(0)).optional()?;
    ensure!(
        sql.as_deref() == Some(expected),
        "native preparation {name} layout differs"
    );
    Ok(())
}
pub(crate) fn version(db: &Connection) -> Result<i64> {
    let raw = crate::library_batch_db::version(db)?;
    let marker = exists(db, "native_preparation_reader")?;
    let records = exists(db, "native_image_preparations")?;
    if raw == 27 || marker || records {
        ensure!(
            raw == 27 && marker && records,
            "native preparation reader barrier missing or downgraded"
        );
        layout(db, "native_preparation_reader", MARKER)?;
        layout(db, "native_image_preparations", CREATE)?;
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM (SELECT 1 FROM native_preparation_reader LIMIT 2)",
            [],
            |r| r.get(0),
        )?;
        ensure!(count == 1, "native preparation reader singleton differs");
        return Ok(db.query_row("SELECT underlying_version FROM native_preparation_reader WHERE singleton=1 AND typeof(singleton)='integer' AND typeof(underlying_version)='integer' AND underlying_version BETWEEN 20 AND 26", [], |r| r.get(0))?);
    }
    ensure!(raw < 27, "unknown native preparation reader version");
    Ok(raw)
}
/// Existing inner reader publication must preserve this outer barrier.
pub(crate) fn publish(db: &Connection, new_version: i64) -> Result<()> {
    let old = version(db)?;
    ensure!(
        (2..=26).contains(&new_version) && new_version >= old,
        "native preparation publication would downgrade storage"
    );
    if exists(db, "native_preparation_reader")? {
        ensure!(
            new_version >= 20,
            "native preparation needs tracked worker storage"
        );
        db.execute(
            "UPDATE native_preparation_reader SET underlying_version=?1 WHERE singleton=1",
            [new_version],
        )?;
    } else {
        crate::library_batch_db::publish(db, new_version)?;
    }
    Ok(())
}
fn arm(tx: &Transaction<'_>) -> Result<()> {
    let old = version(tx)?;
    ensure!(
        (20..=26).contains(&old),
        "native preparation requires tracked creation storage"
    );
    if !exists(tx, "native_preparation_reader")? {
        tx.execute_batch(MARKER)?;
        tx.execute_batch(CREATE)?;
        tx.execute("INSERT INTO native_preparation_reader VALUES(1,?1)", [old])?;
        crate::library_batch_db::publish(tx, 27)?;
    }
    version(tx)?;
    Ok(())
}
fn bytes(report: &NativeImagePreparationReport) -> Result<Vec<u8>> {
    report.validate()?;
    let bytes = serde_json::to_vec(report)?;
    ensure!(
        bytes.len() <= MAX_REPORT,
        "native preparation report exceeds admission"
    );
    Ok(bytes)
}
fn state(report: &NativeImagePreparationReport) -> Result<String> {
    let value = serde_json::to_value(report.state)?;
    Ok(value
        .as_str()
        .context("native preparation state is not text")?
        .to_owned())
}
type Row = (
    String,
    String,
    String,
    u64,
    String,
    String,
    String,
    u64,
    Vec<u8>,
);
fn decode(row: Row) -> Result<NativeImagePreparationReport> {
    let (operation, owner, project, revision, request, fingerprint, stored_state, generation, raw) =
        row;
    let report: NativeImagePreparationReport = serde_json::from_slice(&raw)?;
    ensure!(
        bytes(&report)? == raw,
        "native preparation report is not canonical"
    );
    pulsar_protocol::validate_sha256(&fingerprint)?;
    ensure!(
        report.intent.operation_id.as_str() == operation
            && report.intent.owner.as_str() == owner
            && report.intent.project.as_str() == project
            && report.intent.revision == revision
            && report.intent.original_request_id.as_str() == request
            && report.generation == generation
            && state(&report)? == stored_state,
        "native preparation indexed membership differs"
    );
    Ok(report)
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
    ))
}
pub(crate) fn load(
    db: &Connection,
    operation: &str,
) -> Result<Option<NativeImagePreparationReport>> {
    version(db)?;
    if !exists(db, "native_image_preparations")? {
        return Ok(None);
    }
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM native_image_preparations WHERE operation=?1",
        [operation],
        |r| r.get(0),
    )?;
    let value = db.query_row(&format!("SELECT operation,owner,project,revision,request,fingerprint,state,generation,report FROM native_image_preparations WHERE operation=?1 AND {ADMITTED}"), [operation], row).optional()?;
    ensure!(
        value.is_some() == (count == 1),
        "native preparation row exceeds admission or has invalid types"
    );
    value.map(decode).transpose()
}
pub(crate) fn resolve(
    db: &Connection,
    owner: &pulsar_protocol::SessionId,
    request: &pulsar_protocol::RequestId,
) -> Result<Option<NativeImagePreparationReport>> {
    version(db)?;
    if !exists(db, "native_image_preparations")? {
        return Ok(None);
    }
    let operation: Option<String> = db.query_row("SELECT operation FROM native_image_preparations WHERE owner=?1 AND request=?2 AND typeof(operation)='text' AND length(CAST(operation AS BLOB)) BETWEEN 1 AND 128", params![owner.as_str(),request.as_str()], |r| r.get(0)).optional()?;
    operation
        .map(|operation| load(db, &operation)?.context("native preparation resolution row missing"))
        .transpose()
}
pub(crate) fn validate(db: &Connection) -> Result<()> {
    version(db)?;
    if !exists(db, "native_image_preparations")? {
        return Ok(());
    }
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM native_image_preparations LIMIT 4097)",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count <= MAX_RECORDS,
        "native preparation retained record capacity exceeded"
    );
    let admitted: i64 = db.query_row(
        &format!("SELECT COUNT(*) FROM native_image_preparations WHERE {ADMITTED}"),
        [],
        |r| r.get(0),
    )?;
    ensure!(
        admitted == count,
        "native preparation row exceeds admission or has invalid types"
    );
    let members: i64 = db.query_row("SELECT COUNT(*) FROM native_image_preparations p JOIN sessions s ON s.id=p.owner JOIN revisions r ON r.project=p.project AND r.revision=p.revision JOIN projects j ON j.id=p.project", [], |r| r.get(0))?;
    ensure!(
        members == count,
        "native preparation actor/project/revision membership missing"
    );
    let mut statement = db.prepare("SELECT operation,owner,project,revision,request,fingerprint,state,generation,report FROM native_image_preparations ORDER BY operation LIMIT 4096")?;
    for value in statement.query_map([], row)? {
        decode(value?)?;
    }
    Ok(())
}
/// Caller must commit this transaction before spawning or capturing anything.
pub(crate) fn begin(
    tx: &Transaction<'_>,
    intent: NativeImagePreparationIntent,
    fingerprint: &str,
) -> Result<NativeImagePreparationReport> {
    pulsar_protocol::validate_sha256(fingerprint)?;
    let report = NativeImagePreparationReport::queued(intent)?;
    let membership: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sessions s JOIN revisions r ON r.project=?2 AND r.revision=?3 JOIN projects p ON p.id=r.project WHERE s.id=?1)", params![report.intent.owner.as_str(),report.intent.project.as_str(),report.intent.revision], |r| r.get(0))?;
    ensure!(
        membership,
        "native preparation actor/project/revision membership missing"
    );
    arm(tx)?;
    validate(tx)?;
    if let Some(old) = load(tx, report.intent.operation_id.as_str())? {
        let old_fingerprint: String = tx.query_row(
            "SELECT fingerprint FROM native_image_preparations WHERE operation=?1",
            [report.intent.operation_id.as_str()],
            |r| r.get(0),
        )?;
        ensure!(
            old.intent == report.intent && old_fingerprint == fingerprint,
            "native preparation request identity conflict"
        );
        return Ok(old);
    }
    let count: i64 = tx.query_row("SELECT COUNT(*) FROM native_image_preparations", [], |r| {
        r.get(0)
    })?;
    ensure!(
        count < MAX_RECORDS,
        "native preparation record admission exhausted"
    );
    let i = &report.intent;
    tx.execute(
        "INSERT INTO native_image_preparations VALUES(?1,?2,?3,?4,?5,?6,?7,0,?8)",
        params![
            i.operation_id.as_str(),
            i.owner.as_str(),
            i.project.as_str(),
            i.revision,
            i.original_request_id.as_str(),
            fingerprint,
            state(&report)?,
            bytes(&report)?
        ],
    )?;
    Ok(report)
}
pub(crate) fn advance(tx: &Transaction<'_>, next: &NativeImagePreparationReport) -> Result<()> {
    let old = load(tx, next.intent.operation_id.as_str())?
        .context("native preparation intent missing")?;
    old.validate_successor(next)?;
    let changed = tx.execute("UPDATE native_image_preparations SET state=?1,generation=?2,report=?3 WHERE operation=?4 AND generation=?5", params![state(next)?,next.generation,bytes(next)?,next.intent.operation_id.as_str(),old.generation])?;
    ensure!(changed == 1, "native preparation generation conflict");
    Ok(())
}
/// Storage cleanup is still owned by the fence journal; this records lost use.
pub(crate) fn recover(db: &mut Connection) -> Result<()> {
    validate(db)?;
    if !exists(db, "native_image_preparations")? {
        return Ok(());
    }
    let tx = db.transaction()?;
    let operations: Vec<String> = {
        let mut statement = tx.prepare(
            "SELECT operation FROM native_image_preparations ORDER BY operation LIMIT 4096",
        )?;
        let values = statement
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        values
    };
    for operation in operations {
        let mut report =
            load(&tx, &operation)?.context("native preparation recovery intent disappeared")?;
        if !report.state.is_terminal() {
            report.state = NativeImagePreparationState::Interrupted;
            report.generation = report
                .generation
                .checked_add(1)
                .context("native preparation generation overflow")?;
            report.error = Some("engine restart; retained native image use is not restored".into());
            report.cleanup_confirmed = false;
            advance(&tx, &report)?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_protocol::*;
    fn fixture() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA user_version=20; CREATE TABLE sessions(id TEXT PRIMARY KEY); CREATE TABLE projects(id TEXT PRIMARY KEY); CREATE TABLE revisions(project TEXT,revision INTEGER,PRIMARY KEY(project,revision)); INSERT INTO sessions VALUES('owner'); INSERT INTO projects VALUES('project'); INSERT INTO revisions VALUES('project',0);").unwrap();
        db
    }
    fn intent() -> NativeImagePreparationIntent {
        NativeImagePreparationIntent {
            operation_id: AttemptId::new("operation").unwrap(),
            engine_epoch: AttemptId::new("boot").unwrap(),
            owner: SessionId::new("owner").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: 0,
            original_request_id: RequestId::new("request").unwrap(),
            options: NativeImagePreparationOptions {
                preparation_ms: 100,
                retention_ms: 200,
            },
            image_sha256: "a".repeat(64),
            owner_policy_sha256: "b".repeat(64),
            declared_bytes: 4096,
        }
    }
    fn start(db: &mut Connection) -> NativeImagePreparationReport {
        let tx = db.transaction().unwrap();
        let report = begin(&tx, intent(), &"c".repeat(64)).unwrap();
        tx.commit().unwrap();
        report
    }
    #[test]
    fn preparation_intent_commit_rollback_and_request_identity_are_atomic() {
        let mut db = fixture();
        {
            let tx = db.transaction().unwrap();
            begin(&tx, intent(), &"c".repeat(64)).unwrap();
            tx.rollback().unwrap();
        }
        assert_eq!(version(&db).unwrap(), 20);
        assert!(!exists(&db, "native_image_preparations").unwrap());
        let initial = start(&mut db);
        assert_eq!(version(&db).unwrap(), 20);
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            27
        );
        let tx = db.transaction().unwrap();
        assert_eq!(begin(&tx, intent(), &"c".repeat(64)).unwrap(), initial);
        assert!(begin(&tx, intent(), &"d".repeat(64)).is_err());
        let mut changed = intent();
        changed.options.preparation_ms = 99;
        assert!(begin(&tx, changed, &"c".repeat(64)).is_err());
        tx.rollback().unwrap();
        validate(&db).unwrap();
        assert_eq!(load(&db, "operation").unwrap(), Some(initial));
    }
    #[test]
    fn preparation_cancellation_refuses_late_retention_and_stale_generation() {
        let mut db = fixture();
        let initial = start(&mut db);
        let mut preparing = initial;
        preparing.state = NativeImagePreparationState::Preparing;
        preparing.generation = 1;
        let tx = db.transaction().unwrap();
        advance(&tx, &preparing).unwrap();
        tx.commit().unwrap();
        let mut cancellation = preparing.clone();
        cancellation.state = NativeImagePreparationState::CancelRequested;
        cancellation.generation = 2;
        cancellation.elapsed_ms = 5;
        let tx = db.transaction().unwrap();
        advance(&tx, &cancellation).unwrap();
        tx.commit().unwrap();
        let mut late = preparing;
        late.state = NativeImagePreparationState::Retained;
        late.generation = 2;
        late.elapsed_ms = 6;
        late.retained_at_elapsed_ms = Some(6);
        let tx = db.transaction().unwrap();
        assert!(advance(&tx, &late).is_err());
        assert!(advance(&tx, &cancellation).is_err());
        tx.rollback().unwrap();
        recover(&mut db).unwrap();
        let report = load(&db, "operation").unwrap().unwrap();
        assert_eq!(report.state, NativeImagePreparationState::Interrupted);
        assert_eq!(report.generation, 3);
        assert!(!report.cleanup_confirmed);
        let before = report.clone();
        recover(&mut db).unwrap();
        assert_eq!(load(&db, "operation").unwrap(), Some(before));
    }
    #[test]
    fn preparation_restart_preserves_historical_retention_without_restoring_use() {
        let mut db = fixture();
        let mut report = start(&mut db);
        let tx = db.transaction().unwrap();
        report.state = NativeImagePreparationState::Preparing;
        report.generation = 1;
        advance(&tx, &report).unwrap();
        report.state = NativeImagePreparationState::Retained;
        report.generation = 2;
        report.elapsed_ms = 10;
        report.retained_at_elapsed_ms = Some(10);
        advance(&tx, &report).unwrap();
        tx.commit().unwrap();
        recover(&mut db).unwrap();
        let recovered = load(&db, "operation").unwrap().unwrap();
        assert_eq!(recovered.intent, report.intent);
        assert_eq!(recovered.retained_at_elapsed_ms, Some(10));
        assert_eq!(recovered.state, NativeImagePreparationState::Interrupted);
        assert!(!recovered.cleanup_confirmed);
        assert!(recovered.validate_successor(&report).is_err());
    }
    #[test]
    fn preparation_reader_rejects_downgrade_stripped_layout_and_unadmitted_records() {
        let mut db = fixture();
        let mut impossible = start(&mut db);
        impossible.state = NativeImagePreparationState::Retained;
        impossible.retained_at_elapsed_ms = Some(0);
        db.execute(
            "UPDATE native_image_preparations SET state='retained',report=?1",
            [serde_json::to_vec(&impossible).unwrap()],
        )
        .unwrap();
        assert!(validate(&db).is_err());
        assert!(recover(&mut db).is_err());
        for mutation in [
            "PRAGMA user_version=20;",
            "DROP TABLE native_preparation_reader;",
            "DROP TABLE native_image_preparations;",
            "DELETE FROM native_preparation_reader;",
            "ALTER TABLE native_preparation_reader ADD COLUMN extra INTEGER;",
            "PRAGMA ignore_check_constraints=ON; UPDATE native_preparation_reader SET underlying_version=27;",
            "UPDATE native_image_preparations SET report=zeroblob(16385);",
            "UPDATE native_image_preparations SET report=CAST(' ' || CAST(report AS TEXT) AS BLOB);",
            "UPDATE native_image_preparations SET fingerprint=printf('%064d',1) || 'x';",
            "UPDATE native_image_preparations SET state='retained';",
            "UPDATE native_image_preparations SET generation=1;",
            "PRAGMA foreign_keys=OFF; UPDATE native_image_preparations SET revision=1;",
        ] {
            let mut db = fixture(); start(&mut db); db.execute_batch(mutation).unwrap();
            assert!(validate(&db).is_err(), "accepted mutation {mutation}");
            assert!(recover(&mut db).is_err(), "recovered mutation {mutation}");
        }
    }
    #[test]
    fn preparation_barrier_preserves_nested_portable_and_archive_publication() {
        let mut db = fixture();
        start(&mut db);
        let tx = db.transaction().unwrap();
        crate::portable_annotated_db::arm(&tx).unwrap();
        assert_eq!(version(&tx).unwrap(), 24);
        crate::portable_wasm_db::arm(&tx).unwrap();
        assert_eq!(version(&tx).unwrap(), 25);
        crate::wasm_archive_db::arm(&tx).unwrap();
        assert_eq!(version(&tx).unwrap(), 26);
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            27
        );
        assert_eq!(crate::wasm_archive_db::version(&db).unwrap(), 25);
        assert_eq!(crate::portable_wasm_db::storage_version(&db).unwrap(), 24);
        assert_eq!(
            crate::portable_annotated_db::storage_version(&db).unwrap(),
            20
        );
        assert_eq!(
            crate::live_cuda_evidence_db::base_version(&db, 27).unwrap(),
            20
        );
        validate(&db).unwrap();
        recover(&mut db).unwrap();
        assert_eq!(version(&db).unwrap(), 26);
    }
}
