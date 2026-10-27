//! Disclosure observations. No URL, credential, device setup or playback grant.
use crate::*;
pub const HANDY_HOSTED_SCRIPT_MAX_BYTES: u64 = 8 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHostingOutcome {
    Unknown,
    Verified,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHostingReport {
    pub request_id: RequestId,
    pub preparation_request_id: RequestId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub engine_epoch: DeviceSessionId,
    pub master_sha256: String,
    pub script: ArtifactIdentity,
    pub outcome: HandyHostingOutcome,
    pub downloaded: Option<ArtifactIdentity>,
    pub retired: bool,
    pub link_available: bool,
    pub upload_in_flight: bool,
}
impl HandyHostingReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(&self.master_sha256)?;
        self.script.validate()?;
        if self.request_id == self.preparation_request_id
            || self.script.byte_len == 0
            || self.script.byte_len > HANDY_STROKE_SCRIPT_MAX_BYTES
            || self.script.artifact_id.as_str() != self.script.sha256
        {
            return Err(ProtocolError::invalid("invalid Handy hosting subject"));
        }
        match (&self.outcome, &self.downloaded) {
            (HandyHostingOutcome::Unknown, None) if !self.link_available => {}
            (HandyHostingOutcome::Verified, Some(value)) if !self.upload_in_flight => {
                value.validate()?;
                if value.byte_len == 0
                    || value.byte_len > HANDY_HOSTED_SCRIPT_MAX_BYTES
                    || value.artifact_id.as_str() != value.sha256
                {
                    return Err(ProtocolError::invalid(
                        "invalid hosted download observation",
                    ));
                }
            }
            _ => return Err(ProtocolError::invalid("inconsistent Handy hosting outcome")),
        }
        if self.link_available && self.retired {
            return Err(ProtocolError::invalid(
                "retired hosting cannot retain an available link",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_hosting_requires_explicit_consent_project_revision_and_separate_identity() {
        let mut r = Request::new(
            RequestId::new("upload").unwrap(),
            Command::HostHandyStrokeScript {
                preparation_request_id: RequestId::new("prepare").unwrap(),
                script_sha256: "a".repeat(64),
                engine_epoch: DeviceSessionId::new("boot").unwrap(),
                allow_temporary_public_hosting: true,
                owner_proof: "b".repeat(64),
            },
        );
        assert!(r.validate().is_err());
        r.project = Some(ProjectId::new("project").unwrap());
        assert!(r.validate().is_err());
        r.expected_revision = Some(RevisionId::new(1));
        r.validate().unwrap();
        if let Command::HostHandyStrokeScript {
            allow_temporary_public_hosting,
            ..
        } = &mut r.command
        {
            *allow_temporary_public_hosting = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
        if let Command::HostHandyStrokeScript {
            allow_temporary_public_hosting,
            preparation_request_id,
            ..
        } = &mut r.command
        {
            *allow_temporary_public_hosting = true;
            *preparation_request_id = r.request_id.clone();
        }
        assert!(r.validate().is_err());
    }
    #[test]
    fn handy_hosting_unknown_and_retired_observations_cannot_claim_usable_links() {
        let mut r = HandyHostingReport {
            request_id: RequestId::new("upload").unwrap(),
            preparation_request_id: RequestId::new("prepare").unwrap(),
            project_id: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(1),
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            master_sha256: "a".repeat(64),
            script: ArtifactIdentity {
                artifact_id: ArtifactId::new("b".repeat(64)).unwrap(),
                sha256: "b".repeat(64),
                byte_len: 40,
            },
            outcome: HandyHostingOutcome::Unknown,
            downloaded: None,
            retired: false,
            link_available: false,
            upload_in_flight: true,
        };
        r.validate().unwrap();
        r.retired = true;
        r.validate().unwrap();
        r.link_available = true;
        assert!(r.validate().is_err());
        r.link_available = false;
        r.outcome = HandyHostingOutcome::Verified;
        r.downloaded = Some(r.script.clone());
        assert!(r.validate().is_err());
        r.upload_in_flight = false;
        r.validate().unwrap();
    }
}
