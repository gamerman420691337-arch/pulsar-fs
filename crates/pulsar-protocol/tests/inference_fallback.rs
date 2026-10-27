use pulsar_protocol::*;

fn fixture() -> (InferenceFallbackPolicy, InferenceFallbackTransition) {
    let record = QualifiedProfileEquivalence {
        from_profile_sha256: "a".repeat(64),
        to_profile_sha256: "b".repeat(64),
        qualification_record_sha256: "c".repeat(64),
    };
    let transition = InferenceFallbackTransition {
        previous_attempt: AttemptId::new("previous").unwrap(),
        next_attempt: AttemptId::new("next").unwrap(),
        previous_profile_sha256: record.from_profile_sha256.clone(),
        next_profile_sha256: record.to_profile_sha256.clone(),
        qualification_record_sha256: record.qualification_record_sha256.clone(),
        fallback_ordinal: 1,
    };
    (
        InferenceFallbackPolicy::Qualified {
            equivalents: vec![record],
            maximum_fallbacks: 1,
        },
        transition,
    )
}

#[test]
fn strict_pins_and_non_backend_failures_never_fallback() {
    let (policy, t) = fixture();
    assert!(InferenceFallbackPolicy::Pinned
        .admit_transition(InferenceAttemptFailure::ProviderUnavailable, &t)
        .is_err());
    for failure in [
        InferenceAttemptFailure::Cancelled,
        InferenceAttemptFailure::Revoked,
        InferenceAttemptFailure::DependencyMismatch,
        InferenceAttemptFailure::InvalidModel,
        InferenceAttemptFailure::ResourceExhausted,
    ] {
        assert!(policy.admit_transition(failure, &t).is_err());
    }
}

#[test]
fn qualified_fallback_requires_fresh_attempt_exact_record_and_remaining_budget() {
    let (policy, mut t) = fixture();
    policy
        .admit_transition(InferenceAttemptFailure::ProviderUnavailable, &t)
        .unwrap();
    t.next_attempt = t.previous_attempt.clone();
    assert!(policy
        .admit_transition(InferenceAttemptFailure::ProviderUnavailable, &t)
        .is_err());
    t.next_attempt = AttemptId::new("next").unwrap();
    t.fallback_ordinal = 2;
    assert!(policy
        .admit_transition(InferenceAttemptFailure::ProviderUnavailable, &t)
        .is_err());
    t.fallback_ordinal = 1;
    t.qualification_record_sha256 = "d".repeat(64);
    assert!(policy
        .admit_transition(InferenceAttemptFailure::ProviderUnavailable, &t)
        .is_err());
}
