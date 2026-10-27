//! Native credential snapshot. No Debug/Serialize implementation or persisted unkeyed credential digest.
use pulsar_protocol::*;
fn unavailable(message: &str) -> ProtocolError {
    ProtocolError::new(ErrorCode::Unavailable, message)
}

pub(crate) struct Credentials {
    pub(crate) api: String,
    pub(crate) connection: String,
}
impl Credentials {
    pub(crate) fn from_environment(
        request: &HandyInspectionRequest,
    ) -> Result<Self, ProtocolError> {
        request.validate()?;
        let load = |name: &str| {
            std::env::var(name).map_err(|_| unavailable("Handy credential is absent from the engine environment; launch through secrets run"))
        };
        let credentials = Self {
            api: load(&request.api_key_env)?,
            connection: load(&request.connection_key_env)?,
        };
        credentials.validate()?;
        Ok(credentials)
    }
    pub(crate) fn validate(&self) -> Result<(), ProtocolError> {
        for key in [&self.api, &self.connection] {
            if key.is_empty()
                || key.len() > 4096
                || !key.bytes().all(|c| (0x21..=0x7e).contains(&c))
            {
                return Err(unavailable(
                    "invalid Handy credential in engine environment",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn same_route(&self, other: &Self) -> bool {
        self.connection == other.connection
    }
    pub(crate) fn echoed_text(&self, text: &str) -> bool {
        text.contains(&self.api) || text.contains(&self.connection)
    }
    pub(crate) fn echoed(&self, identity: &HandyIdentity) -> bool {
        [
            &identity.firmware,
            &identity.model_name,
            &identity.session_id,
        ]
        .iter()
        .any(|text| self.echoed_text(text))
    }
}

/// Keyed native routing identity. It is neither a credential nor dispatch authority.
/// No Deserialize implementation permits client-supplied identities to become native facts.
#[derive(Clone, PartialEq, Eq)]
pub struct HandyRouteIdentity {
    namespace_sha256: String,
    route_sha256: String,
}
impl HandyRouteIdentity {
    pub fn from_snapshot(
        owner_capability: &str,
        connection_key: &str,
    ) -> Result<Self, ProtocolError> {
        use sha2::{Digest, Sha256};
        if owner_capability.len() != 64
            || !owner_capability
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || connection_key.is_empty()
            || connection_key.len() > 4096
            || !connection_key.bytes().all(|b| (0x21..=0x7e).contains(&b))
        {
            return Err(unavailable("invalid native Handy route inputs"));
        }
        let mut namespace = Sha256::new();
        namespace.update(b"pulsar.handy.setup.namespace.v1\0");
        namespace.update(owner_capability.as_bytes());
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, owner_capability.as_bytes());
        let mut mac = ring::hmac::Context::with_key(&key);
        mac.update(b"pulsar.handy.setup.route.v1\0");
        mac.update(&(connection_key.len() as u64).to_be_bytes());
        mac.update(connection_key.as_bytes());
        let route_sha256 = mac
            .sign()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(Self {
            namespace_sha256: format!("{:x}", namespace.finalize()),
            route_sha256,
        })
    }
    pub fn namespace_sha256(&self) -> &str {
        &self.namespace_sha256
    }
    pub fn route_sha256(&self) -> &str {
        &self.route_sha256
    }
}
#[cfg(test)]
mod route_tests {
    use super::*;
    #[test]
    fn handy_route_identity_matches_independent_hmac_and_is_scoped_without_plaintext() {
        let route = HandyRouteIdentity::from_snapshot(&"a".repeat(64), "route-test").unwrap();
        assert_eq!(
            route.route_sha256(),
            "dc24aee7cdc70087eda4cb733074d3a2c077a92a9c85602d29892707fb082fea"
        );
        assert!(route == HandyRouteIdentity::from_snapshot(&"a".repeat(64), "route-test").unwrap());
        let changed_owner =
            HandyRouteIdentity::from_snapshot(&"b".repeat(64), "route-test").unwrap();
        assert_ne!(route.namespace_sha256(), changed_owner.namespace_sha256());
        assert_ne!(route.route_sha256(), changed_owner.route_sha256());
        let changed_key =
            HandyRouteIdentity::from_snapshot(&"a".repeat(64), "other-route").unwrap();
        assert_eq!(route.namespace_sha256(), changed_key.namespace_sha256());
        assert_ne!(route.route_sha256(), changed_key.route_sha256());
        assert!(!route.route_sha256().contains("route-test"));
        assert!(HandyRouteIdentity::from_snapshot("invalid-owner", "route-test").is_err());
        assert!(HandyRouteIdentity::from_snapshot(&"a".repeat(64), "invalid\nheader").is_err());
    }
}
