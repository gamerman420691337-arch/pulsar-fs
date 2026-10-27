//! Explicit bounded observation consent and descriptive continuity history.
//! No serialized record restores a native clock or source capability.
use crate::*;

pub fn validate_handy_hsp_continuity_selection(
    stream: &RequestId,
    enable: &RequestId,
    duration_ms: u32,
    allow_online_observations: bool,
) -> Result<(), ProtocolError> {
    if !allow_online_observations {
        return Err(ProtocolError::new(
            ErrorCode::Forbidden,
            "continuity requires explicit ongoing online observation consent",
        ));
    }
    if stream == enable || duration_ms == 0 {
        return Err(ProtocolError::invalid(
            "invalid continuity identity or duration",
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspContinuityHistory {
    pub preparation_request_id: RequestId,
    pub stream_record_id: ArtifactId,
    pub clock_owner_id: ArtifactId,
    pub admitted_at_boottime_ns: u64,
    pub deadline_boottime_ns: u64,
    pub synchronization_sent_boottime_ns: u64,
    pub synchronization_ceiling_boottime_ns: u64,
    pub initial_clock_valid_until_boottime_ns: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspContinuityNativeStatus {
    pub observed_at_boottime_ns: u64,
    pub generation: u64,
    pub clock_valid_until_boottime_ns: u64,
    pub current: bool,
    pub retired: bool,
    pub worker_active: bool,
    pub worker_joined: bool,
    pub error: Option<ProtocolError>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspContinuityStatus {
    pub project_id: ProjectId,
    pub stream_request_id: RequestId,
    pub enable_request_id: RequestId,
    pub duration_ms: u32,
    pub history: Option<HandyHspContinuityHistory>,
    pub native: Option<HandyHspContinuityNativeStatus>,
    pub error: Option<ProtocolError>,
    pub qualified: bool,
}
fn error_valid(value: &Option<ProtocolError>) -> bool {
    value.as_ref().is_none_or(|e| {
        !e.message.is_empty() && e.message.len() <= 2048 && !e.message.contains('\0')
    })
}
impl HandyHspContinuityStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_handy_hsp_continuity_selection(
            &self.stream_request_id,
            &self.enable_request_id,
            self.duration_ms,
            true,
        )?;
        if self.qualified
            || !error_valid(&self.error)
            || (self.history.is_none() && self.native.is_some())
        {
            return Err(ProtocolError::invalid("invalid continuity observation"));
        }
        if let Some(h) = &self.history {
            validate_sha256(h.stream_record_id.as_str())?;
            validate_sha256(h.clock_owner_id.as_str())?;
            if h.preparation_request_id == self.enable_request_id
                || h.preparation_request_id == self.stream_request_id
                || h.synchronization_sent_boottime_ns > h.admitted_at_boottime_ns
                || h.admitted_at_boottime_ns
                    .checked_add(u64::from(self.duration_ms) * 1_000_000)
                    != Some(h.deadline_boottime_ns)
                || h.deadline_boottime_ns > h.synchronization_ceiling_boottime_ns
                || h.initial_clock_valid_until_boottime_ns <= h.admitted_at_boottime_ns
                || h.initial_clock_valid_until_boottime_ns > h.deadline_boottime_ns
            {
                return Err(ProtocolError::invalid(
                    "continuity history changed its original limits",
                ));
            }
            if let Some(n) = &self.native {
                if n.generation == 0
                    || n.observed_at_boottime_ns < h.admitted_at_boottime_ns
                    || n.clock_valid_until_boottime_ns > h.deadline_boottime_ns
                    || n.worker_active == n.worker_joined
                    || !error_valid(&n.error)
                    || (n.current
                        && (n.retired
                            || n.observed_at_boottime_ns >= n.clock_valid_until_boottime_ns))
                {
                    return Err(ProtocolError::invalid(
                        "continuity native state differs from original history",
                    ));
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn validate_request(r: &Request) -> Result<(), ProtocolError> {
    match &r.command {
        Command::EnableHandyHspContinuity {
            stream_request_id,
            duration_ms,
            allow_online_observations,
            owner_proof,
        } => {
            bounded_text(owner_proof, 64, "Handy owner capability")?;
            validate_handy_hsp_continuity_selection(
                stream_request_id,
                &r.request_id,
                *duration_ms,
                *allow_online_observations,
            )
        }
        Command::HandyHspContinuityStatus {
            original_request_id,
            owner_proof,
        } => {
            bounded_text(owner_proof, 64, "Handy owner capability")?;
            if original_request_id == &r.request_id {
                return Err(ProtocolError::invalid(
                    "status needs a distinct request identity",
                ));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending() -> HandyHspContinuityStatus {
        HandyHspContinuityStatus {
            project_id: ProjectId::new("project").unwrap(),
            stream_request_id: RequestId::new("stream").unwrap(),
            enable_request_id: RequestId::new("enable").unwrap(),
            duration_ms: 10,
            history: None,
            native: None,
            error: None,
            qualified: false,
        }
    }
    fn admitted() -> HandyHspContinuityStatus {
        let mut value = pending();
        value.history = Some(HandyHspContinuityHistory {
            preparation_request_id: RequestId::new("prep").unwrap(),
            stream_record_id: ArtifactId::new("a".repeat(64)).unwrap(),
            clock_owner_id: ArtifactId::new("b".repeat(64)).unwrap(),
            admitted_at_boottime_ns: 100,
            deadline_boottime_ns: 10_000_100,
            synchronization_sent_boottime_ns: 10,
            synchronization_ceiling_boottime_ns: 20_000_000,
            initial_clock_valid_until_boottime_ns: 1_000_000,
        });
        value.native = Some(HandyHspContinuityNativeStatus {
            observed_at_boottime_ns: 100,
            generation: 1,
            clock_valid_until_boottime_ns: 1_000_000,
            current: true,
            retired: false,
            worker_active: false,
            worker_joined: true,
            error: None,
        });
        value
    }
    #[test]
    fn continuity_reserved_failed_and_historical_reports_do_not_invent_native_admission() {
        let mut value = pending();
        value.validate().unwrap();
        value.error = Some(ProtocolError::new(
            ErrorCode::Unavailable,
            "admission failed",
        ));
        value.validate().unwrap();
        let mut ready = admitted();
        ready.validate().unwrap();
        let history = ready.history.clone();
        ready.native = None;
        ready.validate().unwrap();
        assert_eq!(ready.history, history);
        let mut bad = pending();
        bad.native = admitted().native;
        assert!(bad.validate().is_err());
    }
    #[test]
    fn continuity_history_rejects_deadline_renewal_and_false_worker_join() {
        for case in 0..6 {
            let mut value = admitted();
            match case {
                0 => value.history.as_mut().unwrap().deadline_boottime_ns += 1,
                1 => {
                    value
                        .history
                        .as_mut()
                        .unwrap()
                        .synchronization_ceiling_boottime_ns = 1000
                }
                2 => {
                    value
                        .history
                        .as_mut()
                        .unwrap()
                        .initial_clock_valid_until_boottime_ns = 100
                }
                3 => value.native.as_mut().unwrap().worker_active = true,
                4 => value.native.as_mut().unwrap().generation = 0,
                _ => value.native.as_mut().unwrap().observed_at_boottime_ns = 1_000_000,
            }
            assert!(value.validate().is_err(), "case {case}");
        }
        assert!(validate_handy_hsp_continuity_selection(
            &RequestId::new("s").unwrap(),
            &RequestId::new("e").unwrap(),
            u32::MAX,
            true
        )
        .is_ok());
        assert!(validate_handy_hsp_continuity_selection(
            &RequestId::new("s").unwrap(),
            &RequestId::new("e").unwrap(),
            1,
            false
        )
        .is_err());
    }
}
