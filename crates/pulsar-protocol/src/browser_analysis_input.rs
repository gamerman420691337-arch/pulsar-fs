//! Descriptive finite-input status. None of these records grants native source
//! or device authority, and closing input does not retire its original lease.
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCaptureInputPhase {
    Closing,
    Joining,
    Sealed,
    Aborted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCaptureInputFinalDisposition {
    NoFrames,
    Unpaired,
    Accepted,
    Stale,
    Expired,
    Failed,
    Retired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCaptureInputCloseReport {
    pub original_request_id: RequestId,
    pub final_frame: Option<LiveFrameHeader>,
    pub phase: LiveCaptureInputPhase,
    pub disposition: Option<LiveCaptureInputFinalDisposition>,
    pub worker_joined: bool,
    pub delivery_settled: bool,
}
impl LiveCaptureInputCloseReport {
    pub fn validate(&self, capture: &LiveCaptureReport) -> Result<(), ProtocolError> {
        if capture.options.browser_analysis.is_none()
            || capture.options.encoded_source.is_some()
            || capture
                .options
                .inference
                .as_ref()
                .is_none_or(|i| i.profile.provider != InferenceProvider::Cpu)
            || self.final_frame != capture.last_frame
            || self.final_frame.is_none() != (capture.received_frames == 0)
        {
            return Err(ProtocolError::invalid(
                "input closure differs from original browser CPU ingress",
            ));
        }
        if let Some(frame) = &self.final_frame {
            frame.validate()?;
        }
        match self.phase {
            LiveCaptureInputPhase::Closing | LiveCaptureInputPhase::Joining => {
                if !capture.active
                    || self.disposition.is_some()
                    || self.worker_joined
                    || (self.phase == LiveCaptureInputPhase::Joining && !self.delivery_settled)
                    || capture.inference.as_ref().is_none_or(|info| {
                        !matches!(
                            info.state,
                            LiveCaptureInferenceState::Starting
                                | LiveCaptureInferenceState::Running
                        )
                    })
                {
                    return Err(ProtocolError::invalid(
                        "pending input closure cannot claim completion",
                    ));
                }
            }
            LiveCaptureInputPhase::Sealed => {
                if !self.worker_joined
                    || !self.delivery_settled
                    || self.disposition.is_none()
                    || self.disposition == Some(LiveCaptureInputFinalDisposition::Retired)
                    || capture.queue.as_ref().is_some_and(|queue| {
                        queue.pending_frames != 0 || queue.in_flight_frames != 0
                    })
                    || (capture.active
                        && capture
                            .inference
                            .as_ref()
                            .is_none_or(|i| i.state != LiveCaptureInferenceState::InputSealed))
                {
                    return Err(ProtocolError::invalid(
                        "sealed input requires joined worker and settled delivery",
                    ));
                }
                if capture.active
                    && capture.inference.as_ref().is_none_or(|info| {
                        info.latest.is_some()
                            != (self.disposition
                                == Some(LiveCaptureInputFinalDisposition::Accepted))
                    })
                {
                    return Err(ProtocolError::invalid(
                        "sealed latest estimate must represent the accepted final input only",
                    ));
                }
            }
            LiveCaptureInputPhase::Aborted => {
                if capture.active
                    || !matches!(
                        self.disposition,
                        Some(
                            LiveCaptureInputFinalDisposition::Failed
                                | LiveCaptureInputFinalDisposition::Retired
                        )
                    )
                {
                    return Err(ProtocolError::invalid("aborted input cannot remain active"));
                }
            }
        }
        if self.disposition == Some(LiveCaptureInputFinalDisposition::NoFrames)
            && self.final_frame.is_some()
        {
            return Err(ProtocolError::invalid(
                "no-frame closure names an admitted frame",
            ));
        }
        if matches!(
            self.disposition,
            Some(
                LiveCaptureInputFinalDisposition::Accepted
                    | LiveCaptureInputFinalDisposition::Unpaired
                    | LiveCaptureInputFinalDisposition::Stale
                    | LiveCaptureInputFinalDisposition::Expired
            )
        ) && self.final_frame.is_none()
        {
            return Err(ProtocolError::invalid(
                "final-input disposition requires an original frame",
            ));
        }
        Ok(())
    }
}

pub(crate) fn validate_request(request: &Request) -> Result<(), ProtocolError> {
    if let Command::CloseBrowserAnalysisInput {
        final_frame: Some(frame),
        ..
    } = &request.command
    {
        frame.validate()?;
    }
    Ok(())
}
