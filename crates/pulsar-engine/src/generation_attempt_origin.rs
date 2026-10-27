//! Attempt preparation provenance. This carries no actor, permit or retry authority.
use super::{encode, internal, rev_sql, PResult};
use pulsar_protocol::*;
use rusqlite::{params, Transaction};
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) enum AttemptOrigin<'a> {
    New,
    Resume(&'a WorkerRequest),
    Fallback(&'a WorkerRequest),
}

// Both checks name the immutable predecessor, not just its logical job. An
// assistant pause remains resumable; an ordinary failure is never a fallback
// admission. The controller must first retain its pending eligibility intent.
const PREDECESSOR: &str = "id=?1 AND project=?2 AND base_revision=?3 AND source=?4 AND attempt=?5 AND manifest=?6 AND state=?7 AND EXISTS(SELECT 1 FROM attempts predecessor WHERE predecessor.id=?5 AND predecessor.job=?1 AND predecessor.manifest=?6 AND ((?8='fallback' AND predecessor.state='failed') OR (?8='resume' AND predecessor.state IN ('interrupted','assistant_paused'))))";

impl<'a> AttemptOrigin<'a> {
    pub(super) fn from_resume(previous: Option<&'a WorkerRequest>) -> Self {
        previous.map_or(Self::New, Self::Resume)
    }

    pub(super) fn previous(self) -> Option<&'a WorkerRequest> {
        match self {
            Self::New => None,
            Self::Resume(previous) | Self::Fallback(previous) => Some(previous),
        }
    }

    pub(super) fn resume(self) -> Option<&'a WorkerRequest> {
        match self {
            Self::Resume(previous) => Some(previous),
            Self::New | Self::Fallback(_) => None,
        }
    }

    pub(super) fn retained_budget(self) -> Option<&'a ResourceBudget> {
        self.resume().map(|previous| &previous.budget)
    }

    pub(super) fn retained_dependencies(self) -> &'a [ArtifactIdentity] {
        self.resume()
            .map_or(&[], |previous| previous.dependencies.as_slice())
    }

    pub(super) fn validate_context(
        self,
        project: &ProjectId,
        source: &SourceVersionId,
    ) -> PResult<()> {
        if let Some(previous) = self.previous() {
            previous.validate()?;
            if previous.project_id != *project || previous.source.source_version != *source {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "successor project or source differs from its original attempt",
                ));
            }
        }
        Ok(())
    }

    fn predecessor_state(self) -> (&'static str, &'static str) {
        match self {
            Self::Fallback(_) => ("fallback_pending", "fallback"),
            Self::Resume(_) => ("interrupted", "resume"),
            Self::New => ("", ""),
        }
    }

    /// Read-only early check before resource or output-directory admission.
    /// Publication repeats the same predicate in its conditional update.
    pub(super) fn validate_current(self, tx: &Transaction<'_>) -> PResult<()> {
        let Some(previous) = self.previous() else {
            return Ok(());
        };
        let (state, mode) = self.predecessor_state();
        let raw = encode(previous)?;
        let present: bool = tx
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM jobs WHERE {PREDECESSOR})"),
                params![
                    previous.job_id.as_str(),
                    previous.project_id.as_str(),
                    rev_sql(previous.base_revision)?,
                    previous.source.source_version.as_str(),
                    previous.attempt_id.as_str(),
                    raw,
                    state,
                    mode,
                ],
                |row| row.get(0),
            )
            .map_err(internal)?;
        if !present {
            return Err(stale_predecessor());
        }
        Ok(())
    }

    pub(super) fn create_output_directory(self, path: &Path) -> PResult<()> {
        if let Self::Fallback(previous) = self {
            if path == previous.output_dir {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "fallback cannot reuse the predecessor output directory",
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                // No recursive creation or acceptance of an existing leaf. A
                // prior checkpoint, symlink or colliding attempt is refused.
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(path)
                    .map_err(internal)?;
            }
            #[cfg(not(unix))]
            return Err(ProtocolError::unsupported(
                "fresh fallback output directory admission",
            ));
        }
        crate::artifacts::private_directory(path).map_err(internal)
    }

    /// Called before initial-checkpoint publication. Fallback creates a fresh
    /// boundary and cannot inherit any files from a failed analysis attempt.
    pub(super) fn validate_successor(self, next: &WorkerRequest) -> PResult<()> {
        if let Self::Fallback(previous) = self {
            validate_inference_fallback_successor(previous, next)?;
            if next.output_dir == previous.output_dir {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "fallback output directory is not fresh",
                ));
            }
            crate::artifacts::private_directory(&next.output_dir).map_err(internal)?;
            if std::fs::read_dir(&next.output_dir)
                .map_err(internal)?
                .next()
                .transpose()
                .map_err(internal)?
                .is_some()
            {
                return Err(ProtocolError::new(
                    ErrorCode::DependencyMismatch,
                    "fallback output must be empty before its original checkpoint is published",
                ));
            }
        }
        Ok(())
    }

    /// No actor or receipt is created here. The caller owns authorization and
    /// commits this change with the new attempt and its fallback ledger entry.
    pub(super) fn advance_job(self, tx: &Transaction<'_>, next: &WorkerRequest) -> PResult<()> {
        let previous = self.previous().ok_or_else(|| {
            ProtocolError::invalid("a new job cannot advance a predecessor attempt")
        })?;
        self.validate_context(&next.project_id, &next.source.source_version)?;
        next.validate()?;
        if next.job_id != previous.job_id
            || next.base_revision != previous.base_revision
            || next.attempt_id == previous.attempt_id
            || next.output_dir == previous.output_dir
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "successor must preserve its original job/base and use a distinct attempt/output",
            ));
        }
        if matches!(self, Self::Fallback(_)) {
            validate_inference_fallback_successor(previous, next)?;
        }
        let (state, mode) = self.predecessor_state();
        let changed = tx
            .execute(
                &format!("UPDATE jobs SET attempt=?9,state='queued',manifest=?10,error=NULL,attempts=attempts+1 WHERE {PREDECESSOR} AND attempts<128 AND attempts=(SELECT COUNT(*) FROM attempts retained WHERE retained.job=?1)"),
                params![
                    previous.job_id.as_str(),
                    previous.project_id.as_str(),
                    rev_sql(previous.base_revision)?,
                    previous.source.source_version.as_str(),
                    previous.attempt_id.as_str(),
                    encode(previous)?,
                    state,
                    mode,
                    next.attempt_id.as_str(),
                    encode(next)?,
                ],
            )
            .map_err(internal)?;
        if changed != 1 {
            return Err(stale_predecessor());
        }
        Ok(())
    }
}

fn stale_predecessor() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::RequestConflict,
        "successor predecessor is no longer the current eligible attempt",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn original() -> WorkerRequest {
        serde_json::from_str(include_str!("../tests/fixtures/cuda-intent-request.json")).unwrap()
    }

    fn successor(previous: &WorkerRequest, output: &Path) -> WorkerRequest {
        let mut next = previous.clone();
        next.attempt_id = AttemptId::new("authored-successor").unwrap();
        next.output_dir = output.to_owned();
        next.cuda_execution = None;
        let runtime = next.tools.onnx_runtime.as_mut().unwrap();
        runtime.identity = ArtifactIdentity {
            artifact_id: ArtifactId::new("authored-successor-runtime").unwrap(),
            sha256: "b".repeat(64),
            byte_len: 3,
        };
        let WorkerOperation::Generate { settings, .. } = &mut next.operation else {
            unreachable!()
        };
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.schema_version = 1;
        profile.provider = InferenceProvider::Cpu;
        profile.cuda_resources = None;
        profile.runtime = runtime.identity.clone();
        next.dependencies = vec![runtime.identity.clone()];
        next
    }

    fn database(previous: &WorkerRequest, job_state: &str, attempt_state: &str) -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE jobs(id TEXT PRIMARY KEY,attempt TEXT,project TEXT,base_revision INTEGER,source TEXT,state TEXT,manifest TEXT,error TEXT,attempts INTEGER);CREATE TABLE attempts(id TEXT PRIMARY KEY,job TEXT,manifest TEXT,state TEXT);").unwrap();
        db.execute(
            "INSERT INTO jobs VALUES(?1,?2,?3,?4,?5,?6,?7,'authored error',1)",
            params![
                previous.job_id.as_str(),
                previous.attempt_id.as_str(),
                previous.project_id.as_str(),
                rev_sql(previous.base_revision).unwrap(),
                previous.source.source_version.as_str(),
                job_state,
                encode(previous).unwrap()
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO attempts VALUES(?1,?2,?3,?4)",
            params![
                previous.attempt_id.as_str(),
                previous.job_id.as_str(),
                encode(previous).unwrap(),
                attempt_state
            ],
        )
        .unwrap();
        db
    }

    #[test]
    fn fallback_has_no_retained_budget_dependencies_or_resume_boundary() {
        let previous = original();
        let fallback = AttemptOrigin::Fallback(&previous);
        assert!(fallback.previous().is_some());
        assert!(fallback.retained_budget().is_none());
        assert!(fallback.retained_dependencies().is_empty());
        assert!(fallback.resume().is_none());
        let resume = AttemptOrigin::Resume(&previous);
        assert_eq!(
            serde_json::to_vec(resume.retained_budget().unwrap()).unwrap(),
            serde_json::to_vec(&previous.budget).unwrap()
        );
        assert_eq!(resume.retained_dependencies(), previous.dependencies);
        let baseline = ResourceBudget {
            memory_bytes: 1536 << 20,
            output_bytes: 64 << 20,
            wall_time_ms: 9876,
            cpu_threads: 2,
        };
        let admitted = crate::inference_budget::admit(
            baseline.clone(),
            None,
            fallback.retained_budget(),
            4 << 30,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&admitted).unwrap(),
            serde_json::to_vec(&baseline).unwrap()
        );
        assert_ne!(admitted.wall_time_ms, previous.budget.wall_time_ms);
    }

    #[cfg(unix)]
    #[test]
    fn fallback_refuses_existing_output_or_any_inherited_checkpoint() {
        let previous = original();
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("successor");
        let origin = AttemptOrigin::Fallback(&previous);
        origin.create_output_directory(&output).unwrap();
        let next = successor(&previous, &output);
        origin.validate_successor(&next).unwrap();
        assert!(origin.create_output_directory(&output).is_err());
        std::fs::write(
            output.join("initial-progress-boundary.json"),
            b"authored old checkpoint",
        )
        .unwrap();
        assert!(origin.validate_successor(&next).is_err());
        assert!(origin
            .create_output_directory(&previous.output_dir)
            .is_err());
    }

    #[test]
    fn fallback_requires_exact_pending_predecessor_and_changes_it_only_once() {
        let previous = original();
        let next = successor(
            &previous,
            &previous.output_dir.with_file_name("authored-next"),
        );
        for (job_state, attempt_state) in [
            ("failed", "failed"),
            ("cancelled", "failed"),
            ("fallback_pending", "cancelled"),
            ("fallback_pending", "running"),
        ] {
            let mut db = database(&previous, job_state, attempt_state);
            let tx = db.transaction().unwrap();
            let origin = AttemptOrigin::Fallback(&previous);
            assert!(origin.validate_current(&tx).is_err());
            assert!(origin.advance_job(&tx, &next).is_err());
            let actual: String = tx
                .query_row("SELECT attempt FROM jobs", [], |row| row.get(0))
                .unwrap();
            assert_eq!(actual, previous.attempt_id.as_str());
        }
        let mut db = database(&previous, "fallback_pending", "failed");
        let tx = db.transaction().unwrap();
        let origin = AttemptOrigin::Fallback(&previous);
        origin.validate_current(&tx).unwrap();
        origin.advance_job(&tx, &next).unwrap();
        assert!(origin.advance_job(&tx, &next).is_err());
        let actual: (String, String, i64) = tx
            .query_row("SELECT attempt,state,attempts FROM jobs", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(
            actual,
            (next.attempt_id.to_string(), "queued".to_owned(), 2)
        );
    }

    #[test]
    fn successor_compare_and_swap_rechecks_cancellation_manifest_and_history_bounds() {
        let previous = original();
        let next = successor(
            &previous,
            &previous.output_dir.with_file_name("authored-next"),
        );
        for mutation in [
            "UPDATE jobs SET state='cancelled'",
            "UPDATE jobs SET manifest='{}'",
            "UPDATE attempts SET manifest='{}'",
            "UPDATE jobs SET attempts=128",
            "UPDATE jobs SET attempts=2",
        ] {
            let mut db = database(&previous, "fallback_pending", "failed");
            let tx = db.transaction().unwrap();
            let origin = AttemptOrigin::Fallback(&previous);
            origin.validate_current(&tx).unwrap();
            tx.execute_batch(mutation).unwrap();
            assert!(
                origin.advance_job(&tx, &next).is_err(),
                "accepted {mutation}"
            );
        }
    }

    #[test]
    fn resume_still_accepts_interrupted_and_assistant_paused_attempts() {
        let previous = original();
        let mut next = previous.clone();
        next.attempt_id = AttemptId::new("authored-resume").unwrap();
        next.output_dir = previous.output_dir.with_file_name("authored-resume");
        for state in ["interrupted", "assistant_paused"] {
            let mut db = database(&previous, "interrupted", state);
            let tx = db.transaction().unwrap();
            let origin = AttemptOrigin::Resume(&previous);
            origin.validate_current(&tx).unwrap();
            origin.advance_job(&tx, &next).unwrap();
        }
        assert!(AttemptOrigin::New
            .validate_context(&previous.project_id, &previous.source.source_version)
            .is_ok());
        assert!(AttemptOrigin::from_resume(None).previous().is_none());
    }
}
