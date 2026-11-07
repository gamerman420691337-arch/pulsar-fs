//! Owner configuration only. Registration supplies no device or qualification authority.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const HANDY_REGISTRATION_LIMIT: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    content = "arguments",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HandyRegistrationIntent {
    Save {
        device_id: DeviceId,
        expected_revision: u64,
        settings: HandyRegistrationSettings,
        disabled: bool,
    },
    Disable {
        device_id: DeviceId,
        expected_revision: u64,
    },
}
impl HandyRegistrationIntent {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Self::Save { settings, .. } = self {
            settings.validate()?;
        }
        if self.expected_revision() >= i64::MAX as u64 {
            return Err(ProtocolError::invalid(
                "Handy registration revision exceeds limit",
            ));
        }
        Ok(())
    }
    pub fn device(&self) -> &DeviceId {
        match self {
            Self::Save { device_id, .. } | Self::Disable { device_id, .. } => device_id,
        }
    }
    pub fn expected_revision(&self) -> u64 {
        match self {
            Self::Save {
                expected_revision, ..
            }
            | Self::Disable {
                expected_revision, ..
            } => *expected_revision,
        }
    }
    pub fn command(&self, owner_proof: String) -> Command {
        match self {
            Self::Save {
                device_id,
                expected_revision,
                settings,
                disabled,
            } => Command::SetHandyRegistration {
                device_id: device_id.clone(),
                expected_registration_revision: *expected_revision,
                settings: settings.clone(),
                disabled: *disabled,
                owner_proof,
            },
            Self::Disable {
                device_id,
                expected_revision,
            } => Command::DisableHandyRegistration {
                device_id: device_id.clone(),
                expected_registration_revision: *expected_revision,
                owner_proof,
            },
        }
    }
    pub fn matches(&self, receipt: &HandyRegistrationReceipt) -> bool {
        if &receipt.registration.device_id != self.device()
            || Some(receipt.registration.revision) != self.expected_revision().checked_add(1)
        {
            return false;
        }
        match self {
            Self::Save {
                settings, disabled, ..
            } => {
                receipt.registration.settings == *settings
                    && receipt.registration.disabled == *disabled
            }
            Self::Disable { .. } => receipt.registration.disabled,
        }
    }
}
impl Command {
    pub fn handy_registration_intent(&self) -> Option<HandyRegistrationIntent> {
        match self {
            Self::SetHandyRegistration {
                device_id,
                expected_registration_revision,
                settings,
                disabled,
                ..
            } => Some(HandyRegistrationIntent::Save {
                device_id: device_id.clone(),
                expected_revision: *expected_registration_revision,
                settings: settings.clone(),
                disabled: *disabled,
            }),
            Self::DisableHandyRegistration {
                device_id,
                expected_registration_revision,
                ..
            } => Some(HandyRegistrationIntent::Disable {
                device_id: device_id.clone(),
                expected_revision: *expected_registration_revision,
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HandyRegistrationOutcome {
    Applied(Box<HandyRegistrationReceipt>),
    Withdrawn,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRegistrationResolution {
    pub request_id: RequestId,
    pub intent: HandyRegistrationIntent,
    pub outcome: HandyRegistrationOutcome,
}
impl HandyRegistrationResolution {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.intent.validate()?;
        if let HandyRegistrationOutcome::Applied(receipt) = &self.outcome {
            receipt.validate()?;
            if receipt.request_id != self.request_id || !self.intent.matches(receipt) {
                return Err(ProtocolError::invalid("Handy resolution receipt mismatch"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRegistrationSettings {
    pub label: String,
    pub api_key_env: String,
    pub connection_key_env: String,
}
impl HandyRegistrationSettings {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.label.trim().is_empty()
            || self.label.len() > 128
            || self.label.chars().any(char::is_control)
        {
            return Err(ProtocolError::invalid(
                "Handy registration requires a bounded display label",
            ));
        }
        crate::handy::validate_credential_names(&self.api_key_env, &self.connection_key_env)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRegistration {
    pub device_id: DeviceId,
    pub revision: u64,
    pub settings: HandyRegistrationSettings,
    pub disabled: bool,
    pub binding_sha256: String,
}
impl HandyRegistration {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.settings.validate()?;
        if self.revision == 0 || self.revision > i64::MAX as u64 {
            return Err(ProtocolError::invalid(
                "invalid Handy registration revision",
            ));
        }
        validate_sha256(&self.binding_sha256)
    }
}

/// Historical acknowledgment; callers must query current configuration separately.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRegistrationReceipt {
    pub request_id: RequestId,
    pub registration: HandyRegistration,
}
impl HandyRegistrationReceipt {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.registration.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRegistrationCatalog {
    pub registrations: Vec<HandyRegistration>,
}
impl HandyRegistrationCatalog {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.registrations.len() > HANDY_REGISTRATION_LIMIT {
            return Err(ProtocolError::invalid(
                "Handy registration catalog exceeds limit",
            ));
        }
        let mut ids = HashSet::new();
        for record in &self.registrations {
            record.validate()?;
            if !ids.insert(&record.device_id) {
                return Err(ProtocolError::invalid("duplicate Handy registration"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRegisteredInspection {
    pub registration: HandyRegistration,
    pub inspection: HandyInspection,
}
impl HandyRegisteredInspection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.registration.validate()?;
        if self.registration.disabled {
            return Err(ProtocolError::invalid(
                "disabled registration cannot supply a fresh inspection",
            ));
        }
        self.inspection.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_is_bounded_configuration_not_a_qualification_claim() {
        let settings = HandyRegistrationSettings {
            label: "Handy 2 Pro".into(),
            api_key_env: "HANDY_API_KEY".into(),
            connection_key_env: "HANDY_CONNECTION_KEY".into(),
        };
        settings.validate().unwrap();
        let record = HandyRegistration {
            device_id: DeviceId::new("device").unwrap(),
            revision: 1,
            settings,
            disabled: false,
            binding_sha256: "a".repeat(64),
        };
        record.validate().unwrap();
        let mut value = serde_json::to_value(&record).unwrap();
        value["qualified"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HandyRegistration>(value).is_err());
        let mut bad = record.clone();
        bad.settings.connection_key_env = bad.settings.api_key_env.clone();
        assert!(bad.validate().is_err());
        bad = record.clone();
        bad.settings.label = "\n".into();
        assert!(bad.validate().is_err());
        bad = record.clone();
        bad.revision = 0;
        assert!(bad.validate().is_err());
        assert!(HandyRegistrationCatalog {
            registrations: vec![record.clone(), record]
        }
        .validate()
        .is_err());
    }
    #[test]
    fn commands_are_owner_scoped_and_do_not_borrow_project_revision() {
        let command = Command::SetHandyRegistration {
            device_id: DeviceId::new("device").unwrap(),
            expected_registration_revision: 0,
            settings: HandyRegistrationSettings {
                label: "Handy".into(),
                api_key_env: "API_KEY".into(),
                connection_key_env: "CONNECTION_KEY".into(),
            },
            disabled: false,
            owner_proof: "a".repeat(64),
        };
        assert!(command.is_mutating());
        assert!(!command.requires_project());
        assert!(!command.requires_revision());
        let mut request = Request::new(RequestId::new("request").unwrap(), command);
        request.validate().unwrap();
        request.project = Some(ProjectId::new("project").unwrap());
        assert!(request.validate().is_err());
        for command in [
            Command::ListHandyRegistrations {
                owner_proof: "a".repeat(64),
            },
            Command::InspectHandyRegistration {
                device_id: DeviceId::new("device").unwrap(),
                expected_registration_revision: 1,
                binding_sha256: "b".repeat(64),
                allow_online_inspection: true,
                owner_proof: "a".repeat(64),
            },
        ] {
            assert!(!command.is_mutating());
            assert!(!command.requires_project());
        }
    }

    #[test]
    fn resolution_binds_the_original_intent_and_distinct_request_id() {
        let intent = HandyRegistrationIntent::Save {
            device_id: DeviceId::new("device").unwrap(),
            expected_revision: 0,
            settings: HandyRegistrationSettings {
                label: "Handy".into(),
                api_key_env: "API_KEY".into(),
                connection_key_env: "CONNECTION_KEY".into(),
            },
            disabled: false,
        };
        let original = RequestId::new("original").unwrap();
        let mut request = Request::new(
            RequestId::new("resolution").unwrap(),
            Command::ResolveHandyRegistration {
                original_request_id: original.clone(),
                intent: intent.clone(),
                owner_proof: "a".repeat(64),
            },
        );
        assert!(request.command.is_mutating());
        assert!(!request.command.requires_project());
        request.validate().unwrap();
        request.request_id = original.clone();
        assert!(request.validate().is_err());
        let resolution = HandyRegistrationResolution {
            request_id: original,
            intent,
            outcome: HandyRegistrationOutcome::Withdrawn,
        };
        resolution.validate().unwrap();
        let mut malformed = resolution.clone();
        malformed.intent = HandyRegistrationIntent::Disable {
            device_id: DeviceId::new("device").unwrap(),
            expected_revision: u64::MAX,
        };
        assert!(malformed.validate().is_err());
    }
}
