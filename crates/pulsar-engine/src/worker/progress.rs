use super::*;
use crate::generation_progress::{Binding, Envelope, Event, Record};
use serde::Deserialize;
use std::cell::RefCell;
use std::io::{Read, Write};

thread_local! { static ACTIVE:RefCell<Option<ContextState>>=const {RefCell::new(None)}; }
struct ContextState {
    resume: Option<Binding>,
    sequence: u16,
    budget_stopped: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Snapshot {
    pub schema: u16,
    pub basis_sha256: String,
    pub origin: AttemptId,
    pub next_source_frame: u64,
    pub tracker: tracking::StrokeTrackerCheckpoint,
    pub previous_gray: Option<Vec<u8>>,
    pub previous_rgb: Option<Vec<u8>>,
    pub position_bits: u64,
    pub reviews: ReviewSpans,
    pub actions: Vec<SavedAction>,
    pub gaps: Vec<TimeRange>,
    pub gap_start: Option<ProjectTime>,
    pub samples: serde_json::Value,
    pub last_time: ProjectTime,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SavedAction {
    time: ProjectTime,
    position_bits: u64,
    evidence: EvidenceKind,
}
impl SavedAction {
    pub(super) fn save(action: &MotionAction) -> Self {
        Self {
            time: action.time(),
            position_bits: action.position().value().to_bits(),
            evidence: action.evidence(),
        }
    }
    pub(super) fn restore(self) -> Result<MotionAction> {
        Ok(MotionAction::new(
            self.time,
            NormalizedPosition::new(f64::from_bits(self.position_bits))?,
            self.evidence,
        )?)
    }
}

/// Keep floating lineage values exact while using an inert JSON state format.
pub(super) fn pack_floats(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(ref n) if n.is_f64() => {
            serde_json::json!({"pulsar_f64_bits":n.as_f64().unwrap().to_bits()})
        }
        serde_json::Value::Array(v) => {
            serde_json::Value::Array(v.into_iter().map(pack_floats).collect())
        }
        serde_json::Value::Object(v) => {
            serde_json::Value::Object(v.into_iter().map(|(k, v)| (k, pack_floats(v))).collect())
        }
        other => other,
    }
}
fn unpack_floats(value: serde_json::Value) -> Result<serde_json::Value> {
    Ok(match value {
        serde_json::Value::Object(mut v) if v.contains_key("pulsar_f64_bits") => {
            anyhow::ensure!(v.len() == 1, "checkpoint float wrapper fields");
            let bits = v
                .remove("pulsar_f64_bits")
                .and_then(|v| v.as_u64())
                .context("checkpoint float bits")?;
            serde_json::Value::Number(
                serde_json::Number::from_f64(f64::from_bits(bits))
                    .context("nonfinite checkpoint lineage")?,
            )
        }
        serde_json::Value::Object(v) => serde_json::Value::Object(
            v.into_iter()
                .map(|(k, v)| Ok((k, unpack_floats(v)?)))
                .collect::<Result<_>>()?,
        ),
        serde_json::Value::Array(v) => {
            serde_json::Value::Array(v.into_iter().map(unpack_floats).collect::<Result<_>>()?)
        }
        other => other,
    })
}
pub(super) fn active() -> bool {
    ACTIVE.with(|s| s.borrow().is_some())
}

pub(super) fn load(
    request: &WorkerRequest,
    index: &media::MediaIndex,
    dimensions: [u32; 2],
    deadline: Instant,
) -> Result<Option<Snapshot>> {
    let binding = ACTIVE.with(|s| s.borrow().as_ref().and_then(|s| s.resume.clone()));
    let Some(binding) = binding else {
        return Ok(None);
    };
    binding.record.validate(request, true)?;
    artifacts::verify_artifact(&binding.body.path, &binding.body.identity, deadline)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&binding.body.path)?
        .take(binding.record.byte_len + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 == binding.record.byte_len,
        "progress input length changed"
    );
    anyhow::ensure!(
        Instant::now() < deadline,
        "progress input deadline exhausted"
    );
    let mut snapshot: Snapshot = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        snapshot.schema == 1
            && snapshot.basis_sha256 == crate::generation_progress::basis(request)?
            && snapshot.origin == binding.record.attempt
            && snapshot.next_source_frame == binding.record.next_source_frame
            && snapshot.next_source_frame <= index.pts.len() as u64,
        "progress source/basis/cursor mismatch"
    );
    let WorkerOperation::Generate { settings, .. } = &request.operation else {
        bail!("progress generation required")
    };
    anyhow::ensure!(
        snapshot.next_source_frame == index.pts.len() as u64
            || snapshot
                .next_source_frame
                .is_multiple_of(u64::from(settings.source_sample_stride)),
        "progress cursor outside source lattice"
    );
    snapshot.samples = unpack_floats(snapshot.samples)?;
    let samples: Vec<SampleLineage> = serde_json::from_value(snapshot.samples.clone())?;
    anyhow::ensure!(
        samples.len() as u64 == binding.record.analysed_samples
            && samples.len() <= MAX_SAMPLED_FRAMES
            && snapshot.actions.len() <= samples.len()
            && snapshot.gaps.len() <= samples.len() + 16
            && snapshot.reviews.len() <= MAX_REVIEW_FLAGS,
        "progress computation count budget"
    );
    let mut last = None;
    for sample in &samples {
        let i = usize::try_from(sample.source_frame_index)?;
        anyhow::ensure!(
            i < index.pts.len()
                && (i as u64) < snapshot.next_source_frame
                && index.pts[i] == sample.source_pts
                && i % settings.source_sample_stride as usize == 0
                && last.is_none_or(|old| i > old)
                && sample.observed_boxes <= pulsar_core::perception::MAX_DETECTIONS
                && sample.selected_class.is_none_or(|c| c < 256),
            "progress sample/source mismatch"
        );
        if let Some(id) = &sample.selected_track {
            snapshot.tracker.rebind(id, &request.attempt_id)?;
        }
        last = Some(i);
    }
    anyhow::ensure!(
        samples
            .last()
            .is_some_and(|s| s.project_time_ns == snapshot.last_time.as_nanos()),
        "progress last project time mismatch"
    );
    let pixels = (dimensions[0] as usize)
        .checked_mul(dimensions[1] as usize)
        .context("progress dimensions overflow")?;
    anyhow::ensure!(
        snapshot
            .previous_gray
            .as_ref()
            .is_some_and(|v| v.len() == pixels)
            && snapshot
                .previous_rgb
                .as_ref()
                .is_none_or(|v| v.len() == pixels * 3),
        "progress pixel support mismatch"
    );
    for (code, (start, end, _)) in &snapshot.reviews {
        ReviewReason::new(code.clone())?;
        anyhow::ensure!(end > start, "progress review interval");
    }
    Ok(Some(snapshot))
}

struct BoundedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("progress serialization budget"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn emit(request: &WorkerRequest, snapshot: Snapshot) -> Result<()> {
    ACTIVE.with(|state| -> Result<()> {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return Ok(());
        };
        if state.budget_stopped {
            return Ok(());
        }
        if state.sequence >= crate::generation_progress::MAX_UPDATES {
            state.budget_stopped = true;
            return Ok(());
        }
        let mut buffer = BoundedBytes {
            bytes: Vec::new(),
            limit: usize::try_from(request.budget.output_bytes)?,
        };
        if let Err(error) = serde_json::to_writer(&mut buffer, &snapshot) {
            if error.is_io() {
                state.budget_stopped = true;
                return Ok(());
            }
            return Err(error.into());
        }
        state.sequence += 1;
        let count = snapshot
            .samples
            .as_array()
            .context("progress sample array")?
            .len() as u64;
        let record = Record {
            schema: 1,
            sequence: state.sequence,
            attempt: request.attempt_id.clone(),
            job: request.job_id.clone(),
            basis_sha256: snapshot.basis_sha256,
            next_source_frame: snapshot.next_source_frame,
            analysed_samples: count,
            body_sha256: format!("{:x}", Sha256::digest(&buffer.bytes)),
            byte_len: buffer.bytes.len() as u64,
        };
        record.validate(request, false)?;
        let mut output = std::io::stdout().lock();
        write_message(&mut output, &Event::Progress(record))?;
        for chunk in buffer.bytes.chunks(MAX_ARTIFACT_CHUNK_BYTES) {
            write_artifact_chunk(&mut output, chunk)?;
        }
        output.flush()?;
        Ok(())
    })
}
pub(super) fn recovery_receipt() -> Option<serde_json::Value> {
    ACTIVE.with(|s|s.borrow().as_ref().map(|s|serde_json::json!({"schema_version":1,"checkpoint_updates":s.sequence,
        "checkpoint_budget_stopped":s.budget_stopped,"origin":s.resume.as_ref().map(|b|serde_json::json!({"record":b.record,"body_identity":b.body.identity})),
        "reuse":"completed analysis/actions/gaps/reviews/lineage and checked tracker/pixel support; decoder replays and discards original prefix",
        "qualified":false})))
}
/// Owns the thread-local progress state across execution, including unwinding.
/// Errors while finishing the stream cannot leave a later attempt active.
struct ActiveGuard;
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        ACTIVE.with(|state| *state.borrow_mut() = None);
    }
}
fn execute_envelope(
    envelope: &Envelope,
    execute: impl FnOnce(&WorkerRequest) -> WorkerResult,
) -> Result<WorkerResult> {
    anyhow::ensure!(
        crate::generation_progress::eligible(&envelope.request),
        "CPU finite generation progress role only"
    );
    if let Some(binding) = &envelope.resume {
        binding.record.validate(&envelope.request, true)?;
    }
    ACTIVE.with(|state| -> Result<()> {
        let mut state = state.borrow_mut();
        anyhow::ensure!(
            state.is_none(),
            "generation progress attempt already active"
        );
        *state = Some(ContextState {
            resume: envelope.resume.clone(),
            sequence: 0,
            budget_stopped: false,
        });
        Ok(())
    })?;
    let _guard = ActiveGuard;
    Ok(execute(&envelope.request))
}

pub(super) fn emit_finished<W: Write>(
    request: &WorkerRequest,
    result: &WorkerResult,
    output: &mut W,
) -> Result<()> {
    write_message(output, &Event::Finished)?;
    emit_result_to(request, result, output)
}

/// Uses the existing progress state, checkpoint codec and egress protocol for
/// both native and verified closed CPU execution. The executor retains its own
/// original deadline; activating progress grants no additional time.
pub(super) fn run_envelope(
    envelope: &Envelope,
    execute: impl FnOnce(&WorkerRequest) -> WorkerResult,
) -> Result<()> {
    let result = execute_envelope(envelope, execute)?;
    emit_finished(&envelope.request, &result, &mut std::io::stdout().lock())
}

pub(crate) fn run() -> Result<()> {
    let envelope: Envelope = read_message(&mut std::io::stdin().lock())?;
    run_envelope(&envelope, execute_manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Envelope {
        Envelope {
            request: super::super::tests::cpu_request(),
            resume: None,
        }
    }
    fn failed(request: &WorkerRequest) -> WorkerResult {
        WorkerResult {
            version: PROTOCOL_VERSION,
            attempt_id: request.attempt_id.clone(),
            job_id: request.job_id.clone(),
            cuda_artifacts: None,
            provider_unavailable: None,
            result: Err(ProtocolError::new(
                ErrorCode::Unavailable,
                "authored execution refusal",
            )),
        }
    }

    #[test]
    fn shared_envelope_restores_inactive_state_on_result_panic_and_bad_eligibility() {
        assert!(!active());
        let envelope = fixture();
        let result = execute_envelope(&envelope, |request| {
            assert!(active());
            assert!(recovery_receipt().is_some());
            assert!(execute_envelope(&envelope, failed).is_err());
            assert!(active());
            failed(request)
        })
        .unwrap();
        assert!(result.result.is_err());
        assert!(!active());
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = execute_envelope(&envelope, |_| panic!("authored executor panic"));
        }))
        .is_err());
        assert!(!active());
        let mut ineligible = fixture();
        ineligible.request.operation = WorkerOperation::ProbeMedia;
        assert!(execute_envelope(&ineligible, |_| panic!("must not execute")).is_err());
        assert!(!active());
    }

    #[test]
    fn shared_envelope_refuses_resume_for_a_different_job_before_activation() {
        let mut envelope = fixture();
        envelope.resume = Some(Binding {
            record: Record {
                schema: 1,
                sequence: 1,
                attempt: AttemptId::new("authored-origin").unwrap(),
                job: JobId::new("another-job").unwrap(),
                basis_sha256: crate::generation_progress::basis(&envelope.request).unwrap(),
                next_source_frame: 1,
                analysed_samples: 1,
                body_sha256: "a".repeat(64),
                byte_len: 1,
            },
            body: ArtifactReference {
                path: envelope.request.output_dir.join("resume-progress.json"),
                identity: envelope.request.source.identity.clone(),
            },
        });
        assert!(execute_envelope(&envelope, |_| panic!("must not execute")).is_err());
        assert!(!active());
    }

    #[test]
    fn finished_frame_precedes_original_result_and_bulk_frames() {
        let envelope = fixture();
        let result = failed(&envelope.request);
        let mut bytes = Vec::new();
        emit_finished(&envelope.request, &result, &mut bytes).unwrap();
        let mut input = std::io::Cursor::new(&bytes);
        assert!(matches!(
            read_message::<_, Event>(&mut input).unwrap(),
            Event::Finished
        ));
        let decoded: WorkerResult = read_message(&mut input).unwrap();
        assert_eq!(
            serde_json::to_vec(&decoded).unwrap(),
            serde_json::to_vec(&result).unwrap()
        );
        assert_eq!(input.position() as usize, bytes.len());

        let directory = tempfile::tempdir().unwrap();
        let artifact = |name: &str, body: &[u8]| {
            let path = directory.path().join(name);
            std::fs::write(&path, body).unwrap();
            pin_artifact(&path).unwrap()
        };
        let result = WorkerResult {
            result: Ok(WorkerOutput::Generated {
                program: artifact("program", b"authored-program"),
                receipt: artifact("receipt", b"authored-receipt"),
                lineage: Vec::new(),
                review: Vec::new(),
            }),
            ..failed(&envelope.request)
        };
        bytes.clear();
        emit_finished(&envelope.request, &result, &mut bytes).unwrap();
        let mut input = std::io::Cursor::new(&bytes);
        assert!(matches!(
            read_message::<_, Event>(&mut input).unwrap(),
            Event::Finished
        ));
        let decoded: WorkerResult = read_message(&mut input).unwrap();
        assert!(decoded.result.is_ok());
        assert_eq!(
            read_artifact_chunk(&mut input).unwrap(),
            b"authored-program"
        );
        assert_eq!(
            read_artifact_chunk(&mut input).unwrap(),
            b"authored-receipt"
        );
        assert_eq!(input.position() as usize, bytes.len());
        assert!(!active());
    }

    #[test]
    fn finishing_io_failure_does_not_leave_progress_active() {
        struct Refuse;
        impl Write for Refuse {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("authored output refusal"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let envelope = fixture();
        let result = execute_envelope(&envelope, failed).unwrap();
        assert!(emit_finished(&envelope.request, &result, &mut Refuse).is_err());
        assert!(!active());
    }

    #[test]
    fn lineage_float_codec_preserves_bits_and_refuses_nonfinite_or_extra_fields() {
        for value in [
            -0.0f64,
            0.1,
            f64::MIN_POSITIVE,
            f64::MAX,
            0.9999999999999999,
        ] {
            let packed = pack_floats(serde_json::json!([value]));
            let encoded = serde_json::to_vec(&packed).unwrap();
            let restored = unpack_floats(serde_json::from_slice(&encoded).unwrap()).unwrap();
            assert_eq!(restored[0].as_f64().unwrap().to_bits(), value.to_bits());
        }
        assert!(unpack_floats(serde_json::json!({"pulsar_f64_bits":f64::NAN.to_bits()})).is_err());
        assert!(unpack_floats(serde_json::json!({"pulsar_f64_bits":0,"extra":0})).is_err());
    }
}
