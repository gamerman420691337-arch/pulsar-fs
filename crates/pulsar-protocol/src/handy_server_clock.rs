//! Explicit server-clock assumptions and historical observations, never playback grants.
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyServerClockModel {
    /// Timestamp is generated during the request, on the same nondecreasing
    /// server clock used by playback, without discontinuities during validity.
    RequestBracketMonotonic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyServerClockQualification {
    pub model: HandyServerClockModel,
    /// Symmetric bound including timestamp origin, quantization and backend error.
    /// Integer milliseconds or an HTTP acknowledgment do not establish this bound.
    pub timestamp_error_ns: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyServerClockObservation {
    pub request_id: RequestId,
    pub controller: HandyControllerLease,
    pub qualification: HandyQualificationBinding,
    pub clock_domain: ArtifactId,
    pub sent_boottime_ns: u64,
    pub received_boottime_ns: u64,
    pub estimated_server_ms: u64,
    pub uncertainty_ns: u64,
    /// Whether this exact native sample was usable at the latest Engine check.
    /// Deserializing this report cannot create native clock authority.
    pub current: bool,
    pub playback_enabled: bool,
}
impl HandyServerClockObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.controller.validate()?;
        self.qualification.validate()?;
        validate_sha256(self.clock_domain.as_str())?;
        if self.request_id == self.controller.acquisition_request_id
            || self.controller.state != HandyControllerState::Active
            || self.received_boottime_ns < self.sent_boottime_ns
            || self.received_boottime_ns >= self.controller.deadline_boottime_ns
            || self.estimated_server_ms > (1u64 << 53) - 1
            || self.playback_enabled
        {
            return Err(ProtocolError::invalid(
                "invalid Handy server-clock observation",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_server_clock_request_requires_revision_separate_identity_and_online_consent() {
        let mut r = Request::new(
            RequestId::new("sample").unwrap(),
            Command::SampleHandyServerClock {
                acquisition_request_id: RequestId::new("acquire").unwrap(),
                controller_id: ClientId::new("controller").unwrap(),
                expected_generation: 0,
                allow_online_sampling: true,
                owner_proof: "a".repeat(64),
            },
        );
        assert!(r.command.requires_revision() && r.command.is_mutating());
        assert!(r.validate().is_err());
        r.project = Some(ProjectId::new("project").unwrap());
        r.expected_revision = Some(RevisionId::new(1));
        r.validate().unwrap();
        if let Command::SampleHandyServerClock {
            allow_online_sampling,
            ..
        } = &mut r.command
        {
            *allow_online_sampling = false;
        }
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
    }
    #[test]
    fn handy_server_clock_observation_cannot_claim_playback_or_an_expired_acquisition() {
        let lease: HandyControllerReceipt =
            serde_json::from_str(include_str!("../tests/fixtures/handy-controller.json")).unwrap();
        let mut good = HandyServerClockObservation {
            request_id: RequestId::new("sample").unwrap(),
            controller: lease.lease,
            qualification: HandyQualificationBinding {
                dossier_id: ArtifactId::new("b".repeat(64)).unwrap(),
                revision: 2,
            },
            clock_domain: ArtifactId::new("c".repeat(64)).unwrap(),
            sent_boottime_ns: 1,
            received_boottime_ns: 2,
            estimated_server_ms: 1000,
            uncertainty_ns: 1_000_000,
            current: true,
            playback_enabled: false,
        };
        good.validate().unwrap();
        for case in 0..4 {
            let mut bad = good.clone();
            match case {
                0 => bad.playback_enabled = true,
                1 => bad.received_boottime_ns = 0,
                2 => bad.received_boottime_ns = bad.controller.deadline_boottime_ns,
                _ => bad.estimated_server_ms = 1u64 << 53,
            };
            assert!(bad.validate().is_err());
        }
        good.current = false;
        good.validate().unwrap();
    }
}
