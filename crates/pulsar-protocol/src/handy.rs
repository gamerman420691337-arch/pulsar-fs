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
    /// Stable physical-device identity.
    ///
    /// `session_id` is a per-connection token that changes on every reconnect, so
    /// it is excluded from matching: binding it would invalidate a qualification
    /// (and any accepted playback) as soon as the device reconnects. The physical
    /// device is pinned by the registration binding and connection key, not by
    /// this token. `session_id` is still retained on the record for provenance.
    pub fn same_device(&self, other: &Self) -> bool {
        self.firmware == other.firmware
            && self.model_number == other.model_number
            && self.model_name == other.model_name
            && self.model_variant == other.model_variant
            && self.firmware_status == other.firmware_status
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        // Firmware-reported text may contain interior spaces (e.g. "Handy 2 Pro").
        // Reject control characters and surrounding whitespace, but keep the value
        // as reported: it is part of retained identity and hash bindings.
        let text = |s: &str, max| {
            !s.is_empty()
                && s.len() <= max
                && s.trim() == s
                && !s
                    .chars()
                    .any(|c| c.is_control() || !(c.is_ascii_graphic() || c == ' '))
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

#[cfg(test)]
mod identity_tests {
    use super::*;

    fn identity(model_name: &str) -> HandyIdentity {
        HandyIdentity {
            firmware: "4.0.0".into(),
            model_number: 2,
            model_name: model_name.into(),
            model_variant: 1,
            session_id: "abcd-1234".into(),
            firmware_status: 0,
        }
    }

    #[test]
    fn firmware_reported_spaced_model_name_is_accepted_unchanged() {
        let reported = "Handy 2 Pro";
        let value = identity(reported);
        value.validate().unwrap();
        assert_eq!(value.model_name, reported);
    }

    #[test]
    fn reconnect_session_change_is_not_a_different_device() {
        // The device issues a new session_id on every connection. Qualification
        // and accepted playback must survive that, or they break on every
        // reconnect. The physical device is pinned by the registration binding.
        let mut first = identity("H02_pro");
        first.session_id = "01M67Q3CHBKC6STN99PNMX46JM".into();
        let mut second = identity("H02_pro");
        second.session_id = "01M67X5RGT0AKVQBZMDN3TPQXY".into();
        assert!(first.same_device(&second));
        assert_ne!(first, second, "records still retain the observed session");

        // Genuine hardware differences are still refused.
        let mut other = second.clone();
        other.firmware = "4.3.1+dcee7e58".into();
        assert!(!first.same_device(&other));
        let mut other = second.clone();
        other.model_variant = 2;
        assert!(!first.same_device(&other));
        let mut other = second.clone();
        other.model_name = "H02".into();
        assert!(!first.same_device(&other));
        let mut other = second.clone();
        other.firmware_status = 1;
        assert!(!first.same_device(&other));
    }

    #[test]
    fn identity_text_still_refuses_control_and_surrounding_whitespace() {
        for bad in [
            "Handy\n2",
            "Handy\t2",
            " Handy 2",
            "Handy 2 ",
            "Handy\u{7f}",
        ] {
            assert!(
                identity(bad).validate().is_err(),
                "accepted invalid identity text {bad:?}"
            );
        }
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
