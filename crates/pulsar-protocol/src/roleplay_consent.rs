//! Roleplay consent, age attestation, content-class and disclosure authority.
//!
//! A consent record is evidence of agreement. It is never playback authority: it
//! removes a precondition and nothing more. Three independent things must all hold
//! before an effect-class tool may run, and none substitutes for another: the
//! operator's single-use approval, every present participant's consent to the
//! declared content class *and* processing scope, and a current adult attestation.
//! The attestation is recorded and never represented as verification.
//!
//! Consent covers where the conversation is processed as well as what it may
//! contain. Talking to a local model and sending the same conversation to a remote
//! provider are different disclosures, so switching scope clears consent.
//!
//! This module is pure. It performs no I/O, holds no clock, and stores no identity
//! beyond a local pseudonymous reference and an opaque provider reference.
use crate::*;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Current wire/document schema for the retained consent documents.
pub const CONSENT_SCHEMA_VERSION: u16 = 1;
/// Version of the disclosure text the attestation acknowledged.
pub const ATTESTATION_STATEMENT_VERSION: u16 = 1;
/// Bounded participant count. Owners may be added without reworking the record.
pub const MAX_PARTICIPANTS: usize = 8;

/// Declared session content class, least permissive first.
///
/// The class bounds the tool allowlist and the model context, not only the
/// tools, because consent is not informed unless it names what may happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentClass {
    General,
    Mature,
    Explicit,
}

impl ContentClass {
    /// The default for every new session. Escalation is explicit and requires
    /// fresh consent from every participant.
    pub const fn least_permissive() -> Self {
        Self::General
    }

    /// Classes beyond `General` involve adult material.
    pub const fn requires_adult_attestation(self) -> bool {
        !matches!(self, Self::General)
    }
}

macro_rules! opaque_ref {
    ($name:ident, $label:literal) => {
        /// Local, pseudonymous reference. Deliberately carries no name, mailbox,
        /// date of birth, endpoint, credential or document.
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
                let value = value.into();
                crate::motion::validate_opaque_id(&value, $label)?;
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = ProtocolError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
opaque_ref!(ParticipantRef, "participant reference");
opaque_ref!(ProviderRef, "provider reference");

/// Where a session's conversation is processed.
///
/// Exactly one scope is declared. A remote scope names an opaque configured
/// provider; no URL, endpoint or key is accepted here or from model output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderScope {
    /// Processed entirely on this machine.
    pub local: bool,
    /// Opaque reference to an owner-configured remote provider.
    pub remote_provider: Option<ProviderRef>,
}

impl ProviderScope {
    /// Fully local processing. The only scope available offline.
    pub fn local() -> Self {
        Self {
            local: true,
            remote_provider: None,
        }
    }

    /// Remote processing through one configured provider.
    pub fn remote(provider: ProviderRef) -> Self {
        Self {
            local: false,
            remote_provider: Some(provider),
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        // Exactly one scope: local with no provider, or remote with one provider.
        if self.local != self.remote_provider.is_none() {
            return Err(ProtocolError::invalid(
                "provider scope must declare exactly one of local or remote",
            ));
        }
        if let Some(provider) = &self.remote_provider {
            ProviderRef::new(provider.as_str())?;
        }
        Ok(())
    }

    pub fn is_remote(&self) -> bool {
        self.remote_provider.is_some()
    }
}

/// Local adult attestation.
///
/// This is an attestation, not verification. No threshold, expiry or identity
/// check is imposed here because none was approved; withdrawal is the only
/// transition this record carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgeAttestation {
    pub schema_version: u16,
    /// The owner asserted they are at least 18.
    pub stated_adult: bool,
    pub attested_at_ns: u64,
    /// Which disclosure text was shown when the assertion was made.
    pub statement_version: u16,
    pub withdrawn_at_ns: Option<u64>,
}

impl AgeAttestation {
    pub fn new(attested_at_ns: u64) -> Self {
        Self {
            schema_version: CONSENT_SCHEMA_VERSION,
            stated_adult: true,
            attested_at_ns,
            statement_version: ATTESTATION_STATEMENT_VERSION,
            withdrawn_at_ns: None,
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != CONSENT_SCHEMA_VERSION
            || !self.stated_adult
            || self.attested_at_ns == 0
            || self.statement_version != ATTESTATION_STATEMENT_VERSION
            || self
                .withdrawn_at_ns
                .is_some_and(|withdrawn| withdrawn < self.attested_at_ns)
        {
            return Err(ProtocolError::invalid("adult attestation document differs"));
        }
        Ok(())
    }

    /// Current at `now_ns`: stated, not withdrawn, and not dated in the future.
    pub fn current_at(&self, now_ns: u64) -> bool {
        self.validate().is_ok() && self.withdrawn_at_ns.is_none() && self.attested_at_ns <= now_ns
    }

    pub fn withdraw(&mut self, now_ns: u64) {
        if self.withdrawn_at_ns.is_none() {
            self.withdrawn_at_ns = Some(now_ns.max(self.attested_at_ns));
        }
    }
}

/// One participant's consent to the session's declared content class and scope.
///
/// Consent is specific to a class, a disclosure scope and an occasion. There is no
/// constructor that grants consent without all three, and nothing inherits a record
/// from a prior session, an existing approval or a profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentRecord {
    pub participant: ParticipantRef,
    pub content_class: ContentClass,
    pub provider: ProviderScope,
    pub granted_at_ns: u64,
    pub revoked_at_ns: Option<u64>,
}

impl ConsentRecord {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        crate::motion::validate_opaque_id(self.participant.as_str(), "consent participant")?;
        self.provider.validate()?;
        if self.granted_at_ns == 0
            || self
                .revoked_at_ns
                .is_some_and(|revoked| revoked < self.granted_at_ns)
        {
            return Err(ProtocolError::invalid("consent record ordering differs"));
        }
        Ok(())
    }

    pub fn revoked(&self) -> bool {
        self.revoked_at_ns.is_some()
    }
}

/// Why an admission was refused. Labels are stable and carry no content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsentRefusal {
    AdultAttestationMissing,
    AdultAttestationWithdrawn,
    AdultAttestationNotYetValid,
    ConsentAbsent,
    ConsentRevoked,
    RemoteDisclosureUnconsented,
    ContentClassEscalation,
    ContentClassRegression,
    ProviderScopeUnchanged,
}

impl ConsentRefusal {
    /// Bounded diagnostic label; never includes participant or provider text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::AdultAttestationMissing => "adult_attestation_missing",
            Self::AdultAttestationWithdrawn => "adult_attestation_withdrawn",
            Self::AdultAttestationNotYetValid => "adult_attestation_not_yet_valid",
            Self::ConsentAbsent => "consent_absent",
            Self::ConsentRevoked => "consent_revoked",
            Self::RemoteDisclosureUnconsented => "remote_disclosure_unconsented",
            Self::ContentClassEscalation => "content_class_escalation",
            Self::ContentClassRegression => "content_class_regression",
            Self::ProviderScopeUnchanged => "provider_scope_unchanged",
        }
    }

    pub fn message(self) -> String {
        match self {
            Self::AdultAttestationMissing => {
                "roleplay requires a current adult attestation".to_string()
            }
            Self::AdultAttestationWithdrawn => "the adult attestation was withdrawn".to_string(),
            Self::AdultAttestationNotYetValid => {
                "the adult attestation is not yet current".to_string()
            }
            Self::ConsentAbsent => "a present participant has not consented".to_string(),
            Self::ConsentRevoked => "a present participant revoked consent".to_string(),
            Self::RemoteDisclosureUnconsented => {
                "a present participant has not consented to remote processing".to_string()
            }
            Self::ContentClassEscalation => {
                "the requested content class was not consented to".to_string()
            }
            Self::ContentClassRegression => {
                "the requested content class is below the declared class".to_string()
            }
            Self::ProviderScopeUnchanged => "the declared provider scope is unchanged".to_string(),
        }
    }

    pub fn error(self) -> ProtocolError {
        let code = match self {
            Self::ContentClassEscalation
            | Self::ContentClassRegression
            | Self::ProviderScopeUnchanged => ErrorCode::RequestConflict,
            _ => ErrorCode::Forbidden,
        };
        ProtocolError::new(code, self.message())
    }
}

/// Retained consent state for one roleplay session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleplaySessionConsent {
    pub schema_version: u16,
    /// Declared content class. Bounds both tools and model context.
    pub declared_class: ContentClass,
    /// Declared processing scope. Bounds where the conversation may be sent.
    pub declared_provider: ProviderScope,
    /// Sorted strictly ascending by participant reference; never duplicated.
    pub participants: Vec<ConsentRecord>,
}

impl RoleplaySessionConsent {
    /// A new session always starts local and at the least permissive class.
    pub fn declare(declared_class: ContentClass) -> Self {
        Self::declare_with(declared_class, ProviderScope::local())
    }

    pub fn declare_with(declared_class: ContentClass, declared_provider: ProviderScope) -> Self {
        Self {
            schema_version: CONSENT_SCHEMA_VERSION,
            declared_class,
            declared_provider,
            participants: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != CONSENT_SCHEMA_VERSION
            || self.participants.len() > MAX_PARTICIPANTS
        {
            return Err(ProtocolError::invalid("roleplay consent document differs"));
        }
        self.declared_provider.validate()?;
        for record in &self.participants {
            record.validate()?;
            // A revoked record is history: it keeps the class and scope that were
            // actually consented to, which need not match the current declaration.
            if record.revoked() {
                continue;
            }
            if record.content_class != self.declared_class {
                return Err(ProtocolError::invalid(
                    "consent record class differs from the declared class",
                ));
            }
            if record.provider != self.declared_provider {
                return Err(ProtocolError::invalid(
                    "consent record scope differs from the declared scope",
                ));
            }
        }
        if self
            .participants
            .windows(2)
            .any(|pair| pair[0].participant >= pair[1].participant)
        {
            return Err(ProtocolError::invalid("participant order differs"));
        }
        Ok(())
    }

    /// Record one participant's consent to the declared class and scope.
    ///
    /// A participant cannot consent to a class the session has not declared; the
    /// session must escalate first, which clears every existing consent.
    pub fn grant(
        &mut self,
        participant: ParticipantRef,
        content_class: ContentClass,
        now_ns: u64,
    ) -> Result<(), ProtocolError> {
        if content_class != self.declared_class {
            return Err(ConsentRefusal::ContentClassEscalation.error());
        }
        let provider = self.declared_provider.clone();
        match self
            .participants
            .binary_search_by(|record| record.participant.cmp(&participant))
        {
            Ok(index) => {
                let record = &mut self.participants[index];
                if !record.revoked() {
                    return Err(ProtocolError::new(
                        ErrorCode::RequestConflict,
                        "participant already consented to this class",
                    ));
                }
                // A fresh grant after revocation is a new record with its own time.
                *record = ConsentRecord {
                    participant,
                    content_class,
                    provider,
                    granted_at_ns: now_ns,
                    revoked_at_ns: None,
                };
                Ok(())
            }
            Err(index) => {
                if self.participants.len() >= MAX_PARTICIPANTS {
                    return Err(ProtocolError::new(
                        ErrorCode::ResourceExhausted,
                        "roleplay session participant limit reached",
                    ));
                }
                self.participants.insert(
                    index,
                    ConsentRecord {
                        participant,
                        content_class,
                        provider,
                        granted_at_ns: now_ns,
                        revoked_at_ns: None,
                    },
                );
                Ok(())
            }
        }
    }

    /// Withdraw one participant's consent. Unconditional and idempotent.
    ///
    /// This is the consent analogue of stop: it requires no approval, no
    /// operator cooperation and no matching grant, and the first revocation time
    /// is retained rather than rewritten.
    pub fn revoke(
        &mut self,
        participant: &ParticipantRef,
        now_ns: u64,
    ) -> Result<(), ProtocolError> {
        let index = self
            .participants
            .binary_search_by(|record| record.participant.cmp(participant))
            .map_err(|_| ProtocolError::new(ErrorCode::NotFound, "participant is not present"))?;
        let record = &mut self.participants[index];
        if record.revoked_at_ns.is_none() {
            record.revoked_at_ns = Some(now_ns.max(record.granted_at_ns));
        }
        Ok(())
    }

    /// Move the session to a more explicit class.
    ///
    /// Every existing consent is cleared, because consent to the previous class is
    /// not consent to this one.
    pub fn escalate(
        &mut self,
        content_class: ContentClass,
        now_ns: u64,
    ) -> Result<(), ProtocolError> {
        if content_class <= self.declared_class {
            return Err(ConsentRefusal::ContentClassRegression.error());
        }
        self.declared_class = content_class;
        self.clear_consent(now_ns);
        Ok(())
    }

    /// Move the session between local and remote processing.
    ///
    /// Every existing consent is cleared: talking to a local model is not consent
    /// to disclose the same conversation to a third party, and returning to local
    /// processing is a new declaration rather than a restoration of old consent.
    pub fn switch_provider(
        &mut self,
        provider: ProviderScope,
        now_ns: u64,
    ) -> Result<(), ProtocolError> {
        provider
            .validate()
            .map_err(|_| ConsentRefusal::ProviderScopeUnchanged.error())?;
        if provider == self.declared_provider {
            return Err(ConsentRefusal::ProviderScopeUnchanged.error());
        }
        self.declared_provider = provider;
        self.clear_consent(now_ns);
        Ok(())
    }

    fn clear_consent(&mut self, now_ns: u64) {
        for record in &mut self.participants {
            if record.revoked_at_ns.is_none() {
                record.revoked_at_ns = Some(now_ns.max(record.granted_at_ns));
            }
        }
    }

    /// Roleplay itself requires a current adult attestation, whatever the class.
    pub fn admit_roleplay(
        &self,
        attestation: Option<&AgeAttestation>,
        now_ns: u64,
    ) -> Result<(), ConsentRefusal> {
        let attestation = attestation.ok_or(ConsentRefusal::AdultAttestationMissing)?;
        match attestation.withdrawn_at_ns {
            Some(_) => return Err(ConsentRefusal::AdultAttestationWithdrawn),
            None if attestation.attested_at_ns > now_ns => {
                return Err(ConsentRefusal::AdultAttestationNotYetValid)
            }
            None if !attestation.stated_adult
                || attestation.schema_version != CONSENT_SCHEMA_VERSION
                || attestation.statement_version != ATTESTATION_STATEMENT_VERSION =>
            {
                return Err(ConsentRefusal::AdultAttestationMissing)
            }
            None => {}
        }
        Ok(())
    }

    /// The effect-class precondition: current attestation and every participant's
    /// unrevoked consent for the declared class and processing scope.
    ///
    /// Passing this does not authorise anything. The single-use approval for the
    /// exact call is still required and is tracked separately.
    pub fn admit_effect(
        &self,
        attestation: Option<&AgeAttestation>,
        now_ns: u64,
    ) -> Result<(), ConsentRefusal> {
        self.admit_roleplay(attestation, now_ns)?;
        if self.participants.is_empty() {
            return Err(ConsentRefusal::ConsentAbsent);
        }
        for record in &self.participants {
            if record.revoked() {
                return Err(ConsentRefusal::ConsentRevoked);
            }
            if record.content_class != self.declared_class
                || record.granted_at_ns > now_ns
                || record.provider != self.declared_provider
            {
                return Err(
                    if record.provider != self.declared_provider
                        && self.declared_provider.is_remote()
                    {
                        ConsentRefusal::RemoteDisclosureUnconsented
                    } else {
                        ConsentRefusal::ConsentAbsent
                    },
                );
            }
        }
        Ok(())
    }
}

/// One locally retained roleplay consent document.
///
/// The attestation travels with the session it was given for, so a consent record
/// can never be presented without the adult attestation that accompanied it. The
/// document is local safety state: not a project artifact, evidence receipt or
/// diagnostic attachment (CNS-07).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleplayConsentDocument {
    pub schema_version: u16,
    pub session: RoleplaySessionConsent,
    pub attestation: Option<AgeAttestation>,
}

impl RoleplayConsentDocument {
    pub fn new(session: RoleplaySessionConsent) -> Self {
        Self {
            schema_version: CONSENT_SCHEMA_VERSION,
            session,
            attestation: None,
        }
    }

    /// A fresh session: local scope, least permissive class.
    pub fn least_permissive() -> Self {
        Self::new(RoleplaySessionConsent::declare(
            ContentClass::least_permissive(),
        ))
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != CONSENT_SCHEMA_VERSION {
            return Err(ProtocolError::invalid(
                "roleplay consent document version differs",
            ));
        }
        self.session.validate()?;
        if let Some(attestation) = &self.attestation {
            attestation.validate()?;
        }
        Ok(())
    }

    pub fn attest(&mut self, now_ns: u64) {
        self.attestation = Some(AgeAttestation::new(now_ns));
    }

    pub fn withdraw_attestation(&mut self, now_ns: u64) {
        if let Some(attestation) = &mut self.attestation {
            attestation.withdraw(now_ns);
        }
    }

    pub fn admit_roleplay(&self, now_ns: u64) -> Result<(), ConsentRefusal> {
        self.session
            .admit_roleplay(self.attestation.as_ref(), now_ns)
    }

    pub fn admit_effect(&self, now_ns: u64) -> Result<(), ConsentRefusal> {
        self.session.admit_effect(self.attestation.as_ref(), now_ns)
    }

    /// Check the consent precondition for the exact proposed effect scope.
    /// Successful consent still requires a separate approval for the effect.
    pub fn admit_effect_for(
        &self,
        participants: &[ParticipantRef],
        content_class: ContentClass,
        provider: &ProviderScope,
        now_ns: u64,
    ) -> Result<(), ConsentRefusal> {
        self.admit_effect(now_ns)?;
        if content_class != self.session.declared_class
            || provider != &self.session.declared_provider
            || participants.len() != self.session.participants.len()
            || !self
                .session
                .participants
                .iter()
                .all(|record| participants.contains(&record.participant))
        {
            return Err(ConsentRefusal::ConsentAbsent);
        }
        Ok(())
    }

    /// Redacted operational view: class, scope and local references only.
    pub fn summary(&self, now_ns: u64) -> ConsentSummary {
        let effect = self.admit_effect(now_ns);
        ConsentSummary {
            content_class: self.session.declared_class,
            remote: self.session.declared_provider.is_remote(),
            participants: self
                .session
                .participants
                .iter()
                .map(|record| ParticipantSummary {
                    reference: record.participant.as_str().to_string(),
                    revoked: record.revoked(),
                })
                .collect(),
            attested: self
                .attestation
                .as_ref()
                .is_some_and(|attestation| attestation.current_at(now_ns)),
            roleplay_admitted: self.admit_roleplay(now_ns).is_ok(),
            effect_refusal: effect.err().map(ConsentRefusal::label),
            effect_admitted: effect.is_ok(),
        }
    }
}

/// Redacted operational view of a consent document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConsentSummary {
    pub content_class: ContentClass,
    pub remote: bool,
    pub participants: Vec<ParticipantSummary>,
    pub attested: bool,
    pub roleplay_admitted: bool,
    pub effect_admitted: bool,
    pub effect_refusal: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ParticipantSummary {
    pub reference: String,
    pub revoked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn participant(id: &str) -> ParticipantRef {
        ParticipantRef::new(id).unwrap()
    }

    fn provider(id: &str) -> ProviderRef {
        ProviderRef::new(id).unwrap()
    }

    fn attested(now: u64) -> AgeAttestation {
        AgeAttestation::new(now)
    }

    fn session(class: ContentClass) -> RoleplaySessionConsent {
        let mut session = RoleplaySessionConsent::declare(class);
        session.grant(participant("operator"), class, 10).unwrap();
        session
    }

    #[test]
    fn a_new_session_starts_least_permissive_and_local() {
        let session = RoleplaySessionConsent::declare(ContentClass::least_permissive());
        assert_eq!(session.declared_class, ContentClass::General);
        assert_eq!(session.declared_provider, ProviderScope::local());
        assert!(!session.declared_provider.is_remote());
    }

    #[test]
    fn roleplay_requires_a_current_adult_attestation_even_for_general() {
        let session = session(ContentClass::General);
        assert_eq!(
            session.admit_roleplay(None, 100),
            Err(ConsentRefusal::AdultAttestationMissing)
        );
        assert_eq!(session.admit_roleplay(Some(&attested(100)), 100), Ok(()));
    }

    #[test]
    fn a_future_attestation_is_not_current() {
        let session = session(ContentClass::General);
        assert_eq!(
            session.admit_roleplay(Some(&attested(500)), 100),
            Err(ConsentRefusal::AdultAttestationNotYetValid)
        );
    }

    #[test]
    fn withdrawn_attestation_refuses_roleplay_and_effect() {
        let mut attestation = attested(10);
        attestation.withdraw(50);
        let session = session(ContentClass::Mature);
        assert_eq!(
            session.admit_effect(Some(&attestation), 100),
            Err(ConsentRefusal::AdultAttestationWithdrawn)
        );
    }

    #[test]
    fn effect_scope_requires_every_exact_participant_class_and_provider() {
        let mut document = RoleplayConsentDocument::least_permissive();
        document.attest(1);
        let first = participant("first");
        let second = participant("second");
        let class = ContentClass::General;
        let local = ProviderScope::local();
        document.session.grant(first.clone(), class, 2).unwrap();
        document.session.grant(second.clone(), class, 2).unwrap();
        let exact = [second.clone(), first.clone()];
        assert_eq!(document.admit_effect_for(&exact, class, &local, 3), Ok(()));
        for wrong in [
            vec![first.clone()],
            vec![first.clone(), first.clone()],
            vec![first.clone(), participant("other")],
            vec![first, second, participant("other")],
        ] {
            assert_eq!(
                document.admit_effect_for(&wrong, class, &local, 3),
                Err(ConsentRefusal::ConsentAbsent)
            );
        }
        assert_eq!(
            document.admit_effect_for(&exact, ContentClass::Mature, &local, 3),
            Err(ConsentRefusal::ConsentAbsent)
        );
        assert_eq!(
            document.admit_effect_for(
                &exact,
                class,
                &ProviderScope::remote(ProviderRef::new("remote-fixture").unwrap()),
                3,
            ),
            Err(ConsentRefusal::ConsentAbsent)
        );
    }

    #[test]
    fn effect_requires_an_explicit_consent_record() {
        let empty = RoleplaySessionConsent::declare(ContentClass::Mature);
        assert_eq!(
            empty.admit_effect(Some(&attested(10)), 100),
            Err(ConsentRefusal::ConsentAbsent)
        );
    }

    #[test]
    fn effect_succeeds_only_with_attestation_and_consent() {
        let session = session(ContentClass::Mature);
        assert_eq!(
            session.admit_effect(None, 100),
            Err(ConsentRefusal::AdultAttestationMissing)
        );
        assert_eq!(session.admit_effect(Some(&attested(10)), 100), Ok(()));
    }

    #[test]
    fn revocation_refuses_effect_and_is_idempotent() {
        let mut session = session(ContentClass::Explicit);
        session.revoke(&participant("operator"), 60).unwrap();
        session.revoke(&participant("operator"), 90).unwrap();
        assert_eq!(
            session.participants[0].revoked_at_ns,
            Some(60),
            "the first revocation time is retained"
        );
        assert_eq!(
            session.admit_effect(Some(&attested(10)), 100),
            Err(ConsentRefusal::ConsentRevoked)
        );
    }

    #[test]
    fn revoking_an_absent_participant_is_not_fabricated() {
        let mut session = session(ContentClass::Mature);
        let error = session
            .revoke(&participant("someone-else"), 60)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(session.participants.len(), 1);
    }

    #[test]
    fn regranting_after_revocation_needs_the_explicit_call() {
        let mut session = session(ContentClass::Mature);
        session.revoke(&participant("operator"), 60).unwrap();
        assert!(session.admit_effect(Some(&attested(10)), 100).is_err());
        session
            .grant(participant("operator"), ContentClass::Mature, 70)
            .unwrap();
        assert_eq!(session.admit_effect(Some(&attested(10)), 100), Ok(()));
        assert_eq!(session.participants[0].granted_at_ns, 70);
    }

    #[test]
    fn a_participant_cannot_consent_to_a_class_the_session_has_not_declared() {
        let mut session = RoleplaySessionConsent::declare(ContentClass::General);
        let error = session
            .grant(participant("operator"), ContentClass::Explicit, 10)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RequestConflict);
        assert!(session.participants.is_empty());
    }

    #[test]
    fn escalation_clears_prior_consent_and_requires_fresh_consent() {
        let mut session = session(ContentClass::General);
        assert_eq!(session.admit_effect(Some(&attested(10)), 100), Ok(()));
        session.escalate(ContentClass::Explicit, 80).unwrap();
        assert_eq!(session.declared_class, ContentClass::Explicit);
        assert_eq!(
            session.admit_effect(Some(&attested(10)), 100),
            Err(ConsentRefusal::ConsentRevoked)
        );
        session
            .grant(participant("operator"), ContentClass::Explicit, 90)
            .unwrap();
        assert_eq!(session.admit_effect(Some(&attested(10)), 100), Ok(()));
    }

    #[test]
    fn lowering_the_content_class_is_refused() {
        let mut session = session(ContentClass::Explicit);
        let error = session.escalate(ContentClass::General, 20).unwrap_err();
        assert_eq!(error.code, ErrorCode::RequestConflict);
        assert_eq!(session.declared_class, ContentClass::Explicit);
    }

    #[test]
    fn duplicate_grant_without_revocation_is_a_conflict() {
        let mut session = session(ContentClass::Mature);
        let error = session
            .grant(participant("operator"), ContentClass::Mature, 30)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RequestConflict);
    }

    #[test]
    fn records_stay_sorted_and_validation_refuses_unordered_documents() {
        let mut session = RoleplaySessionConsent::declare(ContentClass::Mature);
        session
            .grant(participant("zed"), ContentClass::Mature, 10)
            .unwrap();
        session
            .grant(participant("alpha"), ContentClass::Mature, 10)
            .unwrap();
        assert_eq!(session.participants[0].participant.as_str(), "alpha");
        session.validate().unwrap();
        session.participants.reverse();
        assert!(session.validate().is_err());
    }

    #[test]
    fn the_participant_bound_is_enforced_without_truncation() {
        let mut session = RoleplaySessionConsent::declare(ContentClass::General);
        for index in 0..MAX_PARTICIPANTS {
            session
                .grant(participant(&format!("p{index}")), ContentClass::General, 10)
                .unwrap();
        }
        let error = session
            .grant(participant("overflow"), ContentClass::General, 10)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert_eq!(session.participants.len(), MAX_PARTICIPANTS);
    }

    #[test]
    fn consent_documents_carry_no_identity_fields() {
        // A guard against later additions: a consent record may name only local
        // pseudonymous references, a class, a scope and its timestamps.
        let record = ConsentRecord {
            participant: participant("operator"),
            content_class: ContentClass::Mature,
            provider: ProviderScope::local(),
            granted_at_ns: 10,
            revoked_at_ns: None,
        };
        let value = serde_json::to_value(&record).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "content_class",
                "granted_at_ns",
                "participant",
                "provider",
                "revoked_at_ns"
            ]
        );

        let attestation = serde_json::to_value(attested(10)).unwrap();
        let mut keys: Vec<_> = attestation.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "attested_at_ns",
                "schema_version",
                "stated_adult",
                "statement_version",
                "withdrawn_at_ns"
            ]
        );
    }

    #[test]
    fn a_forged_adult_assertion_is_rejected() {
        let mut attestation = attested(10);
        attestation.stated_adult = false;
        assert!(attestation.validate().is_err());
        assert!(!attestation.current_at(100));
    }

    #[test]
    fn refusal_labels_are_stable_and_do_not_carry_content() {
        for refusal in [
            ConsentRefusal::AdultAttestationMissing,
            ConsentRefusal::ConsentRevoked,
            ConsentRefusal::ContentClassEscalation,
            ConsentRefusal::RemoteDisclosureUnconsented,
        ] {
            assert!(
                refusal
                    .label()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
                "label is stable ascii snake_case"
            );
            assert!(!refusal.message().contains("operator"));
        }
    }

    #[test]
    fn switching_to_remote_requires_fresh_disclosure_consent() {
        let mut session = session(ContentClass::Mature);
        assert_eq!(session.admit_effect(Some(&attested(10)), 100), Ok(()));

        session
            .switch_provider(ProviderScope::remote(provider("provider-a")), 80)
            .unwrap();
        // Local consent does not authorise remote disclosure.
        assert_eq!(
            session.admit_effect(Some(&attested(10)), 100),
            Err(ConsentRefusal::ConsentRevoked)
        );

        session
            .grant(participant("operator"), ContentClass::Mature, 90)
            .unwrap();
        assert_eq!(session.admit_effect(Some(&attested(10)), 100), Ok(()));
        assert_eq!(
            session.participants[0].provider,
            ProviderScope::remote(provider("provider-a"))
        );
    }

    #[test]
    fn switching_back_to_local_also_clears_consent() {
        let mut session = RoleplaySessionConsent::declare_with(
            ContentClass::Mature,
            ProviderScope::remote(provider("provider-a")),
        );
        session
            .grant(participant("operator"), ContentClass::Mature, 10)
            .unwrap();
        assert_eq!(session.admit_effect(Some(&attested(10)), 100), Ok(()));

        session.switch_provider(ProviderScope::local(), 20).unwrap();
        assert_eq!(
            session.admit_effect(Some(&attested(10)), 100),
            Err(ConsentRefusal::ConsentRevoked)
        );
    }

    #[test]
    fn an_unchanged_provider_scope_is_refused_rather_than_silently_accepted() {
        let mut session = session(ContentClass::Mature);
        let error = session
            .switch_provider(ProviderScope::local(), 30)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RequestConflict);
    }

    #[test]
    fn a_record_for_another_scope_fails_validation_and_cannot_admit() {
        let mut session = session(ContentClass::Mature);
        session.participants[0].provider = ProviderScope::remote(provider("provider-a"));
        assert!(session.validate().is_err(), "mismatched scope is refused");
        assert!(session.admit_effect(Some(&attested(10)), 100).is_err());
    }

    #[test]
    fn ambiguous_or_empty_provider_scopes_are_rejected() {
        let both = ProviderScope {
            local: true,
            remote_provider: Some(provider("provider-a")),
        };
        assert!(both.validate().is_err());
        let neither = ProviderScope {
            local: false,
            remote_provider: None,
        };
        assert!(neither.validate().is_err());
        assert!(ProviderScope::local().validate().is_ok());
        assert!(ProviderScope::remote(provider("provider-a"))
            .validate()
            .is_ok());
    }

    #[test]
    fn a_provider_reference_cannot_be_a_url() {
        // Opaque identifiers only; a URL or credential never becomes a scope.
        assert!(ProviderRef::new("https://example.invalid/v1").is_err());
        assert!(ProviderRef::new("configured-provider-a").is_ok());
    }
}
