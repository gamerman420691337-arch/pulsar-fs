//! Captures original Engine records into durable, uncommitted candidate lineage.
use super::*;

impl Engine {
    pub(super) fn prepare_live_recording_candidate(
        &self,
        request: &Request,
        session: &SessionId,
        options: &LiveRecordingStageOptions,
    ) -> PResult<editing::PreparedFunscript> {
        options.validate()?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            check_revision(&db, request)?;
        }
        let pages = self
            .live_captures
            .lock()
            .map_err(internal)?
            .recording_pages(request, session, options)?;
        {
            let db = self.db.lock().map_err(internal)?;
            self.authorize(&db, request, session)?;
            check_revision(&db, request)?;
            for page in &pages {
                crate::live_cpu_history::validate_recording_page(&db, session, page)
                    .map_err(internal)?;
            }
        }
        let assembly =
            assemble_live_recordings(&pages, options.range, options.maximum_interpolation_ns)?;
        validate_program(&assembly.program)?;
        let mut proof = serde_json::json!({
            "schema":1,"kind":"engine-owned-live-recording-source","qualified":false,
            "stage_request_id":request.request_id,"actor":session,"options":options,
            "source_time_basis":pages[0].source_time_basis,"contains_cuda_provisional":assembly.contains_cuda_provisional,"pages":pages
        });
        if let Some(choice) = &pages[0].model_pack {
            proof["model_pack"] = serde_json::to_value(choice).map_err(internal)?;
        }
        if let Some(binding) = &pages[0].browser_media {
            proof["browser_media"] = serde_json::to_value(binding).map_err(internal)?;
        }
        let bytes = serde_json::to_vec(&proof).map_err(internal)?;
        if bytes.len() > 40 * 1024 * 1024 {
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "recording provenance exceeds 40 MiB",
            ));
        }
        let mut temporary =
            tempfile::NamedTempFile::new_in(self.config.state_dir.join("snapshots"))
                .map_err(unavailable)?;
        temporary.write_all(&bytes).map_err(unavailable)?;
        temporary.as_file().sync_all().map_err(unavailable)?;
        let source = self.snapshot_model(temporary.path())?;
        let mut review = vec![ReviewFlag {
            axis: Some(Axis::Stroke),
            range: options.range,
            reason: ReviewReason::new("live_recording_unqualified")?,
            evidence: EvidenceKind::Inferred,
            confidence: None,
        }];
        if pages[0].source_time_basis.is_capture_relative() {
            review.push(ReviewFlag {
                axis: Some(Axis::Stroke),
                range: options.range,
                reason: ReviewReason::new("live_capture_time_not_media_time")?,
                evidence: EvidenceKind::Inferred,
                confidence: None,
            });
        }
        if pages[0].browser_media.is_some() {
            review.push(ReviewFlag {
                axis: Some(Axis::Stroke),
                range: options.range,
                reason: ReviewReason::new("browser_source_clock_declared")?,
                evidence: EvidenceKind::Inferred,
                confidence: None,
            });
        }
        if assembly.contains_cuda_provisional {
            review.push(ReviewFlag {
                axis: Some(Axis::Stroke),
                range: options.range,
                reason: ReviewReason::new("live_cuda_provisional")?,
                evidence: EvidenceKind::Inferred,
                confidence: None,
            });
        }
        for gap in assembly.program.tracks()[0].gaps() {
            review.push(ReviewFlag {
                axis: Some(Axis::Stroke),
                range: *gap,
                reason: ReviewReason::new("live_recording_missing_coverage")?,
                evidence: EvidenceKind::Unavailable,
                confidence: None,
            });
        }
        Ok(editing::PreparedFunscript {
            program: self.motion_store.publish(&assembly.program)?,
            identity: source.identity,
            path: source.path,
            label: "Live accepted estimates (unqualified)".into(),
            review,
        })
    }
}
