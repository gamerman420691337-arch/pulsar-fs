//! Local Stroke CSV publication metadata, never device admission or hosting consent.
use crate::*;

pub const HANDY_STROKE_SCRIPT_MAX_BYTES: u64 = 4 * 1024 * 1024;
pub const HANDY_STROKE_SCRIPT_MAX_ACTIONS: u64 = 1_000_000;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyStrokeQuantization {
    pub action_count: u64,
    pub first_time_ms: u64,
    pub last_time_ms: u64,
    pub max_time_error_ns: u64,
    /// Conservative ceiling in millionths of normalized full travel.
    pub max_position_error_ppm: u32,
    pub omitted_axes: Vec<Axis>,
}
impl HandyStrokeQuantization {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let unique: std::collections::BTreeSet<_> = self.omitted_axes.iter().collect();
        if !(2..=HANDY_STROKE_SCRIPT_MAX_ACTIONS).contains(&self.action_count)
            || self.first_time_ms >= self.last_time_ms
            || self.last_time_ms > (1u64 << 53) - 1
            || self.action_count > self.last_time_ms - self.first_time_ms + 1
            || self.max_time_error_ns > 500_000
            || self.max_position_error_ppm > 5_001
            || self.omitted_axes.len() > 5
            || unique.len() != self.omitted_axes.len()
            || self.omitted_axes.contains(&Axis::Stroke)
        {
            return Err(ProtocolError::invalid(
                "invalid Handy Stroke quantization report",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyStrokeScript {
    pub request_id: RequestId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub path: std::path::PathBuf,
    pub master_sha256: String,
    pub identity: ArtifactIdentity,
    pub quantization: HandyStrokeQuantization,
    pub unresolved_review_count: u32,
    pub review_state_unknown: bool,
    pub release_qualified: bool,
}
impl HandyStrokeScript {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.master_sha256)?;
        self.identity.validate()?;
        self.quantization.validate()?;
        let path = self.path.as_os_str().as_encoded_bytes();
        if self.release_qualified
            || self.identity.artifact_id.as_str() != self.identity.sha256
            || self.identity.byte_len == 0
            || self.identity.byte_len > HANDY_STROKE_SCRIPT_MAX_BYTES
            || !self.path.is_absolute()
            || path.len() > 32768
            || path.contains(&0)
            || self
                .path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(ProtocolError::invalid(
                "invalid Handy Stroke script receipt",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_script_contract_requires_revision_and_rejects_inconsistent_quantization() {
        let mut request = Request::new(
            RequestId::new("prepare").unwrap(),
            Command::PrepareHandyStrokeScript {
                path: std::env::temp_dir().join("stroke.csv"),
            },
        );
        request.project = Some(ProjectId::new("project").unwrap());
        assert!(request.validate().is_err());
        request.expected_revision = Some(RevisionId::new(1));
        request.validate().unwrap();
        let mut q = HandyStrokeQuantization {
            action_count: 2,
            first_time_ms: 0,
            last_time_ms: 1,
            max_time_error_ns: 500_000,
            max_position_error_ppm: 5_001,
            omitted_axes: vec![Axis::Roll],
        };
        q.validate().unwrap();
        q.action_count = 3;
        assert!(q.validate().is_err());
        q.action_count = 2;
        q.omitted_axes.push(Axis::Roll);
        assert!(q.validate().is_err());
        q.omitted_axes.clear();
        q.max_time_error_ns = 500_001;
        assert!(q.validate().is_err());
    }
}
