//! Ephemeral capture-session lifecycle, not acquisition or playback authority.
use crate::*;

fn validate_error(error: &Option<ProtocolError>) -> Result<(), ProtocolError> {
    if error.as_ref().is_some_and(|error| {
        error.message.is_empty() || error.message.len() > 2048 || error.message.contains('\0')
    }) {
        return Err(ProtocolError::invalid(
            "invalid browser analysis transfer error observation",
        ));
    }
    Ok(())
}
use serde::{Deserialize, Serialize};

/// Only successor capabilities/lease vary. Source/profile/policy/mapping are
/// derived from the actual predecessor under Engine ownership.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAnalysisSuccessorOptions {
    pub browser_media: BrowserMediaBinding,
    pub browser_analysis: BrowserAnalysisCaptureDeclaration,
    pub lease_ms: u32,
}
impl BrowserAnalysisSuccessorOptions {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.browser_media.validate()?;
        self.browser_analysis.validate()?;
        if self.lease_ms == 0 || self.lease_ms > 30_000 {
            return Err(ProtocolError::invalid(
                "successor capture lease exceeds bound",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAnalysisSuccessorPhase {
    Preparing,
    Prepared,
    Transferring,
    Active,
    Aborted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAnalysisSuccessorReport {
    pub original_request_id: RequestId,
    pub predecessor: LiveIdentity,
    pub successor: LiveCaptureReport,
    pub phase: BrowserAnalysisSuccessorPhase,
    pub transfer_request_id: Option<RequestId>,
}
impl BrowserAnalysisSuccessorReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.successor.validate()?;
        if self.predecessor.epoch == self.successor.identity.epoch
            || self.predecessor.project != self.successor.identity.project
            || self.predecessor.revision != self.successor.identity.revision
            || self.predecessor.source != self.successor.identity.source
            || self.successor.options.browser_analysis.is_none()
            || self
                .successor
                .options
                .inference
                .as_ref()
                .is_none_or(|i| i.profile.provider != InferenceProvider::Cpu)
            || self.transfer_request_id.as_ref() == Some(&self.original_request_id)
            || (matches!(
                self.phase,
                BrowserAnalysisSuccessorPhase::Transferring | BrowserAnalysisSuccessorPhase::Active
            ) && self.transfer_request_id.is_none())
        {
            return Err(ProtocolError::invalid(
                "inconsistent browser analysis successor",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAnalysisTransferPhase {
    BoundaryReserved,
    JoiningPredecessor,
    AwaitingSuccessorPair,
    ReadyForTransfer,
    Transferred,
    Aborted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserAnalysisTransferReport {
    pub original_request_id: RequestId,
    pub preparation_request_id: RequestId,
    pub predecessor: LiveIdentity,
    pub successor: LiveIdentity,
    pub boundary: LiveFrameHeader,
    pub phase: BrowserAnalysisTransferPhase,
    pub predecessor_worker_joined: bool,
    pub predecessor_delivery_settled: bool,
    pub successor_pair_accepted: bool,
    pub error: Option<ProtocolError>,
}
impl BrowserAnalysisTransferReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.boundary.validate()?;
        validate_error(&self.error)?;
        if self.original_request_id == self.preparation_request_id
            || self.predecessor.epoch == self.successor.epoch
            || self.predecessor.project != self.successor.project
            || self.predecessor.revision != self.successor.revision
            || self.predecessor.source != self.successor.source
            || (matches!(
                self.phase,
                BrowserAnalysisTransferPhase::ReadyForTransfer
                    | BrowserAnalysisTransferPhase::Transferred
            ) && (!self.predecessor_worker_joined
                || !self.predecessor_delivery_settled
                || !self.successor_pair_accepted))
            || (self.predecessor_worker_joined && !self.predecessor_delivery_settled)
            || (self.error.is_some() && self.phase != BrowserAnalysisTransferPhase::Aborted)
        {
            return Err(ProtocolError::invalid(
                "inconsistent browser analysis transfer",
            ));
        }
        Ok(())
    }
}
pub(crate) fn validate_browser_analysis_successor_request(
    request: &Request,
) -> Result<(), ProtocolError> {
    match &request.command {
        Command::PrepareBrowserAnalysisSuccessor { options, .. } => options.validate()?,
        Command::TransferBrowserAnalysisInput {
            predecessor,
            successor,
            boundary,
        } => {
            boundary.validate()?;
            if predecessor == successor {
                return Err(ProtocolError::invalid(
                    "source transfer requires distinct epochs",
                ));
            }
        }
        Command::BrowserAnalysisTransferStatus {
            transfer_request_id,
        }
        | Command::CancelBrowserAnalysisTransfer {
            transfer_request_id,
        } if request.request_id == *transfer_request_id || request.expected_revision.is_some() => {
            return Err(ProtocolError::invalid(
                "transfer status/cancel requires a distinct request and no revision",
            ));
        }
        _ => {}
    }
    Ok(())
}
/// Selects when a continuation replaces its predecessor. This grants no playback authority.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveContinuationMode {
    #[default]
    Immediate,
    WhenReady,
}
impl LiveContinuationMode {
    pub fn is_immediate(&self) -> bool {
        *self == Self::Immediate
    }
}
/// Producer declaration only; this is not source-player or physical clock calibration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveSourceTimeBasis {
    #[default]
    ProducerDeclared,
    SourceMediaPresentation,
    DisplayCapturePresentation,
    CameraCapturePresentation,
}
impl LiveSourceTimeBasis {
    pub fn is_capture_relative(&self) -> bool {
        matches!(
            self,
            Self::DisplayCapturePresentation | Self::CameraCapturePresentation
        )
    }
    pub fn is_producer_declared(&self) -> bool {
        *self == Self::ProducerDeclared
    }
}

#[cfg(test)]
mod pack_option_tests {
    use super::*;
    #[test]
    fn unresolved_pack_is_an_explicit_request_and_legacy_omits_it() {
        let mut options = LiveCaptureOptions {
            browser_analysis: None,
            browser_media: None,
            model_pack: Some(crate::ModelPackChoice {
                manifest_sha256: "a".repeat(64),
                selection_revision: 1,
            }),
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            continuation_mode: LiveContinuationMode::WhenReady,
            encoded_source: None,
            inference: None,
            policy: LivePolicy {
                maximum_analysis_age_ns: 1_000_000_000,
                added_buffer_ns: 0,
            },
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            lease_ms: 1000,
        };
        options.validate().unwrap();
        assert!(serde_json::to_value(&options)
            .unwrap()
            .get("model_pack")
            .is_some());
        options.model_pack = None;
        assert!(options.validate().is_err());
        options.continuation_mode = LiveContinuationMode::Immediate;
        options.validate().unwrap();
        assert!(serde_json::to_value(&options)
            .unwrap()
            .get("model_pack")
            .is_none());
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_analysis: Option<BrowserAnalysisCaptureDeclaration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_media: Option<BrowserMediaBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<crate::ModelPackChoice>,
    #[serde(
        default,
        skip_serializing_if = "LiveSourceTimeBasis::is_producer_declared"
    )]
    pub source_time_basis: LiveSourceTimeBasis,
    #[serde(default, skip_serializing_if = "LiveContinuationMode::is_immediate")]
    pub continuation_mode: LiveContinuationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_source: Option<LiveEncodedSourceOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<LiveCaptureInference>,
    pub policy: LivePolicy,
    pub source_anchor: SourceTimestamp,
    pub project_anchor: ProjectTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_project_anchor: Option<ExactSeconds>,
    pub presentation_anchor: LiveClockTick,
    pub lease_ms: u32,
}
impl LiveCaptureOptions {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(analysis) = &self.browser_analysis {
            analysis.validate()?;
            if self.browser_media.is_none()
                || self.encoded_source.is_some()
                || self.source_time_basis != LiveSourceTimeBasis::SourceMediaPresentation
            {
                return Err(ProtocolError::invalid(
                    "browser analysis requires bound source-media RGB capture",
                ));
            }
        }
        if let Some(binding) = &self.browser_media {
            if self.encoded_source.is_some() {
                return Err(ProtocolError::invalid(
                    "bound browser media requires RGB capture",
                ));
            }
            binding.validate_capture_anchor(
                self.source_time_basis,
                self.source_anchor,
                self.project_anchor,
                self.exact_project_anchor,
            )?;
        }
        if let Some(choice) = &self.model_pack {
            choice.validate()?;
        }
        if self.continuation_mode == LiveContinuationMode::WhenReady
            && self.inference.is_none()
            && self.model_pack.is_none()
        {
            return Err(ProtocolError::invalid(
                "prepared continuation requires inference",
            ));
        }
        if let Some(source) = &self.encoded_source {
            if self.source_time_basis.is_capture_relative() {
                return Err(ProtocolError::invalid(
                    "encoded media cannot declare browser-capture presentation time",
                ));
            }
            source.validate()?;
        }
        if self.lease_ms == 0 || self.lease_ms > 30_000 {
            return Err(ProtocolError::invalid("capture lease must be 1..=30000ms"));
        }
        if let Some(inference) = &self.inference {
            if !inference.model_path.is_absolute()
                || inference
                    .model_path
                    .to_str()
                    .is_none_or(|path| path.len() > 4096 || path.contains('\0'))
            {
                return Err(ProtocolError::invalid(
                    "live model path must be bounded and absolute",
                ));
            }
            inference.profile.validate()?;
        }
        let epoch = AttemptId::new("validation").expect("constant");
        let clock = self.clock(epoch.clone());
        clock
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        LiveScheduler::with_presentation_clock(
            LiveIdentity {
                epoch,
                project: ProjectId::new("validation").expect("constant"),
                revision: RevisionId::new(0),
                source: SourceVersionId::new("validation").expect("constant"),
            },
            self.policy,
            clock,
            LiveClockTick(0),
        )
        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        Ok(())
    }
    pub fn clock(&self, epoch: AttemptId) -> LivePresentationClock {
        LivePresentationClock {
            epoch,
            source_anchor: self.source_anchor,
            project_anchor: self.project_anchor,
            exact_project_anchor: self.exact_project_anchor,
            presentation_anchor: self.presentation_anchor,
            added_buffer_ns: self.policy.added_buffer_ns,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCaptureRetirement {
    Completed,
    Superseded,
    WorkerFailed,
    Stopped,
    Expired,
    RevisionChanged,
    AuthorityLost,
    EngineRestarted,
    LeaseEvicted,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_close: Option<LiveCaptureInputCloseReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<LiveCaptureContinuation>,
    #[serde(default)]
    pub age_basis: LiveCaptureAgeBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoded_source: Option<LiveEncodedSourceReport>,
    pub engine_epoch: AttemptId,
    pub identity: LiveIdentity,
    pub clock: LivePresentationClock,
    pub options: LiveCaptureOptions,
    pub active: bool,
    pub retirement: Option<LiveCaptureRetirement>,
    pub server_elapsed: Option<LiveClockTick>,
    pub admitted_pixel_bytes: u64,
    pub received_frames: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_frame: Option<LiveFrameHeader>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<LiveCaptureInferenceReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<LiveQueueStatus>,
}
impl LiveCaptureReport {
    /// Derive an explicit fresh-epoch admission from a currently active report.
    /// This transfers declared project phase only, never clock or frame authority.
    pub fn continuation_options(
        &self,
        source_anchor: SourceTimestamp,
        lease_ms: u32,
    ) -> Result<LiveCaptureOptions, ProtocolError> {
        self.validate()?;
        if !self.active {
            return Err(ProtocolError::new(
                ErrorCode::DependencyMismatch,
                "retired capture cannot derive continuation",
            ));
        }
        let floor = self
            .last_frame
            .as_ref()
            .map_or(self.options.source_anchor, |f| f.source_time);
        if ExactSeconds::from_source(source_anchor)
            .checked_cmp(ExactSeconds::from_source(floor))
            .map_err(|e| ProtocolError::invalid(e.to_string()))?
            != std::cmp::Ordering::Greater
        {
            return Err(ProtocolError::invalid(
                "continuation source anchor must advance past predecessor",
            ));
        }
        let exact = self
            .clock
            .project_at_source(source_anchor)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        let mut options = self.options.clone();
        options.source_anchor = source_anchor;
        options.project_anchor = exact
            .quantize_nanoseconds()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?
            .time;
        options.exact_project_anchor = (options.browser_media.is_some()
            || exact != ExactSeconds::from_project(options.project_anchor))
        .then_some(exact);
        options.presentation_anchor = LiveClockTick(0);
        options.lease_ms = lease_ms;
        options.validate()?;
        Ok(options)
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.options.validate()?;
        if let Some(input) = &self.input_close {
            input.validate(self)?;
        }
        if let Some(binding) = &self.options.browser_media {
            if binding.engine_epoch != self.engine_epoch
                || binding.project_id != self.identity.project
                || binding.revision != self.identity.revision
                || binding.selection.source_version != self.identity.source
            {
                return Err(ProtocolError::invalid(
                    "capture browser media identity differs",
                ));
            }
        }
        if self.options.model_pack.is_some()
            && (self.options.inference.is_none() || self.inference.is_none())
        {
            return Err(ProtocolError::invalid(
                "pack capture report lacks resolved inference",
            ));
        }
        if self.options.continuation_mode == LiveContinuationMode::WhenReady
            && self.continuation.is_none()
        {
            return Err(ProtocolError::invalid(
                "prepared capture requires a predecessor",
            ));
        }
        if self.retirement == Some(LiveCaptureRetirement::Completed)
            && self
                .inference
                .as_ref()
                .is_none_or(|i| i.cuda_evidence.is_none())
        {
            return Err(ProtocolError::invalid(
                "completed capture requires retained final evidence",
            ));
        }
        if let Some(continuation) = &self.continuation {
            continuation.validate(&self.identity, &self.options)?;
        }
        let basis = if self.options.encoded_source.is_some() {
            LiveCaptureAgeBasis::DecodedStdoutLocalMonotonicEqualRate
        } else {
            LiveCaptureAgeBasis::ProducerDeclared
        };
        if self.age_basis != basis
            || self.encoded_source.is_some() != self.options.encoded_source.is_some()
        {
            return Err(ProtocolError::invalid("capture source/age basis differs"));
        }
        if let Some(source) = &self.encoded_source {
            source.validate(self)?;
        }
        if let Some(queue) = &self.queue {
            if !self.active
                || !queue.connected
                || queue.received != self.received_frames
                || queue.pending_frames > 1
                || queue.in_flight_frames > 1
            {
                return Err(ProtocolError::invalid("live queue status differs"));
            }
        }
        if self.inference.is_some() != self.options.inference.is_some() {
            return Err(ProtocolError::invalid(
                "capture inference admission differs",
            ));
        }
        if let Some(inference) = &self.inference {
            inference.validate(self)?;
        }
        if (self.received_frames == 0) != self.last_frame.is_none() {
            return Err(ProtocolError::invalid(
                "capture frame receipt count differs",
            ));
        }
        if let Some(frame) = &self.last_frame {
            frame.validate()?;
            if self.continuation.is_some()
                && ExactSeconds::from_source(frame.source_time)
                    .checked_cmp(ExactSeconds::from_source(self.options.source_anchor))
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?
                    == std::cmp::Ordering::Less
            {
                return Err(ProtocolError::invalid(
                    "continuation receipt precedes source anchor",
                ));
            }
        }
        if self.clock != self.options.clock(self.identity.epoch.clone())
            || self.active == self.retirement.is_some()
            || self.admitted_pixel_bytes != if self.active { 5 * 64 * 64 * 3 } else { 0 }
        {
            return Err(ProtocolError::invalid(
                "capture report clock, lifetime or pixel admission differs",
            ));
        }
        if self.active && self.server_elapsed.is_none() {
            return Err(ProtocolError::invalid(
                "active capture requires its current server elapsed clock",
            ));
        }
        Ok(())
    }
}
/// Explicit predecessor facts. Clock/pixel/inference authority never crosses epochs.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureContinuation {
    pub predecessor: LiveIdentity,
    pub predecessor_clock: LivePresentationClock,
    pub predecessor_elapsed: LiveClockTick,
    pub predecessor_last_frame: Option<LiveFrameHeader>,
}
impl LiveCaptureContinuation {
    pub fn validate(
        &self,
        identity: &LiveIdentity,
        options: &LiveCaptureOptions,
    ) -> Result<(), ProtocolError> {
        if self.predecessor.epoch == identity.epoch
            || self.predecessor.project != identity.project
            || self.predecessor.revision != identity.revision
            || self.predecessor.source != identity.source
            || self.predecessor_clock.epoch != self.predecessor.epoch
            || self.predecessor_elapsed.0 > 30_000_000_000
            || options.presentation_anchor != LiveClockTick(0)
        {
            return Err(ProtocolError::invalid(
                "continuation identity or lease clock differs",
            ));
        }
        let floor = self
            .predecessor_last_frame
            .as_ref()
            .map_or(self.predecessor_clock.source_anchor, |f| f.source_time);
        if i128::from(options.source_anchor.numerator()) * i128::from(floor.denominator())
            <= i128::from(floor.numerator()) * i128::from(options.source_anchor.denominator())
        {
            return Err(ProtocolError::invalid(
                "continuation source anchor must advance past predecessor",
            ));
        }
        if let Some(frame) = &self.predecessor_last_frame {
            frame.validate()?;
            if frame.capture_tick > self.predecessor_elapsed {
                return Err(ProtocolError::invalid(
                    "predecessor frame exceeds its elapsed clock",
                ));
            }
        }
        let mapped = self
            .predecessor_clock
            .project_at_source(options.source_anchor)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        let successor = options
            .clock(identity.epoch.clone())
            .project_at_source(options.source_anchor)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if mapped != successor {
            return Err(ProtocolError::invalid(
                "continuation must preserve exact project correspondence at its anchor",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureLimits {
    #[serde(default)]
    pub epoch_continuation: bool,
    #[serde(default)]
    pub encoded_source_ingress: bool,
    pub engine_epoch: AttemptId,
    pub maximum_active: u32,
    pub maximum_lease_ms: u32,
    pub native_acquisition: bool,
    pub continuous_inference: bool,
    pub physical_dispatch: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuation_contract_preserves_exact_correspondence_and_distinct_epochs() {
        let prior = LiveIdentity {
            epoch: AttemptId::new("prior").unwrap(),
            project: ProjectId::new("p").unwrap(),
            revision: RevisionId::new(0),
            source: SourceVersionId::new("s").unwrap(),
        };
        let next = LiveIdentity {
            epoch: AttemptId::new("next").unwrap(),
            ..prior.clone()
        };
        let mut options = LiveCaptureOptions {
            browser_analysis: None,
            browser_media: None,
            model_pack: None,
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            continuation_mode: LiveContinuationMode::Immediate,
            encoded_source: None,
            inference: None,
            policy: LivePolicy {
                maximum_analysis_age_ns: 200_000_000,
                added_buffer_ns: 0,
            },
            source_anchor: SourceTimestamp::new(1, 1).unwrap(),
            project_anchor: ProjectTime::from_nanos(1_000_000_000),
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            lease_ms: 30_000,
        };
        let mut facts = LiveCaptureContinuation {
            predecessor: prior.clone(),
            predecessor_clock: LivePresentationClock {
                epoch: prior.epoch.clone(),
                source_anchor: SourceTimestamp::new(0, 1).unwrap(),
                project_anchor: ProjectTime::ZERO,
                exact_project_anchor: None,
                presentation_anchor: LiveClockTick(0),
                added_buffer_ns: 0,
            },
            predecessor_elapsed: LiveClockTick(10),
            predecessor_last_frame: None,
        };
        facts.validate(&next, &options).unwrap();
        assert!(facts.validate(&prior, &options).is_err());
        let wrong = LiveIdentity {
            source: SourceVersionId::new("foreign").unwrap(),
            ..next.clone()
        };
        assert!(facts.validate(&wrong, &options).is_err());
        options.source_anchor = SourceTimestamp::new(1, 3).unwrap();
        options.project_anchor = ProjectTime::from_nanos(333_333_333);
        assert!(facts.validate(&next, &options).is_err());
        options.exact_project_anchor = Some(ExactSeconds::new(1, 3).unwrap());
        facts.validate(&next, &options).unwrap();
        options.exact_project_anchor = Some(ExactSeconds::new(666_666_667, 2_000_000_000).unwrap());
        assert!(facts.validate(&next, &options).is_err());
        options.exact_project_anchor = None;
        options.source_anchor = SourceTimestamp::new(1, 1).unwrap();
        options.project_anchor = ProjectTime::from_nanos(1_000_000_000);
        facts.predecessor_last_frame = Some(LiveFrameHeader {
            frame: FrameId::new(0),
            source_time: SourceTimestamp::new(1, 1).unwrap(),
            capture_tick: LiveClockTick(10),
            sha256: "a".repeat(64),
        });
        assert!(facts.validate(&next, &options).is_err());
        facts.predecessor_last_frame.as_mut().unwrap().source_time =
            SourceTimestamp::new(0, 1).unwrap();
        facts.predecessor_last_frame.as_mut().unwrap().capture_tick = LiveClockTick(11);
        assert!(facts.validate(&next, &options).is_err());
    }
    #[test]
    fn capture_options_refuse_unbounded_lifetimes_and_require_project_revision() {
        let mut options = LiveCaptureOptions {
            browser_analysis: None,
            browser_media: None,
            model_pack: None,
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            continuation_mode: LiveContinuationMode::Immediate,
            encoded_source: None,
            inference: None,
            policy: LivePolicy {
                maximum_analysis_age_ns: 200_000_000,
                added_buffer_ns: 0,
            },
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            lease_ms: 30_000,
        };
        options.validate().unwrap();
        options.lease_ms = 30_001;
        assert!(options.validate().is_err());
        options.lease_ms = 30_000;
        options.policy.added_buffer_ns = 1_000_000_001;
        assert!(options.validate().is_err());
        let request = Request::new(
            RequestId::new("capture").unwrap(),
            Command::LiveCaptureStatus {
                epoch: AttemptId::new("epoch").unwrap(),
            },
        );
        assert!(request.validate().is_err());
        assert!(request
            .in_project(ProjectId::new("p").unwrap(), None)
            .validate()
            .is_err());
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureInference {
    pub model_path: std::path::PathBuf,
    pub profile: DeclaredInferenceProfile,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCaptureInferenceState {
    Starting,
    Running,
    InputSealed,
    /// Actual predecessor CPU custody joined for a pending source transfer.
    /// This is not final EOF, successor activation or playback authority.
    TransferJoined,
    FinishRequested,
    Finishing,
    Retired,
    Failed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureEstimate {
    #[serde(default)]
    pub age_basis: LiveCaptureAgeBasis,
    pub evidence: EvidenceKind,
    pub qualified: bool,
    pub result: LiveInferenceReply,
    pub capture_tick: LiveClockTick,
    pub accepted_at: LiveClockTick,
    pub capture_to_analysis_ns: u64,
    pub capture_age_ns: u64,
    pub fresh: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureInferenceReport {
    /// Exact captured mode declared at admission, including while Starting.
    /// This metadata is not a verified runtime receipt or permission to execute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_manifest: Option<Box<LiveCpuInferenceManifest>>,
    /// Set only after the matching private GPU worker reaches Ready. A digest
    /// binding is provisional metadata, not native trace verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda_manifest_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cuda_evidence: Option<LiveCudaEvidenceRecord>,
    pub attempt_id: AttemptId,
    pub profile_sha256: String,
    pub state: LiveCaptureInferenceState,
    pub completed: u64,
    pub accepted: u64,
    pub stale: u64,
    pub latest: Option<LiveCaptureEstimate>,
    pub error: Option<ProtocolError>,
}
fn validate_captured_cpu_scope(
    manifest: &LiveCpuInferenceManifest,
    attempt: &AttemptId,
    identity: &LiveIdentity,
    clock: &LivePresentationClock,
    profile: &DeclaredInferenceProfile,
    lease_ms: Option<u64>,
) -> Result<(), ProtocolError> {
    manifest.validate()?;
    if profile.provider != InferenceProvider::Cpu
        || &manifest.stream.attempt_id != attempt
        || &manifest.stream.identity != identity
        || &manifest.stream.clock != clock
        || serde_json::to_value(&manifest.stream.profile)
            .map_err(|e| ProtocolError::invalid(e.to_string()))?
            != serde_json::to_value(profile).map_err(|e| ProtocolError::invalid(e.to_string()))?
        || lease_ms.is_some_and(|lease| manifest.stream.budget.wall_time_ms != lease)
    {
        return Err(ProtocolError::invalid(
            "captured CPU declaration differs from original live scope",
        ));
    }
    Ok(())
}
impl LiveCaptureInferenceReport {
    pub fn validate(&self, capture: &LiveCaptureReport) -> Result<(), ProtocolError> {
        validate_sha256(&self.profile_sha256)?;
        use sha2::{Digest, Sha256};
        let profile = &capture
            .options
            .inference
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("missing live admission"))?
            .profile;
        if format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(profile)
                    .map_err(|_| ProtocolError::invalid("profile encoding"))?
            )
        ) != self.profile_sha256
        {
            return Err(ProtocolError::invalid(
                "live result profile digest differs from options",
            ));
        }
        if let Some(manifest) = &self.cpu_manifest {
            validate_captured_cpu_scope(
                manifest,
                &self.attempt_id,
                &capture.identity,
                &capture.clock,
                profile,
                Some(u64::from(capture.options.lease_ms)),
            )?;
            if self.cuda_manifest_sha256.is_some()
                || self.cuda_evidence.is_some()
                || (self.state == LiveCaptureInferenceState::Starting
                    && (self.completed != 0
                        || self.accepted != 0
                        || self.stale != 0
                        || self.latest.is_some()))
            {
                return Err(ProtocolError::invalid(
                    "captured CPU declaration mixes CUDA or pre-Ready results",
                ));
            }
        }
        if let Some(hash) = &self.cuda_manifest_sha256 {
            validate_sha256(hash)?;
        }
        if (profile.provider != InferenceProvider::Cuda && self.cuda_manifest_sha256.is_some())
            || (profile.provider == InferenceProvider::Cuda
                && matches!(
                    self.state,
                    LiveCaptureInferenceState::Running
                        | LiveCaptureInferenceState::FinishRequested
                        | LiveCaptureInferenceState::Finishing
                )
                && self.cuda_manifest_sha256.is_none())
        {
            return Err(ProtocolError::invalid(
                "live CUDA report binding differs from provider or readiness",
            ));
        }
        if self.accepted.checked_add(self.stale) != Some(self.completed)
            || capture.active
                != matches!(
                    self.state,
                    LiveCaptureInferenceState::Starting
                        | LiveCaptureInferenceState::Running
                        | LiveCaptureInferenceState::FinishRequested
                        | LiveCaptureInferenceState::Finishing
                        | LiveCaptureInferenceState::InputSealed
                        | LiveCaptureInferenceState::TransferJoined
                )
            || self
                .error
                .as_ref()
                .is_some_and(|error| error.message.len() > 2048)
            || (self.state == LiveCaptureInferenceState::Failed) != self.error.is_some()
        {
            return Err(ProtocolError::invalid(
                "live inference state/counters differ",
            ));
        }
        if self.state == LiveCaptureInferenceState::InputSealed
            && (profile.provider != InferenceProvider::Cpu
                || capture.input_close.as_ref().is_none_or(|input| {
                    input.phase != LiveCaptureInputPhase::Sealed
                        || !input.worker_joined
                        || !input.delivery_settled
                }))
        {
            return Err(ProtocolError::invalid(
                "sealed inference requires settled browser CPU input",
            ));
        }
        if self.state == LiveCaptureInferenceState::TransferJoined
            && (profile.provider != InferenceProvider::Cpu
                || capture.input_close.is_some()
                || capture.options.browser_analysis.is_none()
                || self.latest.is_none())
        {
            return Err(ProtocolError::invalid(
                "transfer join requires original accepted browser CPU input",
            ));
        }
        if self.state == LiveCaptureInferenceState::FinishRequested
            && (profile.provider != InferenceProvider::Cuda
                || self.latest.is_some()
                || self.cuda_evidence.is_some()
                || (self.completed == 0
                    && capture
                        .queue
                        .as_ref()
                        .is_none_or(|q| q.in_flight_frames == 0)))
        {
            return Err(ProtocolError::invalid(
                "CUDA Finish request requires Ready work and no provisional publication",
            ));
        }
        if self.state == LiveCaptureInferenceState::Finishing
            && (profile.provider != InferenceProvider::Cuda
                || self.completed == 0
                || self.latest.is_some()
                || self.cuda_evidence.is_some()
                || capture
                    .queue
                    .as_ref()
                    .is_some_and(|q| q.in_flight_frames != 0))
        {
            return Err(ProtocolError::invalid(
                "CUDA finishing capture must freeze work and provisional publication",
            ));
        }
        if let Some(evidence) = &self.cuda_evidence {
            evidence.validate()?;
            if profile.provider != InferenceProvider::Cuda
                || capture.active
                || capture.retirement != Some(LiveCaptureRetirement::Completed)
                || self.state != LiveCaptureInferenceState::Retired
                || self.latest.is_some()
                || self.cuda_manifest_sha256.as_ref() != Some(&evidence.manifest.sha256)
                || self.completed != evidence.summary.successful_runs
                || capture
                    .server_elapsed
                    .is_none_or(|t| t.0 > u64::from(capture.options.lease_ms) * 1_000_000)
                || capture.last_frame.as_ref().is_none_or(|f| {
                    f.frame < evidence.summary.last_frame
                        || ExactSeconds::from_source(f.source_time)
                            .checked_cmp(ExactSeconds::from_source(
                                evidence.summary.last_source_time,
                            ))
                            .map_or(true, |o| o == std::cmp::Ordering::Less)
                })
            {
                return Err(ProtocolError::invalid(
                    "CUDA completed report differs from retained evidence",
                ));
            }
        }
        if let Some(latest) = &self.latest {
            let (attempt_id, identity, profile_sha256, input, elapsed) = match &latest.result {
                LiveInferenceReply::Estimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    pair_analysis_elapsed_ns,
                    ..
                } if profile.provider == InferenceProvider::Cpu
                    && self.cpu_manifest.is_none()
                    && self.cuda_manifest_sha256.is_none() =>
                {
                    (
                        attempt_id,
                        identity,
                        profile_sha256,
                        input,
                        *pair_analysis_elapsed_ns,
                    )
                }
                LiveInferenceReply::CapturedCpuEstimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    pair_analysis_elapsed_ns,
                    ..
                } if profile.provider == InferenceProvider::Cpu && self.cpu_manifest.is_some() => {
                    let manifest = self.cpu_manifest.as_ref().expect("captured declaration");
                    manifest.validate_estimated(input, &latest.result)?;
                    if latest.accepted_at.0 >= manifest.stream.budget.wall_time_ms * 1_000_000 {
                        return Err(ProtocolError::invalid(
                            "captured estimate acceptance exceeds original lease",
                        ));
                    }
                    (
                        attempt_id,
                        identity,
                        profile_sha256,
                        input,
                        *pair_analysis_elapsed_ns,
                    )
                }
                LiveInferenceReply::CudaProvisional {
                    attempt_id,
                    identity,
                    profile_sha256,
                    manifest_sha256,
                    input,
                    pair_analysis_elapsed_ns,
                    ..
                } if profile.provider == InferenceProvider::Cuda
                    && self.cpu_manifest.is_none()
                    && self.cuda_manifest_sha256.as_ref() == Some(manifest_sha256) =>
                {
                    (
                        attempt_id,
                        identity,
                        profile_sha256,
                        input,
                        *pair_analysis_elapsed_ns,
                    )
                }
                _ => {
                    return Err(ProtocolError::invalid(
                        "live estimate provider or declared runtime binding differs",
                    ))
                }
            };
            if elapsed == 0 || elapsed > u64::from(capture.options.lease_ms) * 1_000_000 {
                return Err(ProtocolError::invalid(
                    "live estimate analysis duration differs from lease",
                ));
            }
            let now = capture
                .server_elapsed
                .ok_or_else(|| ProtocolError::invalid("latest estimate needs elapsed clock"))?;
            input.pair.elapsed()?;
            input.pair.presentation_mapping()?;
            for pixels in [&input.previous, &input.current] {
                pixels.validate()?;
                if pixels.byte_len != LIVE_FRAME_BYTES as u64
                    || pixels.artifact_id.as_str() != pixels.sha256
                {
                    return Err(ProtocolError::invalid(
                        "live estimate pixel identity differs",
                    ));
                }
            }
            let age = now
                .0
                .checked_sub(latest.capture_tick.0)
                .ok_or_else(|| ProtocolError::invalid("future latest capture"))?;
            if latest.age_basis != capture.age_basis
                || latest.evidence != EvidenceKind::Inferred
                || latest.qualified
                || !matches!(
                    self.state,
                    LiveCaptureInferenceState::Running
                        | LiveCaptureInferenceState::InputSealed
                        | LiveCaptureInferenceState::TransferJoined
                )
                || *attempt_id != self.attempt_id
                || *identity != capture.identity
                || *profile_sha256 != self.profile_sha256
                || input.pair.source_epoch != identity.epoch
                || input.pair.presentation_clock.as_ref() != Some(&capture.clock)
                || latest.accepted_at.0.checked_sub(latest.capture_tick.0)
                    != Some(latest.capture_to_analysis_ns)
                || latest.capture_to_analysis_ns > capture.options.policy.maximum_analysis_age_ns
                || latest.accepted_at > now
                || self.accepted == 0
                || input.sequence == 0
                || input.sequence > self.completed
                || capture
                    .last_frame
                    .as_ref()
                    .is_none_or(|last| input.pair.current_frame > last.frame)
                || latest.capture_age_ns != age
                || latest.fresh != (age <= capture.options.policy.maximum_analysis_age_ns)
            {
                return Err(ProtocolError::invalid(
                    "latest estimate scope/freshness differs",
                ));
            }
            if self.state == LiveCaptureInferenceState::InputSealed
                && capture.input_close.as_ref().is_none_or(|close| {
                    close.disposition != Some(LiveCaptureInputFinalDisposition::Accepted)
                        || close.final_frame.as_ref().is_none_or(|frame| {
                            frame.frame != input.pair.current_frame
                                || frame.source_time != input.pair.current_time
                                || frame.capture_tick != latest.capture_tick
                                || frame.sha256 != input.current.sha256
                        })
                        || input.sequence != self.completed
                })
            {
                return Err(ProtocolError::invalid(
                    "sealed estimate differs from exact final accepted input",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod preparation_tests {
    use super::*;
    #[test]
    fn default_wire_identity_and_fail_closed_preparation() {
        // Derive timestamp encoding from the typed value rather than assuming its JSON representation.
        let options = LiveCaptureOptions {
            browser_analysis: None,
            browser_media: None,
            model_pack: None,
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            continuation_mode: LiveContinuationMode::Immediate,
            encoded_source: None,
            inference: None,
            policy: LivePolicy {
                maximum_analysis_age_ns: 200000000,
                added_buffer_ns: 0,
            },
            source_anchor: SourceTimestamp::new(0, 1).unwrap(),
            project_anchor: ProjectTime::ZERO,
            exact_project_anchor: None,
            presentation_anchor: LiveClockTick(0),
            lease_ms: 30000,
        };
        let wire = serde_json::to_value(&options).unwrap();
        assert!(wire.get("continuation_mode").is_none());
        let decoded: LiveCaptureOptions = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded.continuation_mode, LiveContinuationMode::Immediate);
        assert_eq!(serde_json::to_value(&decoded).unwrap(), wire);
        let mut invalid = wire.clone();
        invalid["continuation_mode"] = serde_json::json!("automatic");
        assert!(serde_json::from_value::<LiveCaptureOptions>(invalid).is_err());
        let mut prepared = options;
        prepared.continuation_mode = LiveContinuationMode::WhenReady;
        assert!(prepared
            .validate()
            .unwrap_err()
            .message
            .contains("requires inference"));
    }
}

/// Engine-owned accepted estimates, not client-authored or physically qualified motion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRecordingPage {
    /// Original declared captured mode; absence retains legacy host history.
    /// Full per-epoch metadata is retained as provenance, never execution authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_manifest: Option<Box<LiveCpuInferenceManifest>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_media: Option<BrowserMediaBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_pack: Option<crate::ModelPackChoice>,
    #[serde(
        default,
        skip_serializing_if = "LiveSourceTimeBasis::is_producer_declared"
    )]
    pub source_time_basis: LiveSourceTimeBasis,
    pub schema: u16,
    pub engine_epoch: AttemptId,
    pub identity: LiveIdentity,
    pub clock: LivePresentationClock,
    pub profile: DeclaredInferenceProfile,
    pub inference_attempt: AttemptId,
    pub recording_request_id: RequestId,
    pub maximum_samples: u16,
    pub total_samples: u32,
    pub discarded: bool,
    pub exhausted: bool,
    pub active: bool,
    pub retirement: Option<LiveCaptureRetirement>,
    pub after: u32,
    pub next: Option<u32>,
    pub samples: Vec<LiveCaptureEstimate>,
    pub qualified: bool,
}

impl LiveRecordingPage {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(binding) = &self.browser_media {
            binding.validate_capture_anchor(
                self.source_time_basis,
                self.clock.source_anchor,
                self.clock.project_anchor,
                self.clock.exact_project_anchor,
            )?;
            if binding.engine_epoch != self.engine_epoch
                || binding.project_id != self.identity.project
                || binding.revision != self.identity.revision
                || binding.selection.source_version != self.identity.source
            {
                return Err(ProtocolError::invalid(
                    "recorded browser media identity differs",
                ));
            }
        }
        if let Some(choice) = &self.model_pack {
            choice.validate()?;
        }
        use sha2::{Digest, Sha256};
        self.profile.validate()?;
        self.clock
            .validate()
            .map_err(|e| ProtocolError::invalid(e.to_string()))?;
        if let Some(manifest) = &self.cpu_manifest {
            validate_captured_cpu_scope(
                manifest,
                &self.inference_attempt,
                &self.identity,
                &self.clock,
                &self.profile,
                None,
            )?;
        }
        let profile_sha = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&self.profile)
                    .map_err(|e| ProtocolError::invalid(e.to_string()))?
            )
        );
        if self.schema != 1
            || self.qualified
            || !(1..=4096).contains(&self.maximum_samples)
            || self.total_samples > u32::from(self.maximum_samples)
            || self.samples.len() > 64
            || self.clock.epoch != self.identity.epoch
            || self.active == self.retirement.is_some()
            || (self.discarded
                && (!self.samples.is_empty() || self.next.is_some() || self.after != 0))
        {
            return Err(ProtocolError::invalid("live recording descriptor differs"));
        }
        if !self.discarded {
            let end = self
                .after
                .checked_add(self.samples.len() as u32)
                .ok_or_else(|| ProtocolError::invalid("recording page overflow"))?;
            if end > self.total_samples || self.next != (end < self.total_samples).then_some(end) {
                return Err(ProtocolError::invalid("recording page cursor differs"));
            }
        }
        let mut previous_sequence = 0;
        for sample in &self.samples {
            let (attempt, identity, hash, input) = match &sample.result {
                LiveInferenceReply::Estimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    ..
                } if self.profile.provider == InferenceProvider::Cpu
                    && self.cpu_manifest.is_none() =>
                {
                    (attempt_id, identity, profile_sha256, input)
                }
                LiveInferenceReply::CapturedCpuEstimated {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    ..
                } if self.profile.provider == InferenceProvider::Cpu
                    && self.cpu_manifest.is_some() =>
                {
                    let manifest = self.cpu_manifest.as_ref().expect("captured declaration");
                    manifest.validate_estimated(input, &sample.result)?;
                    // Pages retain the original manifest rather than a separate
                    // lease field. Engine provenance must bind it to admission.
                    if sample.accepted_at.0 >= manifest.stream.budget.wall_time_ms * 1_000_000
                        || sample.accepted_at.0.checked_sub(sample.capture_tick.0)
                            != Some(sample.capture_to_analysis_ns)
                    {
                        return Err(ProtocolError::invalid(
                            "captured recording sample lies outside original lease or age basis",
                        ));
                    }
                    (attempt_id, identity, profile_sha256, input)
                }
                LiveInferenceReply::CudaProvisional {
                    attempt_id,
                    identity,
                    profile_sha256,
                    input,
                    ..
                } if self.profile.provider == InferenceProvider::Cuda
                    && self.cpu_manifest.is_none() =>
                {
                    (attempt_id, identity, profile_sha256, input)
                }
                _ => {
                    return Err(ProtocolError::invalid(
                        "recording estimate kind/provider differs",
                    ))
                }
            };
            input.previous.validate()?;
            input.current.validate()?;
            input.pair.elapsed()?;
            if let Some(binding) = &self.browser_media {
                for time in [input.pair.previous_time, input.pair.current_time] {
                    binding
                        .clock
                        .project_at(time, &binding.selection.placement.spec)
                        .map_err(|e| ProtocolError::invalid(e.to_string()))?;
                }
            }
            if sample.qualified
                || sample.evidence != EvidenceKind::Inferred
                || !sample.fresh
                || sample.capture_age_ns != sample.capture_to_analysis_ns
                || attempt != &self.inference_attempt
                || identity != &self.identity
                || hash != &profile_sha
                || input.sequence <= previous_sequence
                || input.pair.source_epoch != self.identity.epoch
                || input.pair.presentation_clock.as_ref() != Some(&self.clock)
                || input.previous.byte_len != LIVE_FRAME_BYTES as u64
                || input.current.byte_len != LIVE_FRAME_BYTES as u64
            {
                return Err(ProtocolError::invalid(
                    "recording estimate authority/clock differs",
                ));
            }
            previous_sequence = input.sequence;
        }
        Ok(())
    }
}
#[cfg(test)]
mod recording_tests {
    use super::*;
    fn page() -> LiveRecordingPage {
        let capture: LiveCaptureReport = serde_json::from_str(include_str!(
            "../tests/fixtures/live-cpu-capture-report.json"
        ))
        .unwrap();
        let mut sample = capture.inference.as_ref().unwrap().latest.clone().unwrap();
        sample.capture_age_ns = sample.capture_to_analysis_ns;
        LiveRecordingPage {
            cpu_manifest: None,
            browser_media: None,
            model_pack: None,
            source_time_basis: LiveSourceTimeBasis::ProducerDeclared,
            schema: 1,
            engine_epoch: capture.engine_epoch,
            identity: capture.identity,
            clock: capture.clock,
            profile: capture.options.inference.unwrap().profile,
            inference_attempt: capture.inference.unwrap().attempt_id,
            recording_request_id: RequestId::new("recording-intent").unwrap(),
            maximum_samples: 2,
            total_samples: 1,
            discarded: false,
            exhausted: false,
            active: true,
            retirement: None,
            after: 0,
            next: None,
            samples: vec![sample],
            qualified: false,
        }
    }
    #[test]
    fn recording_pages_refuse_promoted_foreign_and_cursor_inconsistent_estimates() {
        let good = page();
        good.validate().unwrap();
        let mut bad = good.clone();
        bad.qualified = true;
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.samples[0].evidence = EvidenceKind::Observed;
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.identity.epoch = AttemptId::new("foreign").unwrap();
        assert!(bad.validate().is_err());
        let mut bad = good.clone();
        bad.next = Some(1);
        assert!(bad.validate().is_err());
        let mut bad = good;
        bad.samples.push(bad.samples[0].clone());
        bad.total_samples = 2;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn discarded_recording_retains_historical_count_without_rearm_or_motion_payload() {
        let mut page = page();
        page.discarded = true;
        page.samples.clear();
        page.validate().unwrap();
        assert_eq!(page.total_samples, 1);
        page.after = 1;
        assert!(page.validate().is_err());
        assert!(Command::StartLiveRecording {
            epoch: AttemptId::new("epoch").unwrap(),
            maximum_samples: 2
        }
        .requires_revision());
        assert!(!Command::LiveRecordingPage {
            epoch: AttemptId::new("epoch").unwrap(),
            after: 0,
            limit: 1
        }
        .is_mutating());
    }
    #[test]
    fn captured_recording_page_preserves_full_manifest_and_legacy_omission() {
        let host = page();
        let raw = serde_json::to_value(&host).unwrap();
        assert!(raw.get("cpu_manifest").is_none());
        let decoded: LiveRecordingPage = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), raw);
        let mut captured = host;
        super::captured_cpu_report_tests::bind_page(&mut captured);
        let raw = serde_json::to_value(&captured).unwrap();
        assert_eq!(
            raw["cpu_manifest"],
            serde_json::to_value(captured.cpu_manifest.as_ref().unwrap()).unwrap()
        );
        let decoded: LiveRecordingPage = serde_json::from_value(raw).unwrap();
        decoded.validate().unwrap();
    }
    #[test]
    fn recorded_samples_refuse_host_captured_and_cuda_mode_confusion() {
        let host = page();
        let mut captured = host.clone();
        super::captured_cpu_report_tests::bind_page(&mut captured);
        let mut wrong = captured.clone();
        wrong.samples = host.samples;
        assert!(wrong.validate().is_err());
        let mut wrong = captured.clone();
        wrong.cpu_manifest = None;
        assert!(wrong.validate().is_err());
        let mut wrong = captured;
        wrong.profile.provider = InferenceProvider::Cuda;
        assert!(wrong.validate().is_err());
    }
    #[test]
    fn recorded_captured_manifest_and_sample_are_bound_to_scope_and_original_budget() {
        let mut good = page();
        super::captured_cpu_report_tests::bind_page(&mut good);
        for case in 0..6 {
            let mut wrong = good.clone();
            match case {
                0 => wrong.inference_attempt = AttemptId::new("different-attempt").unwrap(),
                1 => wrong.clock.added_buffer_ns += 1,
                2 => wrong.identity.source = SourceVersionId::new("other-source").unwrap(),
                3 => wrong.profile.minimum_memory_bytes += 1,
                4 => wrong.samples[0].accepted_at = LiveClockTick(30_000_000_000),
                _ => {
                    let LiveInferenceReply::CapturedCpuEstimated {
                        manifest_sha256, ..
                    } = &mut wrong.samples[0].result
                    else {
                        panic!("fixture");
                    };
                    *manifest_sha256 = "0".repeat(64);
                }
            }
            assert!(wrong.validate().is_err(), "recorded scope {case}");
        }
    }
}

#[cfg(test)]
pub(crate) mod captured_cpu_report_tests {
    use super::*;
    fn artifact(n: u8) -> ArtifactIdentity {
        let sha256 = format!("{n:064x}");
        ArtifactIdentity {
            artifact_id: ArtifactId::new(sha256.clone()).unwrap(),
            sha256,
            byte_len: 10,
        }
    }
    /// Authored metadata fixture only: no measured equivalence or native proof.
    pub(crate) fn manifest(
        attempt: &AttemptId,
        identity: &LiveIdentity,
        clock: &LivePresentationClock,
        profile: &DeclaredInferenceProfile,
        lease_ms: u64,
    ) -> LiveCpuInferenceManifest {
        let mut libraries: Vec<_> = CPU_RUNTIME_REQUIRED_ALIASES
            .iter()
            .enumerate()
            .map(|(i, name)| CapturedNativeLibrary {
                identity: if *name == "libonnxruntime.so.1" {
                    profile.runtime.clone()
                } else {
                    artifact(i as u8 + 20)
                },
                names: vec![name.to_string()],
            })
            .collect();
        libraries.sort_by(|a, b| a.identity.sha256.cmp(&b.identity.sha256));
        LiveCpuInferenceManifest::new(
            LiveInferenceManifest {
                version: PROTOCOL_VERSION,
                attempt_id: attempt.clone(),
                identity: identity.clone(),
                clock: clock.clone(),
                model: ArtifactReference {
                    path: LIVE_CPU_MODEL_PATH.into(),
                    identity: profile.model.clone(),
                },
                runtime: ArtifactReference {
                    path: LIVE_CPU_RUNTIME_PATH.into(),
                    identity: profile.runtime.clone(),
                },
                worker: artifact(251),
                profile: profile.clone(),
                budget: ResourceBudget {
                    memory_bytes: profile.minimum_memory_bytes.max(256 << 20),
                    output_bytes: profile.minimum_output_bytes.max(4096),
                    wall_time_ms: lease_ms,
                    cpu_threads: profile.cpu_threads,
                },
            },
            CpuRuntimeImage {
                schema_version: 1,
                owner_policy: artifact(250),
                libraries,
            },
        )
        .unwrap()
    }
    pub(crate) fn bind_result(
        result: &mut LiveInferenceReply,
        manifest: &LiveCpuInferenceManifest,
    ) {
        let (input, position, pair_analysis_elapsed_ns) = match result.clone() {
            LiveInferenceReply::Estimated {
                input,
                position,
                pair_analysis_elapsed_ns,
                ..
            }
            | LiveInferenceReply::CapturedCpuEstimated {
                input,
                position,
                pair_analysis_elapsed_ns,
                ..
            } => (input, position, pair_analysis_elapsed_ns),
            _ => panic!("authored CPU fixture"),
        };
        let binding = manifest.reply_binding().unwrap();
        *result = LiveInferenceReply::CapturedCpuEstimated {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: binding.profile_sha256().into(),
            manifest_sha256: binding.manifest_sha256().into(),
            input,
            position,
            pair_analysis_elapsed_ns,
        };
    }
    pub(crate) fn capture() -> LiveCaptureReport {
        let mut capture: LiveCaptureReport = serde_json::from_str(include_str!(
            "../tests/fixtures/live-cpu-capture-report.json"
        ))
        .unwrap();
        let report = capture.inference.as_mut().unwrap();
        let manifest = manifest(
            &report.attempt_id,
            &capture.identity,
            &capture.clock,
            &capture.options.inference.as_ref().unwrap().profile,
            u64::from(capture.options.lease_ms),
        );
        bind_result(&mut report.latest.as_mut().unwrap().result, &manifest);
        report.cpu_manifest = Some(Box::new(manifest));
        capture.validate().unwrap();
        capture
    }
    pub(crate) fn bind_page(page: &mut LiveRecordingPage) {
        let manifest = manifest(
            &page.inference_attempt,
            &page.identity,
            &page.clock,
            &page.profile,
            30_000,
        );
        for sample in &mut page.samples {
            bind_result(&mut sample.result, &manifest);
        }
        page.cpu_manifest = Some(Box::new(manifest));
        page.validate().unwrap();
    }
    #[test]
    fn captured_declaration_is_present_before_ready_without_claiming_a_runtime_receipt() {
        let mut capture = self::capture();
        let report = capture.inference.as_mut().unwrap();
        report.state = LiveCaptureInferenceState::Starting;
        report.completed = 0;
        report.accepted = 0;
        report.stale = 0;
        report.latest = None;
        capture.validate().unwrap();
        let wire = serde_json::to_value(&capture).unwrap();
        assert_eq!(wire["inference"]["state"], "starting");
        assert!(wire["inference"].get("cpu_manifest").is_some());
        capture.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Running;
        capture.validate().unwrap(); // Readiness is only the private Engine's observation.
        capture.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Starting;
        capture.inference.as_mut().unwrap().completed = 1;
        capture.inference.as_mut().unwrap().stale = 1;
        assert!(capture.validate().is_err());
        let mut capture = self::capture();
        capture.inference.as_mut().unwrap().state = LiveCaptureInferenceState::Starting;
        assert!(capture.validate().is_err());
    }
    #[test]
    fn captured_reports_refuse_host_cuda_and_unbound_captured_estimates() {
        let host: LiveCaptureReport = serde_json::from_str(include_str!(
            "../tests/fixtures/live-cpu-capture-report.json"
        ))
        .unwrap();
        let mut declared = capture();
        declared.inference.as_mut().unwrap().latest =
            host.inference.as_ref().unwrap().latest.clone();
        assert!(declared.validate().is_err());
        let mut unbound = capture();
        unbound.inference.as_mut().unwrap().cpu_manifest = None;
        assert!(unbound.validate().is_err());
        let mut ready_as_estimate = capture();
        let report = ready_as_estimate.inference.as_mut().unwrap();
        let manifest = report.cpu_manifest.as_ref().unwrap();
        let binding = manifest.reply_binding().unwrap();
        report.latest.as_mut().unwrap().result = LiveInferenceReply::CapturedCpuReady {
            attempt_id: manifest.stream.attempt_id.clone(),
            identity: manifest.stream.identity.clone(),
            profile_sha256: binding.profile_sha256().into(),
            manifest_sha256: binding.manifest_sha256().into(),
        };
        assert!(ready_as_estimate.validate().is_err());
        let mut mixed = capture();
        mixed.inference.as_mut().unwrap().cuda_manifest_sha256 = Some("a".repeat(64));
        assert!(mixed.validate().is_err());
        let mut cuda = capture();
        cuda.options.inference.as_mut().unwrap().profile.provider = InferenceProvider::Cuda;
        assert!(cuda.validate().is_err());
    }
    #[test]
    fn capture_declared_scope_profile_clock_attempt_and_lease_are_exact() {
        for case in 0..7 {
            let mut capture = self::capture();
            let manifest = capture
                .inference
                .as_mut()
                .unwrap()
                .cpu_manifest
                .as_mut()
                .unwrap();
            match case {
                0 => manifest.stream.attempt_id = AttemptId::new("another-attempt").unwrap(),
                1 => {
                    manifest.stream.identity.source =
                        SourceVersionId::new("another-source").unwrap()
                }
                2 => manifest.stream.identity.epoch = AttemptId::new("another-epoch").unwrap(),
                3 => manifest.stream.clock.added_buffer_ns += 1,
                4 => manifest.stream.profile.minimum_memory_bytes += 1,
                5 => manifest.stream.budget.wall_time_ms -= 1,
                _ => {
                    manifest.dependencies.pop();
                }
            }
            assert!(capture.validate().is_err(), "captured scope {case}");
        }
    }
    #[test]
    fn captured_estimates_bind_original_manifest_image_and_input_checks() {
        for case in 0..5 {
            let mut capture = self::capture();
            if case == 0 {
                let report = capture.inference.as_mut().unwrap();
                let original = report.cpu_manifest.as_ref().unwrap();
                let mut image = original.image.clone();
                image.owner_policy = artifact(249);
                report.cpu_manifest = Some(Box::new(
                    LiveCpuInferenceManifest::new(original.stream.clone(), image).unwrap(),
                ));
            } else {
                let LiveInferenceReply::CapturedCpuEstimated {
                    manifest_sha256,
                    input,
                    pair_analysis_elapsed_ns,
                    ..
                } = &mut capture
                    .inference
                    .as_mut()
                    .unwrap()
                    .latest
                    .as_mut()
                    .unwrap()
                    .result
                else {
                    panic!("captured fixture");
                };
                match case {
                    1 => *manifest_sha256 = "0".repeat(64),
                    2 => input.sequence = 0,
                    3 => input.previous.artifact_id = ArtifactId::new("different-pixels").unwrap(),
                    _ => *pair_analysis_elapsed_ns = 30_000_000_001,
                }
            }
            assert!(capture.validate().is_err(), "captured result {case}");
        }
    }
    #[test]
    fn legacy_host_report_omits_the_field_and_retains_its_wire_value() {
        let raw = include_str!("../tests/fixtures/live-cpu-capture-report.json");
        let capture: LiveCaptureReport = serde_json::from_str(raw).unwrap();
        capture.validate().unwrap();
        assert!(capture.inference.as_ref().unwrap().cpu_manifest.is_none());
        let before = serde_json::to_value(&capture).unwrap();
        assert!(before["inference"].get("cpu_manifest").is_none());
        let decoded: LiveCaptureReport = serde_json::from_value(before.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), before);
    }
    #[test]
    fn retired_captured_mode_remains_declared_history_without_execution_permission() {
        let mut capture = self::capture();
        capture.active = false;
        capture.retirement = Some(LiveCaptureRetirement::EngineRestarted);
        capture.admitted_pixel_bytes = 0;
        let report = capture.inference.as_mut().unwrap();
        report.state = LiveCaptureInferenceState::Retired;
        report.latest = None;
        capture.validate().unwrap();
        assert!(capture.inference.unwrap().cpu_manifest.is_some());
    }
}
