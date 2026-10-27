//! Native image preparation metadata. A retained report is never launch authority.
use crate::{validate_sha256, AttemptId, ProjectId, ProtocolError, RequestId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImagePreparationOptions {
    pub preparation_ms: u64,
    pub retention_ms: u64,
}
impl NativeImagePreparationOptions {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !(1..=3_600_000).contains(&self.preparation_ms)
            || !(1..=3_600_000).contains(&self.retention_ms)
        {
            return Err(ProtocolError::invalid(
                "native preparation and retention must be 1..=3600000 ms",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImagePreparationIntent {
    pub operation_id: AttemptId,
    pub engine_epoch: AttemptId,
    pub owner: SessionId,
    pub project: ProjectId,
    pub revision: u64,
    pub original_request_id: RequestId,
    pub options: NativeImagePreparationOptions,
    pub image_sha256: String,
    pub owner_policy_sha256: String,
    pub declared_bytes: u64,
}
impl NativeImagePreparationIntent {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.options.validate()?;
        validate_sha256(&self.image_sha256)?;
        validate_sha256(&self.owner_policy_sha256)?;
        if self.revision >= i64::MAX as u64
            || self.declared_bytes == 0
            || self.declared_bytes > crate::MAX_NATIVE_TOTAL_BYTES
        {
            return Err(ProtocolError::invalid(
                "native preparation revision or declared size exceeds admission",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeImagePreparationState {
    Queued,
    Preparing,
    Retained,
    CancelRequested,
    Releasing,
    Released,
    Expired,
    Cancelled,
    Failed,
    Interrupted,
}
impl NativeImagePreparationState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Released | Self::Expired | Self::Cancelled | Self::Failed | Self::Interrupted
        )
    }
    pub fn can_transition_to(self, next: Self) -> bool {
        use NativeImagePreparationState::*;
        matches!(
            (self, next),
            (Queued, Preparing | CancelRequested | Failed | Interrupted)
                | (Preparing, Retained | CancelRequested | Failed | Interrupted)
                | (Retained, Releasing | Expired | Failed | Interrupted)
                | (CancelRequested, Cancelled | Failed | Interrupted)
                | (Releasing, Released | Failed | Interrupted)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImagePreparationReport {
    pub intent: NativeImagePreparationIntent,
    pub state: NativeImagePreparationState,
    pub generation: u64,
    pub elapsed_ms: u64,
    pub retained_at_elapsed_ms: Option<u64>,
    /// False until the resource owner actually confirms physical storage retirement.
    pub cleanup_confirmed: bool,
    pub error: Option<String>,
}
impl NativeImagePreparationReport {
    pub fn queued(intent: NativeImagePreparationIntent) -> Result<Self, ProtocolError> {
        let report = Self {
            intent,
            state: NativeImagePreparationState::Queued,
            generation: 0,
            elapsed_ms: 0,
            retained_at_elapsed_ms: None,
            cleanup_confirmed: false,
            error: None,
        };
        report.validate()?;
        Ok(report)
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        use NativeImagePreparationState::*;
        self.intent.validate()?;
        let minimum_generation = match self.state {
            Queued => 0,
            Preparing | CancelRequested | Failed | Interrupted => 1,
            Retained | Cancelled => 2,
            Releasing | Expired => 3,
            Released => 4,
        };
        if self.generation >= i64::MAX as u64
            || self.generation < minimum_generation
            || self.elapsed_ms >= i64::MAX as u64
            || (self.cleanup_confirmed && !self.state.is_terminal())
            || (matches!(self.state, Failed | Interrupted) != self.error.is_some())
            || self
                .error
                .as_ref()
                .is_some_and(|e| e.is_empty() || e.len() > 4096)
            || (matches!(self.state, Retained | Releasing | Released | Expired)
                && self.retained_at_elapsed_ms.is_none())
            || (matches!(self.state, Queued | Preparing | CancelRequested | Cancelled)
                && self.retained_at_elapsed_ms.is_some())
            || self
                .retained_at_elapsed_ms
                .is_some_and(|at| at > self.elapsed_ms || at >= self.intent.options.preparation_ms)
            || (self.state == Retained
                && self
                    .retained_at_elapsed_ms
                    .is_some_and(|at| self.elapsed_ms - at >= self.intent.options.retention_ms))
            || (self.state == Expired
                && self
                    .retained_at_elapsed_ms
                    .is_some_and(|at| self.elapsed_ms - at < self.intent.options.retention_ms))
            || (self.state == Queued && (self.generation != 0 || self.elapsed_ms != 0))
        {
            return Err(ProtocolError::invalid(
                "native preparation report state, timing or custody differs",
            ));
        }
        Ok(())
    }
    pub fn validate_successor(&self, next: &Self) -> Result<(), ProtocolError> {
        self.validate()?;
        next.validate()?;
        let cleanup_receipt = self.state.is_terminal()
            && next.state == self.state
            && !self.cleanup_confirmed
            && next.cleanup_confirmed
            && self.error == next.error;
        if self.intent != next.intent
            || next.generation != self.generation + 1
            || next.elapsed_ms < self.elapsed_ms
            || (!self.state.can_transition_to(next.state) && !cleanup_receipt)
            || (self.retained_at_elapsed_ms.is_some()
                && self.retained_at_elapsed_ms != next.retained_at_elapsed_ms)
            || (self.retained_at_elapsed_ms.is_none()
                && next.retained_at_elapsed_ms.is_some()
                && !(self.state == NativeImagePreparationState::Preparing
                    && next.state == NativeImagePreparationState::Retained
                    && next
                        .retained_at_elapsed_ms
                        .is_some_and(|at| at >= self.elapsed_ms && at <= next.elapsed_ms)))
        {
            return Err(ProtocolError::invalid(
                "native preparation successor changes intent, clock or lifecycle",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImagePreparationLimits {
    pub engine_epoch: AttemptId,
    pub supported: bool,
    pub preparation_max_ms: u64,
    pub retention_max_ms: u64,
    pub maximum_active: u16,
    pub maximum_records: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImagePreparationStatus {
    pub report: NativeImagePreparationReport,
    /// Availability of retained bytes only; never GPU/model readiness or authority.
    pub available: bool,
    pub producer_active: bool,
    /// Current operation clock when this engine owns its runtime; absent for history.
    pub observed_elapsed_ms: Option<u64>,
}
impl NativeImagePreparationLimits {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.preparation_max_ms == 0
            || self.preparation_max_ms > 3_600_000
            || self.retention_max_ms == 0
            || self.retention_max_ms > 3_600_000
            || self.maximum_active == 0
            || self.maximum_active > 8
            || self.maximum_records == 0
            || self.maximum_records > 4096
        {
            return Err(ProtocolError::invalid(
                "native preparation limits exceed admission",
            ));
        }
        Ok(())
    }
}
impl NativeImagePreparationStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.report.validate()?;
        if (self.available
            && (self.report.state != NativeImagePreparationState::Retained
                || self.producer_active
                || self.observed_elapsed_ms.is_none()))
            || self.observed_elapsed_ms.is_some_and(|now| {
                now < self.report.elapsed_ms
                    || (self.available
                        && self
                            .report
                            .retained_at_elapsed_ms
                            .is_some_and(|at| now - at >= self.report.intent.options.retention_ms))
            })
        {
            return Err(ProtocolError::invalid(
                "native preparation runtime projection differs",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn intent() -> NativeImagePreparationIntent {
        NativeImagePreparationIntent {
            operation_id: AttemptId::new("operation").unwrap(),
            engine_epoch: AttemptId::new("boot").unwrap(),
            owner: SessionId::new("owner").unwrap(),
            project: ProjectId::new("project").unwrap(),
            revision: 0,
            original_request_id: RequestId::new("request").unwrap(),
            options: NativeImagePreparationOptions {
                preparation_ms: 100,
                retention_ms: 20,
            },
            image_sha256: "a".repeat(64),
            owner_policy_sha256: "b".repeat(64),
            declared_bytes: 4096,
        }
    }
    #[test]
    fn native_preparation_intent_refuses_unbounded_and_unknown_declarations() {
        for (prep, retention) in [(0, 1), (1, 0), (3_600_001, 1), (1, 3_600_001)] {
            let mut bad = intent();
            bad.options = NativeImagePreparationOptions {
                preparation_ms: prep,
                retention_ms: retention,
            };
            assert!(bad.validate().is_err());
        }
        let mut bad = intent();
        bad.image_sha256 = "bad".into();
        assert!(bad.validate().is_err());
        let mut bad = intent();
        bad.declared_bytes = 0;
        assert!(bad.validate().is_err());
        let mut value = serde_json::to_value(intent()).unwrap();
        value["ready"] = serde_json::json!(true);
        assert!(serde_json::from_value::<NativeImagePreparationIntent>(value).is_err());
    }
    #[test]
    fn native_preparation_lifecycle_preserves_intent_and_reports_cleanup_separately() {
        use NativeImagePreparationState::*;
        let queued = NativeImagePreparationReport::queued(intent()).unwrap();
        let mut preparing = queued.clone();
        preparing.state = Preparing;
        preparing.generation = 1;
        queued.validate_successor(&preparing).unwrap();
        let mut retained = preparing.clone();
        retained.state = Retained;
        retained.generation = 2;
        retained.elapsed_ms = 10;
        retained.retained_at_elapsed_ms = Some(10);
        preparing.validate_successor(&retained).unwrap();
        let mut releasing = retained.clone();
        releasing.state = Releasing;
        releasing.generation = 3;
        retained.validate_successor(&releasing).unwrap();
        let mut released = releasing.clone();
        released.state = Released;
        released.generation = 4;
        releasing.validate_successor(&released).unwrap();
        assert!(!released.cleanup_confirmed);
        let mut receipt = released.clone();
        receipt.generation = 5;
        receipt.cleanup_confirmed = true;
        released.validate_successor(&receipt).unwrap();
        assert!(receipt.validate_successor(&retained).is_err());
        let mut renewed = retained.clone();
        renewed.generation = 3;
        renewed.retained_at_elapsed_ms = Some(11);
        assert!(retained.validate_successor(&renewed).is_err());
        let mut expired = retained.clone();
        expired.elapsed_ms = 30;
        assert!(expired.validate().is_err());
        expired.state = Expired;
        expired.generation = 3;
        retained.validate_successor(&expired).unwrap();
        let mut changed = preparing.clone();
        changed.intent.owner = SessionId::new("other").unwrap();
        assert!(queued.validate_successor(&changed).is_err());
        let mut fake = queued.clone();
        fake.state = Retained;
        fake.generation = 1;
        fake.retained_at_elapsed_ms = Some(0);
        assert!(fake.validate().is_err());
        assert!(queued.validate_successor(&fake).is_err());
        let mut cancelled = preparing.clone();
        cancelled.state = CancelRequested;
        cancelled.generation = 2;
        preparing.validate_successor(&cancelled).unwrap();
        let mut late = retained.clone();
        late.generation = 3;
        assert!(cancelled.validate_successor(&late).is_err());
    }
    #[test]
    fn native_preparation_publication_preserves_producer_clock_and_refuses_expired_projection() {
        let mut preparing = NativeImagePreparationReport::queued(intent()).unwrap();
        preparing.state = NativeImagePreparationState::Preparing;
        preparing.generation = 1;
        preparing.elapsed_ms = 1;
        let mut retained = preparing.clone();
        retained.state = NativeImagePreparationState::Retained;
        retained.generation = 2;
        retained.elapsed_ms = 15;
        retained.retained_at_elapsed_ms = Some(10);
        preparing.validate_successor(&retained).unwrap();
        let mut value = NativeImagePreparationStatus {
            report: retained,
            available: true,
            producer_active: false,
            observed_elapsed_ms: Some(29),
        };
        value.validate().unwrap();
        value.observed_elapsed_ms = Some(30);
        assert!(value.validate().is_err());
        value.available = false;
        value.validate().unwrap();
    }
    #[test]
    fn native_preparation_command_requires_revision_and_distinct_resolution_identity() {
        use crate::{Command, Request, RevisionId};
        let mut request = Request::new(
            RequestId::new("original").unwrap(),
            Command::StartNativeImagePreparation {
                engine_epoch: AttemptId::new("boot").unwrap(),
                options: intent().options,
            },
        );
        request.project = Some(intent().project);
        assert!(request.validate().is_err());
        request.expected_revision = Some(RevisionId::new(0));
        request.validate().unwrap();
        request.command = Command::ResolveNativeImagePreparation {
            original_request_id: request.request_id.clone(),
        };
        assert!(request.validate().is_err());
    }
}
