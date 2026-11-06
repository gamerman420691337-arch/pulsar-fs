//! Project-authorized bounded metadata query over an immutable media snapshot.
use super::*;
impl Engine {
    pub(super) fn probe_media(
        &self,
        request: &Request,
        version: &SourceVersionId,
    ) -> PResult<ResponseBody> {
        let session = request.session.as_ref().ok_or_else(|| {
            ProtocolError::new(ErrorCode::Unauthorized, "media probe session required")
        })?;
        let artifact_guard = self.artifact_gate.lock().map_err(internal)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let attempt_id = id!(AttemptId);
        let key = format!(
            "probe:{}:{}:{}:{}",
            project(request)?,
            session,
            version,
            attempt_id
        );
        let (source, revision) = {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            check_revision(&db, request)?;
            source_kinds::require_media(&db, project(request)?, version)?;
            let source = self.source(&db, project(request)?, version)?;
            let revision = project_snapshot(&db, project(request)?)?.revision;
            self.workers
                .lock()
                .map_err(internal)?
                .insert(key.clone(), cancel.clone());
            (source, revision)
        };
        struct QueryLease<'a> {
            workers: &'a Mutex<HashMap<String, Arc<AtomicBool>>>,
            key: String,
        }
        impl Drop for QueryLease<'_> {
            fn drop(&mut self) {
                if let Ok(mut workers) = self.workers.lock() {
                    workers.remove(&self.key);
                }
            }
        }
        let _lease = QueryLease {
            workers: &self.workers,
            key,
        };
        let budget = ResourceBudget {
            memory_bytes: 512 * 1024 * 1024,
            output_bytes: 65536,
            wall_time_ms: self.config.worker_timeout.as_millis().min(30_000) as u64,
            cpu_threads: 1,
        };
        budget.validate()?;
        let reservation = Arc::new(
            self.pool
                .reserve_worker(budget.memory_bytes, budget.cpu_threads)?
                .with_storage(
                    budget.output_bytes,
                    fs2::available_space(&self.config.state_dir).map_err(internal)?,
                )?,
        );
        let mut tools = self.tools.as_ref().map_err(unavailable)?.clone();
        tools.onnx_runtime = None;
        let output_dir = self
            .config
            .state_dir
            .join("attempts")
            .join(attempt_id.as_str());
        artifacts::private_directory(&output_dir).map_err(internal)?;
        let manifest = WorkerRequest {
            cuda_execution: None,
            independent_audio: None,
            version: PROTOCOL_VERSION,
            attempt_id,
            job_id: id!(JobId),
            project_id: project(request)?.clone(),
            base_revision: revision,
            source,
            output_dir,
            operation: WorkerOperation::ProbeMedia,
            budget,
            dependencies: vec![
                tools.ffmpeg.identity.clone(),
                tools.ffprobe.identity.clone(),
            ],
            tools,
        };
        manifest.validate()?;
        {
            let mut db = self.db.lock().map_err(internal)?;
            let tx = db.transaction().map_err(internal)?;
            self.authorize(&tx, request, session)?;
            check_revision(&tx, request)?;
            self.arm_worker_fence(&tx)?;
            tx.commit().map_err(internal)?;
        }
        drop(artifact_guard);
        #[cfg(target_os = "linux")]
        let result = launch::execute_reserved(
            &self.config,
            &manifest,
            &cancel,
            reservation,
            self.worker_fence.clone(),
        )?;
        #[cfg(not(target_os = "linux"))]
        let result = launch::execute_reserved(&self.config, &manifest, &cancel, reservation)?;
        // A joined metadata worker has fixed empty egress; remove only its empty owned directory.
        fs::remove_dir(&manifest.output_dir).map_err(internal)?;
        artifacts::sync_dir(&self.config.state_dir.join("attempts")).map_err(internal)?;
        let db = self.db.lock().map_err(internal)?;
        self.authorize(&db, request, session)?;
        check_revision(&db, request)?;
        let WorkerOutput::MediaProbe(report) = result.output else {
            return Err(ProtocolError::invalid("metadata query output kind differs"));
        };
        report.validate()?;
        if report.project_id != manifest.project_id
            || report.revision != revision
            || report.source_version != *version
            || report.source != manifest.source.identity
        {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "metadata query source or revision differs",
            ));
        }
        Ok(ResponseBody::MediaProbe(report))
    }
}
