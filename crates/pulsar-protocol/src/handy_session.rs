//! Boot-local credential-route ownership. Observation is not playback qualification.
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyConnectionState {
    Opening,
    Observed,
    Retired,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyConnectionSession {
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub open_request_id: RequestId,
    pub registration: HandyRegistration,
    pub state: HandyConnectionState,
    pub inspection: Option<HandyInspection>,
}
impl HandyConnectionSession {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.registration.validate()?;
        if self.registration.disabled || self.engine_epoch == self.device_session {
            return Err(ProtocolError::invalid("invalid Handy connection binding"));
        }
        match (&self.state, &self.inspection) {
            (HandyConnectionState::Observed, Some(report)) if report.connected => report.validate(),
            (HandyConnectionState::Opening | HandyConnectionState::Retired, None) => Ok(()),
            _ => Err(ProtocolError::invalid(
                "inconsistent Handy connection observation",
            )),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyConnectionCatalog {
    pub engine_epoch: DeviceSessionId,
    pub sessions: Vec<HandyConnectionSession>,
}
impl HandyConnectionCatalog {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let mut ids = std::collections::HashSet::new();
        if self.sessions.len() > HANDY_REGISTRATION_LIMIT {
            return Err(ProtocolError::invalid("Handy session limit exceeded"));
        }
        for s in &self.sessions {
            s.validate()?;
            if s.engine_epoch != self.engine_epoch || !ids.insert(&s.device_session) {
                return Err(ProtocolError::invalid(
                    "Handy session catalog binding mismatch",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_session_contract_does_not_promote_observation_or_accept_mixed_epochs() {
        let mut s = HandyConnectionSession {
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
            device_session: DeviceSessionId::new("session").unwrap(),
            open_request_id: RequestId::new("open").unwrap(),
            registration: HandyRegistration {
                device_id: DeviceId::new("device").unwrap(),
                revision: 1,
                settings: HandyRegistrationSettings {
                    label: "fixture".into(),
                    api_key_env: "KEY_A".into(),
                    connection_key_env: "KEY_B".into(),
                },
                disabled: false,
                binding_sha256: "a".repeat(64),
            },
            state: HandyConnectionState::Opening,
            inspection: None,
        };
        s.validate().unwrap();
        s.state = HandyConnectionState::Observed;
        assert!(s.validate().is_err());
        s.inspection = Some(HandyInspection {
            connected: false,
            identity: None,
            observed_at_unix_ms: 1,
        });
        assert!(s.validate().is_err());
        s.state = HandyConnectionState::Retired;
        assert!(s.validate().is_err());
        s.inspection = None;
        s.validate().unwrap();
        let mut c = HandyConnectionCatalog {
            engine_epoch: s.engine_epoch.clone(),
            sessions: vec![s.clone()],
        };
        c.validate().unwrap();
        c.sessions.push(s);
        assert!(c.validate().is_err());
        c.sessions.pop();
        c.engine_epoch = DeviceSessionId::new("other-boot").unwrap();
        assert!(c.validate().is_err());
    }
}
