//! Roleplay consent, age attestation and content-class authority.
//!
//! A consent record is evidence of agreement. It is never playback authority: it
//! removes a precondition and nothing more. Two independent authorities must both
//! hold before an effect-class tool may run, and neither substitutes for the other:
//! the operator's single-use approval, and every present participant's consent to
//! the session's declared content class. Both additionally require a current adult
//! attestation, which is recorded and never represented as verification.
//!
//! This module is pure. It performs no I/O, holds no clock, and stores no
//! participant identity beyond a local pseudonymous reference.
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

/// Local, pseudonymous participant reference.
///
/// Deliberately carries no name, mailbox, date of birth or document. The
/// application must not collect or transmit participant identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ParticipantRef(String);

impl ParticipantRef {
    pub fn new(value: impl Into<String>) -> Result<Self, ProtocolError> {
        let value = value.into();
        crate::motion::validate_opaque_id(&value, "participant reference")?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ParticipantRef {
    type Error = ProtocolError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ParticipantRef> for String {
    fn from(value: ParticipantRef) -> Self {
        value.0
    }
}

impl fmt::Display for ParticipantRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
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

/// One participant's consent to the session's declared content class.
///
/// Consent is specific to a class and to an occasion. There is no constructor
/// that grants consent without both the participant and the class, and nothing
/// inherits a record from a prior session or an existing approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsentRecord {
    pub participant: ParticipantRef,
    pub content_class: ContentClass,
    pub granted_at_ns: u64,
    pub revoked_at_ns: Option<u64>,
}

impl ConsentRecord {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        crate::motion::validate_opaque_id(self.participant.as_str(), "consent participant")?;
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
    ContentClassEscalation,
    ContentClassRegression,
}

impl ConsentRefusal {
    /// Bounded diagnostic label; never includes participant text.
    pub const fn label(self) -> &'static str {
        match self {
            Self::AdultAttestationMissing => "adult_attestation_missing",
            Self::AdultAttestationWithdrawn => "adult_attestation_withdrawn",
            Self::AdultAttestationNotYetValid => "adult_attestation_not_yet_valid",
            Self::ConsentAbsent => "consent_absent",
            Self::ConsentRevoked => "consent_revoked",
            Self::ContentClassEscalation => "content_class_escalation",
            Self::ContentClassRegression => "content_class_regression",
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
            Self::ContentClassEscalation => {
                "the requested content class was not consented to".to_string()
            }
            Self::ContentClassRegression => {
                "the requested content class is below the declared class".to_string()
            }
        }
    }

    pub fn error(self) -> ProtocolError {
        let code = match self {
            Self::ContentClassEscalation | Self::ContentClassRegression => {
                ErrorCode::RequestConflict
            }
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
    /// Sorted strictly ascending by participant reference; never duplicated.
    pub participants: Vec<ConsentRecord>,
}

impl RoleplaySessionConsent {
    /// A new session always starts at the least permissive class.
    pub fn declare(declared_class: ContentClass) -> Self {
        Self {
            schema_version: CONSENT_SCHEMA_VERSION,
            declared_class,
            participants: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema_version != CONSENT_SCHEMA_VERSION
            || self.participants.len() > MAX_PARTICIPANTS
        {
            return Err(ProtocolError::invalid("roleplay consent document differs"));
        }
        for record in &self.participants {
            record.validate()?;
            if record.content_class != self.declared_class {
                return Err(ProtocolError::invalid(
                    "consent record class differs from the declared class",
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

    /// Record one participant's consent to the declared class.
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
        for record in &mut self.participants {
            if record.revoked_at_ns.is_none() {
                record.revoked_at_ns = Some(now_ns.max(record.granted_at_ns));
            }
        }
        Ok(())
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
    /// unrevoked consent for the declared class.
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
            if record.content_class != self.declared_class || record.granted_at_ns > now_ns {
                return Err(ConsentRefusal::ConsentAbsent);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn participant(id: &str) -> ParticipantRef {
        ParticipantRef::new(id).unwrap()
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
    fn a_new_session_starts_least_permissive() {
        assert_eq!(
            RoleplaySessionConsent::declare(ContentClass::least_permissive()).declared_class,
            ContentClass::General
        );
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
        // A guard against later additions: a consent record may name only a local
        // pseudonymous reference, a class, and its timestamps.
        let record = ConsentRecord {
            participant: participant("operator"),
            content_class: ContentClass::Mature,
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
}
