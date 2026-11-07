//! Bounded, actor-owned accepted-estimate retention. No candidate or export authority.
use super::*;
const MAX_SAMPLES: u16 = 4096;
const SAMPLE_RESERVATION: u64 = 16 * 1024;
const MAX_SAMPLE_JSON: usize = 8 * 1024;
pub(super) struct Recording {
    request: RequestId,
    maximum: u16,
    total: u32,
    exhausted: bool,
    samples: Option<Vec<LiveCaptureEstimate>>,
    claim: Option<crate::resources::Reservation>,
}
impl Recording {
    pub(super) fn append(&mut self, estimate: &LiveCaptureEstimate) -> PResult<()> {
        let Some(samples) = &mut self.samples else {
            return Ok(());
        };
        if samples.len() >= usize::from(self.maximum)
            || serde_json::to_vec(estimate).map_err(internal)?.len() > MAX_SAMPLE_JSON
        {
            self.exhausted = true;
            return Err(ProtocolError::new(
                ErrorCode::ResourceExhausted,
                "live recording capacity exhausted; retained samples are partial",
            ));
        }
        samples.push(estimate.clone());
        self.total = u32::try_from(samples.len()).map_err(internal)?;
        Ok(())
    }
    fn release(&mut self) {
        self.samples = None;
        self.claim = None;
    }
}
pub(super) fn page(entry: &Entry, after: u32, limit: u16) -> PResult<ResponseBody> {
    if !(1..=64).contains(&limit) {
        return Err(ProtocolError::invalid(
            "live recording page limit must be 1..=64",
        ));
    }
    let recording = entry.recording.as_ref().ok_or_else(|| {
        ProtocolError::new(ErrorCode::NotFound, "capture has no recording intent")
    })?;
    let retained = recording.samples.as_ref().map_or(0, Vec::len);
    let start = usize::try_from(after).map_err(internal)?;
    if start > retained {
        return Err(ProtocolError::invalid(
            "recording cursor exceeds retained samples",
        ));
    }
    let end = start.saturating_add(usize::from(limit)).min(retained);
    let samples = recording
        .samples
        .as_ref()
        .map_or_else(Vec::new, |v| v[start..end].to_vec());
    let profile = entry
        .report
        .options
        .inference
        .as_ref()
        .expect("recording admission")
        .profile
        .clone();
    let report = LiveRecordingPage {
        fallback: BrowserInferenceRecordingProvenance::from_capture(&entry.report)?.map(Box::new),
        cpu_manifest: entry
            .report
            .inference
            .as_ref()
            .and_then(|info| info.cpu_manifest.clone()),
        browser_media: entry.report.options.browser_media.clone(),
        model_pack: entry.report.options.model_pack.clone(),
        source_time_basis: entry.report.options.source_time_basis,
        schema: 1,
        engine_epoch: entry.report.engine_epoch.clone(),
        identity: entry.report.identity.clone(),
        clock: entry.report.clock.clone(),
        profile,
        inference_attempt: entry
            .report
            .inference
            .as_ref()
            .expect("recording inference")
            .attempt_id
            .clone(),
        recording_request_id: recording.request.clone(),
        maximum_samples: recording.maximum,
        total_samples: recording.total,
        discarded: recording.samples.is_none(),
        exhausted: recording.exhausted,
        active: entry.report.active,
        retirement: entry.report.retirement.clone(),
        after,
        next: (end < retained).then_some(u32::try_from(end).map_err(internal)?),
        samples,
        qualified: false,
    };
    report.validate()?;
    Ok(ResponseBody::LiveRecordingPage(Box::new(report)))
}
pub(super) fn command(
    engine: &Engine,
    entry: &mut Entry,
    request: &Request,
) -> PResult<ResponseBody> {
    match &request.command {
        Command::StartLiveRecording {
            maximum_samples, ..
        } => {
            if let Some(recording) = &entry.recording {
                if recording.request != request.request_id || recording.maximum != *maximum_samples
                {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "recording intent already captured; no replacement recording is authorized",
                    ));
                }
                return page(entry, 0, 64);
            }
            if !(1..=MAX_SAMPLES).contains(maximum_samples) {
                return Err(ProtocolError::invalid("recording maximum must be 1..=4096"));
            }
            let info =
                entry.report.inference.as_ref().ok_or_else(|| {
                    ProtocolError::unsupported("recording requires native inference")
                })?;
            if !entry.report.active || info.completed != 0 || finishing(entry) {
                return Err(ProtocolError::new(
                    ErrorCode::Unavailable,
                    "start recording before the first inference completion in an active capture",
                ));
            }
            let claim = engine
                .pool
                .reserve_memory(u64::from(*maximum_samples) * SAMPLE_RESERVATION)?;
            entry.recording = Some(Recording {
                request: request.request_id.clone(),
                maximum: *maximum_samples,
                total: 0,
                exhausted: false,
                samples: Some(Vec::new()),
                claim: Some(claim),
            });
            page(entry, 0, 64)
        }
        Command::LiveRecordingPage { after, limit, .. } => page(entry, *after, *limit),
        Command::DiscardLiveRecording {
            recording_request_id,
            ..
        } => {
            let recording = entry.recording.as_mut().ok_or_else(|| {
                ProtocolError::new(ErrorCode::NotFound, "capture has no recording intent")
            })?;
            if recording.request != *recording_request_id {
                return Err(ProtocolError::new(
                    ErrorCode::RequestConflict,
                    "recording release identity differs",
                ));
            }
            recording.release();
            page(entry, 0, 64)
        }
        _ => unreachable!("recording command routed"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recording_release_keeps_intent_and_count_tombstone_and_frees_capacity() {
        let pool = crate::resources::ResourcePool::new(SAMPLE_RESERVATION, 1);
        let mut recording = Recording {
            request: RequestId::new("retained-intent").unwrap(),
            maximum: 1,
            total: 1,
            exhausted: true,
            samples: Some(vec![]),
            claim: Some(pool.reserve_memory(SAMPLE_RESERVATION).unwrap()),
        };
        assert!(pool.reserve_memory(1).is_err());
        recording.release();
        assert!(pool.reserve_memory(SAMPLE_RESERVATION).is_ok());
        assert_eq!(recording.request.as_str(), "retained-intent");
        assert_eq!(recording.total, 1);
        assert!(recording.exhausted);
        recording.release();
    }
}
