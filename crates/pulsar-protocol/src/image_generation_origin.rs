//! Retained image approvals are historical evidence, never execution authority.
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageGenerationApprovalRecord {
    pub schema_version: u16,
    pub approval_request: RequestId,
    pub approval: AssistantCreativeApproval,
    pub report: AssistantReport,
    pub configuration_sha256: String,
    pub observed_temporal_motion: bool,
    pub qualified: bool,
}
impl ImageGenerationApprovalRecord {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.report.validate()?;
        self.approval.validate()?;
        let expected = AssistantCreativeApproval::from_report(&self.report)?;
        let Some(AssistantProposal::SynthesizePatterns { patterns }) = &self.report.proposal else {
            return Err(ProtocolError::invalid(
                "image origin lacks synthetic proposal",
            ));
        };
        let configuration = serde_json::to_vec(&GenerationInput::Patterns {
            patterns: patterns.clone(),
        })
        .map_err(|_| ProtocolError::invalid("image configuration encoding"))?;
        if self.schema_version != 1
            || self.report.image.is_none()
            || self.observed_temporal_motion
            || self.qualified
            || self.approval.attempt_id != expected.attempt_id
            || self.approval.context_sha256 != expected.context_sha256
            || self.approval.proposal_sha256 != expected.proposal_sha256
            || self.configuration_sha256 != format!("{:x}", Sha256::digest(configuration))
        {
            return Err(ProtocolError::invalid(
                "image approval correspondence or qualification differs",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageGenerationOrigin {
    pub origin_manifest: Option<PackageObjectRef>,
    pub candidate_id: CandidateId,
    pub job_id: JobId,
    pub attempt_id: AttemptId,
    pub worker_manifest: PackageObjectRef,
    pub worker_receipt: ArtifactIdentity,
    pub worker_program: ArtifactIdentity,
    pub program: ArtifactIdentity,
    pub record: ArtifactIdentity,
    pub approval: ImageGenerationApprovalRecord,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateImageOriginReport {
    pub project_id: ProjectId,
    pub candidate_id: CandidateId,
    pub base_revision: RevisionId,
    pub motion: MotionDescriptor,
    pub origins: Vec<ImageGenerationOrigin>,
    pub observed_temporal_motion: bool,
    pub execution_authority: bool,
    pub qualified: bool,
}
impl CandidateImageOriginReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.motion.validate()?;
        if self.motion.binding
            != (MotionBinding::Candidate {
                project_id: self.project_id.clone(),
                candidate_id: self.candidate_id.clone(),
                base_revision: self.base_revision,
            })
            || self.observed_temporal_motion
            || self.execution_authority
            || self.qualified
            || self.origins.is_empty()
            || self.origins.len() > 16
        {
            return Err(ProtocolError::invalid(
                "image inspection scope or authority differs",
            ));
        }
        let mut previous = None;
        for origin in &self.origins {
            if let Some(namespace) = &origin.origin_manifest {
                namespace.validate()?;
            }
            origin.worker_manifest.validate()?;
            origin.worker_receipt.validate()?;
            origin.worker_program.validate()?;
            origin.program.validate()?;
            origin.record.validate()?;
            origin.approval.validate()?;
            let bytes = serde_json::to_vec(&origin.approval)
                .map_err(|_| ProtocolError::invalid("image approval encoding"))?;
            let key = (
                origin
                    .origin_manifest
                    .as_ref()
                    .map(|m| m.sha256.as_str())
                    .unwrap_or(""),
                origin.candidate_id.as_str(),
            );
            if previous.is_some_and(|old| old >= key)
                || origin.record.byte_len > 128 * 1024
                || origin.record.artifact_id.as_str()
                    != format!("assistant-image-origin-v1:{}", origin.record.sha256)
                || origin.record.sha256 != format!("{:x}", Sha256::digest(&bytes))
                || origin.record.byte_len != bytes.len() as u64
                || origin.program.sha256 != self.motion.program.sha256
                || origin.program.byte_len != self.motion.program.byte_len
            {
                return Err(ProtocolError::invalid(
                    "image inspection record, motion or ordering differs",
                ));
            }
            previous = Some(key);
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("image inspection encoding"))?;
        if bytes.len() > MAX_CONTROL_BYTES - 4096 {
            return Err(ProtocolError::invalid(
                "image inspection exceeds control frame",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_origin_query_is_scoped_read_only_and_never_requires_fresh_generation_revision() {
        let command = Command::CandidateImageOriginStatus {
            candidate_id: CandidateId::new("candidate").unwrap(),
        };
        assert!(command.requires_project());
        assert!(!command.requires_revision());
        assert!(!command.is_mutating());
        assert_eq!(format!("{command:?}"), "CandidateImageOriginStatus");
        let mut frame = vec![];
        write_message(&mut frame, &command).unwrap();
        let decoded: Command = read_message(&mut std::io::Cursor::new(frame)).unwrap();
        assert!(
            matches!(decoded,Command::CandidateImageOriginStatus {candidate_id} if candidate_id.as_str()=="candidate")
        );
    }
}

/// The existing local raw-frame ABI, with explicit retained approval binding.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateImageOriginPixels {
    pub report: CandidateImageOriginReport,
    pub record: ArtifactIdentity,
    pub frame: FrameArtifact,
}
impl CandidateImageOriginPixels {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.report.validate()?;
        self.frame.validate(512 * 512 * 3)?;
        let origin = self
            .report
            .origins
            .iter()
            .find(|o| o.record == self.record)
            .ok_or_else(|| ProtocolError::invalid("pixel frame lacks selected origin"))?;
        let image = origin
            .approval
            .report
            .image
            .as_ref()
            .ok_or_else(|| ProtocolError::invalid("pixel frame lacks image context"))?;
        if self.frame.artifact.identity != image.rgb
            || self.frame.width != image.width
            || self.frame.height != image.height
            || self.frame.stride_bytes != u64::from(image.width) * 3
            || self.frame.format != PixelFormat::Rgb8
        {
            return Err(ProtocolError::invalid(
                "pixel frame differs from retained image context",
            ));
        }
        let bytes =
            serde_json::to_vec(self).map_err(|_| ProtocolError::invalid("pixel frame encoding"))?;
        if bytes.len() > MAX_CONTROL_BYTES - 4096 {
            return Err(ProtocolError::invalid(
                "pixel metadata exceeds control frame",
            ));
        }
        Ok(())
    }
}
