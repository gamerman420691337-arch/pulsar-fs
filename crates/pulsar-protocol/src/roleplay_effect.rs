//! In-memory approval lifecycle for one owner-controlled roleplay session.
use crate::{ConsentRefusal, ContentClass, ParticipantRef, ProviderScope, RoleplayConsentDocument};
use serde::Serialize;
use sha2::{Digest, Sha256};

const APPROVAL_WINDOW_NS: u64 = 30_000_000_000;

/// Content identities must come from the prepared resources at the effect owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "effect", rename_all = "snake_case")]
pub enum RoleplayEffect {
    PresentMedia { media_sha256: [u8; 32] },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RoleplayEffectScope {
    pub effect: RoleplayEffect,
    pub participants: Vec<ParticipantRef>,
    pub content_class: ContentClass,
    pub provider: ProviderScope,
}

/// Echo the entire review identity from the owner UI when confirming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectReview {
    pub sequence: u64,
    pub digest: [u8; 32],
    pub expires_at_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectApprovalRefusal {
    Consent(ConsentRefusal),
    Serialization,
    SequenceExhausted,
    ReviewChanged,
    Expired,
    ConfirmationRequired,
}

struct PendingEffect {
    review: EffectReview,
    scope_digest: [u8; 32],
    proposed_at_ns: u64,
    confirmed: bool,
}

/// Keep one instance at the effect owner. Stop invalidates the slot without
/// consulting consent or spending a tool call budget.
pub struct RoleplayEffectApproval {
    owner_nonce: [u8; 32],
    sequence: u64,
    pending: Option<PendingEffect>,
}

fn scoped_digest(
    scope: &RoleplayEffectScope,
    consent: &RoleplayConsentDocument,
    now_ns: u64,
) -> Result<[u8; 32], EffectApprovalRefusal> {
    consent
        .admit_effect_for(
            &scope.participants,
            scope.content_class,
            &scope.provider,
            now_ns,
        )
        .map_err(EffectApprovalRefusal::Consent)?;
    let bytes =
        serde_json::to_vec(&(scope, consent)).map_err(|_| EffectApprovalRefusal::Serialization)?;
    let mut digest = Sha256::new();
    digest.update(b"pulsar-roleplay-effect-v1\0");
    digest.update(bytes);
    Ok(digest.finalize().into())
}

impl RoleplayEffectApproval {
    /// The host must supply a fresh random nonce for each owner lifetime.
    pub fn new(owner_nonce: [u8; 32]) -> Self {
        Self {
            owner_nonce,
            sequence: 0,
            pending: None,
        }
    }

    pub fn propose(
        &mut self,
        scope: &RoleplayEffectScope,
        consent: &RoleplayConsentDocument,
        now_ns: u64,
    ) -> Result<EffectReview, EffectApprovalRefusal> {
        let scope_digest = scoped_digest(scope, consent, now_ns)?;
        let expires_at_ns = now_ns
            .checked_add(APPROVAL_WINDOW_NS)
            .ok_or(EffectApprovalRefusal::Expired)?;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(EffectApprovalRefusal::SequenceExhausted)?;
        let mut digest = Sha256::new();
        digest.update(b"pulsar-roleplay-review-v1\0");
        digest.update(self.owner_nonce);
        digest.update(self.sequence.to_be_bytes());
        digest.update(expires_at_ns.to_be_bytes());
        digest.update(scope_digest);
        let review = EffectReview {
            sequence: self.sequence,
            digest: digest.finalize().into(),
            expires_at_ns,
        };
        self.pending = Some(PendingEffect {
            review,
            scope_digest,
            proposed_at_ns: now_ns,
            confirmed: false,
        });
        Ok(review)
    }

    /// Call only from an explicit, authenticated owner confirmation.
    pub fn confirm(
        &mut self,
        review: EffectReview,
        scope: &RoleplayEffectScope,
        consent: &RoleplayConsentDocument,
        now_ns: u64,
    ) -> Result<(), EffectApprovalRefusal> {
        let pending = self.take_matching(review, scope, consent, now_ns)?;
        self.pending = Some(PendingEffect {
            confirmed: true,
            ..pending
        });
        Ok(())
    }

    /// Consume before dispatch so retries require a new review and confirmation.
    pub fn consume(
        &mut self,
        review: EffectReview,
        scope: &RoleplayEffectScope,
        consent: &RoleplayConsentDocument,
        now_ns: u64,
    ) -> Result<(), EffectApprovalRefusal> {
        let pending = self.take_matching(review, scope, consent, now_ns)?;
        if !pending.confirmed {
            return Err(EffectApprovalRefusal::ConfirmationRequired);
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        self.pending = None;
    }

    fn take_matching(
        &mut self,
        review: EffectReview,
        scope: &RoleplayEffectScope,
        consent: &RoleplayConsentDocument,
        now_ns: u64,
    ) -> Result<PendingEffect, EffectApprovalRefusal> {
        if self.pending.as_ref().map(|pending| pending.review) != Some(review) {
            return Err(EffectApprovalRefusal::ReviewChanged);
        }
        let pending = self
            .pending
            .take()
            .ok_or(EffectApprovalRefusal::ReviewChanged)?;
        if now_ns < pending.proposed_at_ns || now_ns >= review.expires_at_ns {
            return Err(EffectApprovalRefusal::Expired);
        }
        if scoped_digest(scope, consent, now_ns)? != pending.scope_digest {
            return Err(EffectApprovalRefusal::ReviewChanged);
        }
        Ok(pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (RoleplayEffectScope, RoleplayConsentDocument) {
        let participant = ParticipantRef::new("operator").unwrap();
        let mut consent = RoleplayConsentDocument::least_permissive();
        consent.attest(1);
        consent
            .session
            .grant(participant.clone(), ContentClass::General, 2)
            .unwrap();
        let scope = RoleplayEffectScope {
            effect: RoleplayEffect::PresentMedia {
                media_sha256: [1; 32],
            },
            participants: vec![participant],
            content_class: ContentClass::General,
            provider: ProviderScope::local(),
        };
        (scope, consent)
    }

    #[test]
    fn a_review_cannot_confirm_the_same_effect_in_another_owner_lifetime() {
        let (scope, consent) = fixture();
        let mut first = RoleplayEffectApproval::new([1; 32]);
        let mut second = RoleplayEffectApproval::new([2; 32]);
        let old = first.propose(&scope, &consent, 3).unwrap();
        let current = second.propose(&scope, &consent, 3).unwrap();
        assert_eq!(old.sequence, current.sequence);
        assert_eq!(old.expires_at_ns, current.expires_at_ns);
        assert_eq!(
            second.confirm(old, &scope, &consent, 4),
            Err(EffectApprovalRefusal::ReviewChanged)
        );
        second.confirm(current, &scope, &consent, 4).unwrap();
        second.consume(current, &scope, &consent, 5).unwrap();
    }

    #[test]
    fn confirmation_is_single_use_and_bound_to_prepared_media() {
        let (scope, consent) = fixture();
        let mut slot = RoleplayEffectApproval::new([1; 32]);
        let review = slot.propose(&scope, &consent, 3).unwrap();
        assert_eq!(
            slot.consume(review, &scope, &consent, 4),
            Err(EffectApprovalRefusal::ConfirmationRequired)
        );
        let review = slot.propose(&scope, &consent, 5).unwrap();
        slot.confirm(review, &scope, &consent, 6).unwrap();
        let mut changed = scope.clone();
        changed.effect = RoleplayEffect::PresentMedia {
            media_sha256: [2; 32],
        };
        assert_eq!(
            slot.consume(review, &changed, &consent, 7),
            Err(EffectApprovalRefusal::ReviewChanged)
        );
        let review = slot.propose(&scope, &consent, 8).unwrap();
        slot.confirm(review, &scope, &consent, 9).unwrap();
        slot.consume(review, &scope, &consent, 10).unwrap();
        assert_eq!(
            slot.consume(review, &scope, &consent, 11),
            Err(EffectApprovalRefusal::ReviewChanged)
        );
    }

    #[test]
    fn stop_expiry_and_consent_change_retire_pending_approval() {
        let (scope, mut consent) = fixture();
        let mut slot = RoleplayEffectApproval::new([1; 32]);
        let stopped = slot.propose(&scope, &consent, 3).unwrap();
        slot.confirm(stopped, &scope, &consent, 4).unwrap();
        slot.stop();
        let next = slot.propose(&scope, &consent, 5).unwrap();
        assert_eq!(
            slot.consume(stopped, &scope, &consent, 6),
            Err(EffectApprovalRefusal::ReviewChanged)
        );
        slot.confirm(next, &scope, &consent, 7).unwrap();
        assert_eq!(
            slot.consume(next, &scope, &consent, next.expires_at_ns),
            Err(EffectApprovalRefusal::Expired)
        );
        let review = slot.propose(&scope, &consent, 8).unwrap();
        slot.confirm(review, &scope, &consent, 9).unwrap();
        consent.withdraw_attestation(10);
        assert_eq!(
            slot.consume(review, &scope, &consent, 10),
            Err(EffectApprovalRefusal::Consent(
                ConsentRefusal::AdultAttestationWithdrawn
            ))
        );
    }
}
