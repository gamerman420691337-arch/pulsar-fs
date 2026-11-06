//! Explicit online inspection only. No credential values, endpoint selection,
//! motion submission, firmware mutation or device qualification is accepted.
use crate::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyInspectionRequest {
    pub api_key_env: String,
    pub connection_key_env: String,
    pub allow_online_inspection: bool,
}
impl HandyInspectionRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !self.allow_online_inspection {
            return Err(ProtocolError::new(
                ErrorCode::Forbidden,
                "Handy online inspection requires explicit consent",
            ));
        }
        validate_credential_names(&self.api_key_env, &self.connection_key_env)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HandyIdentity {
    pub firmware: String,
    pub model_number: u32,
    pub model_name: String,
    pub model_variant: u32,
    pub session_id: String,
    pub firmware_status: u8,
}
impl HandyIdentity {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let text = |s: &str, max| {
            !s.is_empty()
                && s.len() <= max
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        };
        if !text(&self.firmware, 64)
            || !text(&self.model_name, 64)
            || !text(&self.session_id, 128)
            || self.firmware_status > 2
        {
            return Err(ProtocolError::invalid("invalid Handy identity observation"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyInspection {
    pub connected: bool,
    pub identity: Option<HandyIdentity>,
    pub observed_at_unix_ms: u64,
}
impl HandyInspection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.connected != self.identity.is_some() || self.observed_at_unix_ms == 0 {
            return Err(ProtocolError::invalid("inconsistent Handy inspection"));
        }
        if let Some(identity) = &self.identity {
            identity.validate()?;
        }
        Ok(())
    }
}

pub(crate) fn validate_credential_names(
    api_key_env: &str,
    connection_key_env: &str,
) -> Result<(), ProtocolError> {
    let name = |s: &str| {
        !s.is_empty()
            && s.len() <= 128
            && s.as_bytes()[0].is_ascii_uppercase()
            && s.bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
    };
    if !name(api_key_env) || !name(connection_key_env) || api_key_env == connection_key_env {
        return Err(ProtocolError::invalid(
            "Handy credentials require distinct environment variable names",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn online_consent_and_environment_names_are_required() {
        let mut r = HandyInspectionRequest {
            api_key_env: "HANDY_API_KEY".into(),
            connection_key_env: "HANDY_CONNECTION_KEY".into(),
            allow_online_inspection: false,
        };
        assert_eq!(r.validate().unwrap_err().code, ErrorCode::Forbidden);
        r.allow_online_inspection = true;
        assert!(r.validate().is_ok());
        for name in ["", "secret-value", "1KEY", "KEY\nVALUE", "KEY=value"] {
            r.api_key_env = name.into();
            assert!(r.validate().is_err());
        }
        r.api_key_env = r.connection_key_env.clone();
        assert!(r.validate().is_err());
    }
    #[test]
    fn observation_cannot_claim_connected_without_identity() {
        let r = HandyInspection {
            connected: true,
            identity: None,
            observed_at_unix_ms: 1,
        };
        assert!(r.validate().is_err());
        assert!(HandyInspection {
            connected: false,
            ..r
        }
        .validate()
        .is_ok());
    }
}
