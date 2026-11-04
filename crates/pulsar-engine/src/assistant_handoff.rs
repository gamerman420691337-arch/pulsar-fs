//! Owned offline input checkpoints lend admission to authenticated assistant work.
use super::*;
use std::time::{Duration, Instant};
fn marker(attempt: &AttemptId) -> String {
    format!("assistant-memory-handoff:{attempt}")
}
impl Engine {
    pub(super) fn pause_assistant_generation(
        &self,
        authorization: &Request,
        accepted: &AssistantReport,
    ) -> PResult<usize> {
        let owner = &accepted.session_id;
        let plans = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, authorization, owner)?;
            check_revision(&db, authorization)?;
            require_grant(&db, owner, &accepted.project_id, Scope::Generate)?;
            let mut stmt = db.prepare("SELECT CASE WHEN typeof(manifest)='text' AND length(CAST(manifest AS BLOB)) BETWEEN 1 AND 262144 THEN manifest ELSE NULL END FROM jobs WHERE session=?1 AND project=?2 AND state IN ('queued','running') ORDER BY rowid LIMIT 17").map_err(internal)?;
            let rows = stmt
                .query_map(params![owner.as_str(), accepted.project_id.as_str()], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .map_err(internal)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(internal)?;
            if rows.len() > 16 {
                return Err(ProtocolError::new(
                    ErrorCode::ResourceExhausted,
                    "assistant handoff job capture exceeds admission",
                ));
            }
            rows.into_iter()
                .map(|raw| {
                    let raw = raw.ok_or_else(|| {
                        ProtocolError::new(
                            ErrorCode::ResourceExhausted,
                            "handoff manifest exceeds control capture admission",
                        )
                    })?;
                    decode::<WorkerRequest>(&raw)
                })
                .collect::<PResult<Vec<_>>>()?
        };
        let mut eligible = Vec::new();
        for manifest in plans {
            manifest.validate()?;
            if !matches!(
                manifest.operation,
                WorkerOperation::Generate { .. } | WorkerOperation::GenerateInput { .. }
            ) {
                continue;
            }
            self.validate_dependencies(&manifest)?;
            self.validate_checkpoint(&manifest)?;
            eligible.push(manifest);
        }
        let mut db = self.db.lock().map_err(internal)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(internal)?;
        self.authorize(&tx, authorization, owner)?;
        check_revision(&tx, authorization)?;
        require_grant(&tx, owner, &accepted.project_id, Scope::Generate)?;
        if assistant_operations::read(&tx, &accepted.attempt_id)?.state != AssistantState::Running {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "assistant no longer running",
            ));
        }
        let mut signals = Vec::new();
        let mut paused = 0;
        for manifest in eligible {
            let job = job_snapshot(&tx, &manifest.job_id, &accepted.project_id)?;
            if job.attempt_id != manifest.attempt_id
                || !["queued", "running"].contains(&job.state.as_str())
            {
                continue;
            }
            let flag = self
                .workers
                .lock()
                .map_err(internal)?
                .get(manifest.job_id.as_str())
                .cloned();
            if job.state == "running" && flag.is_none() {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "running offline worker has no exact cancellation handle",
                ));
            }
            let changed = tx.execute("UPDATE jobs SET state='interrupted',error=?3 WHERE id=?1 AND attempt=?2 AND session=?4 AND project=?5 AND state IN ('queued','running')",params![manifest.job_id.as_str(),manifest.attempt_id.as_str(),marker(&accepted.attempt_id),owner.as_str(),accepted.project_id.as_str()]).map_err(internal)?;
            if changed == 1 {
                let changed_attempt = tx.execute("UPDATE attempts SET state='assistant_paused' WHERE id=?1 AND job=?2 AND state IN ('queued','running')",params![manifest.attempt_id.as_str(),manifest.job_id.as_str()]).map_err(internal)?;
                if changed_attempt != 1 {
                    return Err(internal("handoff attempt state differs from active job"));
                }
                event(
                    &tx,
                    &accepted.project_id,
                    accepted.base_revision,
                    EventBody::JobChanged(job_snapshot(
                        &tx,
                        &manifest.job_id,
                        &accepted.project_id,
                    )?),
                )?;
                if let Some(flag) = flag {
                    signals.push(flag);
                }
                paused += 1;
            }
        }
        tx.commit().map_err(internal)?;
        // No signal escapes a failed transaction; DB lock still fences publication.
        for flag in signals {
            flag.store(true, Ordering::Release);
        }
        Ok(paused)
    }
    pub(super) fn acquire_assistant_memory(
        &self,
        authorization: &Request,
        accepted: &AssistantReport,
        memory: u64,
        cpu_threads: u16,
        cancel: &AtomicBool,
    ) -> PResult<Reservation> {
        if cancel.load(Ordering::Acquire) {
            return Err(ProtocolError::new(
                ErrorCode::Cancelled,
                "assistant cancelled before handoff",
            ));
        }
        if cpu_threads > self.config.max_cpu_threads || memory > self.config.max_ram_bytes {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "assistant claim exceeds owner pool; no generation paused",
            ));
        }
        match self.pool.reserve_worker(memory, cpu_threads) {
            Ok(claim) => return Ok(claim),
            Err(e) if e.code == ErrorCode::ResourceExhausted => {}
            Err(e) => return Err(e),
        }
        if self.pause_assistant_generation(authorization, accepted)? == 0 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "no eligible owned offline generation can lend admission",
            ));
        }
        let deadline = Instant::now()
            .checked_add(self.config.worker_timeout.min(Duration::from_secs(5)))
            .ok_or_else(|| ProtocolError::invalid("handoff deadline overflow"))?;
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(ProtocolError::new(
                    ErrorCode::Cancelled,
                    "assistant cancelled during handoff",
                ));
            }
            match self.pool.reserve_worker(memory, cpu_threads) {
                Ok(claim) => return Ok(claim),
                Err(e) if e.code == ErrorCode::ResourceExhausted => {}
                Err(e) => return Err(e),
            }
            if Instant::now() >= deadline {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "offline retirement/admission did not complete within handoff deadline",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub(super) fn resume_assistant_generation(
        self: &Arc<Self>,
        authorization: &Request,
        accepted: &AssistantReport,
    ) -> PResult<bool> {
        let wanted = marker(&accepted.attempt_id);
        let ids = {
            let db = self.db.lock().map_err(internal)?;
            let mut stmt=db.prepare("SELECT id FROM jobs WHERE session=?1 AND project=?2 AND state='interrupted' AND error=?3 ORDER BY rowid LIMIT 16").map_err(internal)?;
            let rows = stmt
                .query_map(
                    params![
                        accepted.session_id.as_str(),
                        accepted.project_id.as_str(),
                        wanted
                    ],
                    |r| r.get::<_, String>(0),
                )
                .map_err(internal)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(internal)?;
            rows
        };
        let mut complete = true;
        for id in ids {
            let mut request = authorization.clone();
            request.request_id = id!(RequestId);
            request.expected_revision = None;
            request.command = Command::ResumeJob {
                job_id: JobId::new(id.clone()).map_err(internal)?,
            };
            let response = self.handle(request);
            if response.result.is_err() {
                // Keep the marker so later observation can retry after actual custody
                // retirement. No token is persisted and no new authority is invented.
                let db = self.db.lock().map_err(internal)?;
                let state = job_snapshot(
                    &db,
                    &JobId::new(id).map_err(internal)?,
                    &accepted.project_id,
                )?;
                if state.state == "interrupted" {
                    complete = false;
                }
            }
        }
        Ok(complete)
    }
}
