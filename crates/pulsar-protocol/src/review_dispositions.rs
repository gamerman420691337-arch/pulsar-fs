//! Explicit declarations about retained review evidence; no human-identity claim.
use crate::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDisposition {
    Resolved,
    Reopened,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<TimeRange>,
    pub review_id: String,
    pub actor: String,
    pub request_id: RequestId,
    pub base_revision: RevisionId,
    pub recorded_revision: RevisionId,
    pub disposition: ReviewDisposition,
    pub note: String,
    pub recorded_at_unix_ms: u64,
}

/// Identity binds the entire validated flag, including reason and confidence.
pub fn review_identity(flag: &ReviewFlag) -> Result<String, ProtocolError> {
    flag.validate()?;
    let bytes = serde_json::to_vec(flag).map_err(|_| ProtocolError::invalid("review encoding"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
pub fn validate_review_intent(id: &str, note: &str) -> Result<(), ProtocolError> {
    if id.len() != 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ProtocolError::invalid(
            "review ID must be a lowercase SHA-256",
        ));
    }
    if note.trim().is_empty() || note.len() > 256 || note.chars().any(char::is_control) {
        return Err(ProtocolError::invalid("review decision requires a nonempty note of at most 256 bytes without control characters"));
    }
    Ok(())
}
pub fn validate_review_decisions(
    flags: &[ReviewFlag],
    decisions: &[ReviewDecision],
) -> Result<(), ProtocolError> {
    validate_reviews(flags)?;
    if decisions.len() > 1024 {
        return Err(ProtocolError::invalid("too many review decisions"));
    }
    let ids = flags
        .iter()
        .map(|f| Ok((review_identity(f)?, f.range)))
        .collect::<Result<std::collections::BTreeMap<_, _>, ProtocolError>>()?;
    let mut seen = BTreeSet::new();
    let mut previous = None;
    for d in decisions {
        validate_review_intent(&d.review_id, &d.note)?;
        if d.actor.is_empty()
            || d.actor.len() > 256
            || !d
                .actor
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
        {
            return Err(ProtocolError::invalid(
                "review actor attribution is invalid",
            ));
        }
        let key = (d.review_id.clone(), d.recorded_revision);
        let flag_range = ids
            .get(&d.review_id)
            .ok_or_else(|| ProtocolError::invalid("review evidence is absent"))?;
        if d.range
            .is_some_and(|r| r.start() < flag_range.start() || r.end() > flag_range.end())
            || !seen.insert(key.clone())
            || previous.as_ref().is_some_and(|p| p >= &key)
            || (d.range.is_none()
                && previous
                    .as_ref()
                    .is_some_and(|p: &(String, RevisionId)| p.0 == d.review_id))
            || d.base_revision.checked_next().ok() != Some(d.recorded_revision)
            || d.recorded_at_unix_ms == 0
        {
            return Err(ProtocolError::invalid(
                "review decision must uniquely bind retained evidence and consecutive revisions",
            ));
        }
        previous = Some(key);
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewEntry {
    pub review_id: String,
    pub flag: ReviewFlag,
    pub decision: Option<ReviewDecision>,
    /// Chronological whole baseline plus interval overrides, when present.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decision_history: Vec<ReviewDecision>,
}
/// Latest declarations win only within their exact half-open interval.
/// This is a review projection: it neither edits evidence nor fills motion gaps.
pub fn review_unresolved_ranges(
    flag: &ReviewFlag,
    decisions: &[ReviewDecision],
) -> Result<Vec<TimeRange>, ProtocolError> {
    validate_review_decisions(std::slice::from_ref(flag), decisions)?;
    let mut ranges = vec![flag.range];
    for d in decisions {
        let selected = d.range.unwrap_or(flag.range);
        match d.disposition {
            ReviewDisposition::Resolved => {
                let mut next = Vec::new();
                for r in ranges {
                    if selected.end() <= r.start() || selected.start() >= r.end() {
                        next.push(r);
                        continue;
                    }
                    if r.start() < selected.start() {
                        next.push(
                            TimeRange::new(r.start(), selected.start())
                                .map_err(|e| ProtocolError::invalid(e.to_string()))?,
                        );
                    }
                    if r.end() > selected.end() {
                        next.push(
                            TimeRange::new(selected.end(), r.end())
                                .map_err(|e| ProtocolError::invalid(e.to_string()))?,
                        );
                    }
                }
                ranges = next;
            }
            ReviewDisposition::Reopened => {
                ranges.push(selected);
                ranges.sort_by_key(|r| r.start());
                let mut merged: Vec<TimeRange> = Vec::new();
                for r in ranges {
                    if let Some(last) = merged.last_mut() {
                        if last.end() >= r.start() {
                            *last = TimeRange::new(last.start(), last.end().max(r.end()))
                                .map_err(|e| ProtocolError::invalid(e.to_string()))?;
                            continue;
                        }
                    }
                    merged.push(r);
                }
                ranges = merged;
            }
        }
    }
    Ok(ranges)
}
impl ReviewEntry {
    pub fn unresolved_ranges(&self) -> Result<Vec<TimeRange>, ProtocolError> {
        if !self.decision_history.is_empty() {
            review_unresolved_ranges(&self.flag, &self.decision_history)
        } else {
            review_unresolved_ranges(&self.flag, self.decision.as_slice())
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewStatus {
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub entries: Vec<ReviewEntry>,
    pub unknown: bool,
    pub conservative: bool,
}
impl ReviewStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.entries.len() > 1024 {
            return Err(ProtocolError::invalid(
                "review status exceeds flag admission",
            ));
        }
        let mut ids = BTreeSet::new();
        for e in &self.entries {
            if review_identity(&e.flag)? != e.review_id || !ids.insert(&e.review_id) {
                return Err(ProtocolError::invalid(
                    "review status has changed or duplicate evidence IDs",
                ));
            }
            if let Some(d) = &e.decision {
                if !e.decision_history.is_empty() || d.range.is_some() {
                    return Err(ProtocolError::invalid(
                        "interval review history must not masquerade as a whole-flag decision",
                    ));
                }
                validate_review_decisions(std::slice::from_ref(&e.flag), std::slice::from_ref(d))?;
                if d.recorded_revision > self.revision {
                    return Err(ProtocolError::invalid(
                        "review decision follows status revision",
                    ));
                }
            }
            if !e.decision_history.is_empty() {
                if !e.decision_history.iter().any(|d| d.range.is_some())
                    || e.decision_history
                        .iter()
                        .any(|d| d.recorded_revision > self.revision)
                {
                    return Err(ProtocolError::invalid("invalid interval review history"));
                }
                validate_review_decisions(std::slice::from_ref(&e.flag), &e.decision_history)?;
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| ProtocolError::invalid("review status encoding"))?
            .len()
            > MAX_CONTROL_BYTES - 4096
        {
            return Err(ProtocolError::invalid(
                "review status exceeds control-byte admission",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn flag() -> ReviewFlag {
        ReviewFlag {
            axis: Some(Axis::Stroke),
            range: TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(10)).unwrap(),
            reason: ReviewReason::new("fixture_uncertainty").unwrap(),
            evidence: EvidenceKind::Inferred,
            confidence: None,
        }
    }
    #[test]
    fn decisions_bind_complete_flags_and_refuse_duplicates_missing_evidence_and_bad_revisions() {
        let f = flag();
        let id = review_identity(&f).unwrap();
        let d = ReviewDecision {
            range: None,
            review_id: id.clone(),
            actor: "fixture-actor".into(),
            request_id: RequestId::new("fixture-request").unwrap(),
            base_revision: RevisionId::new(1),
            recorded_revision: RevisionId::new(2),
            disposition: ReviewDisposition::Resolved,
            note: "Reviewed fixture interval".into(),
            recorded_at_unix_ms: 1,
        };
        validate_review_decisions(std::slice::from_ref(&f), std::slice::from_ref(&d)).unwrap();
        assert!(
            validate_review_decisions(std::slice::from_ref(&f), &[d.clone(), d.clone()]).is_err()
        );
        let mut changed = f.clone();
        changed.reason = ReviewReason::new("another_reason").unwrap();
        assert!(validate_review_decisions(&[changed], std::slice::from_ref(&d)).is_err());
        let mut changed = d;
        changed.recorded_revision = RevisionId::new(3);
        assert!(validate_review_decisions(&[f], &[changed]).is_err());
        for note in ["", " ", "contains\ncontrol"] {
            assert!(validate_review_intent(&id, note).is_err());
        }
    }
    #[test]
    fn interval_overlay_preserves_boundaries_and_latest_reopen_wins() {
        let f = flag();
        let id = review_identity(&f).unwrap();
        let decision = |rev, start, end, disposition| ReviewDecision {
            range: Some(
                TimeRange::new(ProjectTime::from_nanos(start), ProjectTime::from_nanos(end))
                    .unwrap(),
            ),
            review_id: id.clone(),
            actor: "actor".into(),
            request_id: RequestId::new(format!("r-{rev}")).unwrap(),
            base_revision: RevisionId::new(rev - 1),
            recorded_revision: RevisionId::new(rev),
            disposition,
            note: "Inspected".into(),
            recorded_at_unix_ms: 1,
        };
        let mut ds = vec![decision(1, 2, 8, ReviewDisposition::Resolved)];
        let ranges = |ds: &[ReviewDecision]| {
            review_unresolved_ranges(&f, ds)
                .unwrap()
                .iter()
                .map(|r| (r.start().as_nanos(), r.end().as_nanos()))
                .collect::<Vec<_>>()
        };
        assert_eq!(ranges(&ds), vec![(0, 2), (8, 10)]);
        ds.push(decision(2, 4, 6, ReviewDisposition::Reopened));
        assert_eq!(ranges(&ds), vec![(0, 2), (4, 6), (8, 10)]);
        ds.push(decision(3, 0, 10, ReviewDisposition::Resolved));
        assert!(ranges(&ds).is_empty());
        ds.push(decision(4, 1, 3, ReviewDisposition::Reopened));
        assert_eq!(ranges(&ds), vec![(1, 3)]);
        let mut bad = ds.clone();
        bad[0].range =
            Some(TimeRange::new(ProjectTime::from_nanos(-1), ProjectTime::from_nanos(8)).unwrap());
        assert!(review_unresolved_ranges(&f, &bad).is_err());
        let mut bad = ds;
        bad.swap(0, 1);
        assert!(review_unresolved_ranges(&f, &bad).is_err());
        assert_eq!(
            f.range,
            TimeRange::new(ProjectTime::ZERO, ProjectTime::from_nanos(10)).unwrap()
        );
    }
}
