//! Read-only HSP observations. These DTOs cannot restore native authority, prove
//! physical motion or stopping, or qualify the streaming playback path.
use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspRuntimeState {
    AwaitingSetup,
    Prefilling,
    Playing,
    StopRequired,
    StopInFlight,
    StopAcknowledged,
    StopUnknown,
    SetupCancelled,
    /// Native-only proof that Setup was not sent; no durable cancellation claim.
    SetupUnsent,
    StopUnclaimed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspEffectKind {
    Setup,
    Append,
    Play,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspEffectStatus {
    pub sequence: u64,
    pub kind: HandyHspEffectKind,
}
impl HandyHspEffectStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.sequence == 0 {
            return Err(ProtocolError::invalid("invalid HSP effect sequence"));
        }
        Ok(())
    }
}

/// The immutable first result after the native writer was joined. Acknowledged
/// is a transport observation; Unknown must not become Acknowledged after a late
/// reply. Rejected means this attempt did not claim execution authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspJoinedOutcome {
    Acknowledged,
    Unknown,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspJoinedObservation {
    pub effect: HandyHspEffectStatus,
    pub outcome: HandyHspJoinedOutcome,
}

/// Native input closure and software phase passage, never physical completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyHspInputState {
    Sealed,
    Draining,
    TailReached,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspFiniteDrainStatus {
    pub input_state: HandyHspInputState,
    pub terminal_time_ms: u32,
    pub terminal_position_percent: u8,
    pub terminal_source_sequence: u64,
    pub captured_at_ns: u64,
    pub accepted_at_ns: u64,
    pub closed_at_boottime_ns: u64,
    pub expires_at_boottime_ns: u64,
    pub tail_reached_at_boottime_ns: Option<u64>,
    pub stop_dispatch_deadline_boottime_ns: Option<u64>,
    pub stop_dispatched_at_boottime_ns: Option<u64>,
    pub stop_deadline_missed: bool,
    pub aborted: bool,
}
impl HandyHspFiniteDrainStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.terminal_position_percent > 100
            || self.terminal_source_sequence == 0
            || self.captured_at_ns > self.accepted_at_ns
            || self.captured_at_ns > self.closed_at_boottime_ns
            || self.closed_at_boottime_ns >= self.expires_at_boottime_ns
            || self.tail_reached_at_boottime_ns.is_some()
                != self.stop_dispatch_deadline_boottime_ns.is_some()
            || (self.input_state == HandyHspInputState::TailReached)
                != self.tail_reached_at_boottime_ns.is_some()
            || self.tail_reached_at_boottime_ns.is_some_and(|at| {
                at < self.closed_at_boottime_ns || at >= self.expires_at_boottime_ns
            })
            || self
                .stop_dispatch_deadline_boottime_ns
                .is_some_and(|until| {
                    self.tail_reached_at_boottime_ns
                        .is_none_or(|at| until <= at)
                })
            || self.stop_dispatched_at_boottime_ns.is_some_and(|at| {
                self.tail_reached_at_boottime_ns
                    .is_none_or(|start| at < start)
            })
            || (self.stop_deadline_missed && self.stop_dispatch_deadline_boottime_ns.is_none())
            || self
                .stop_dispatched_at_boottime_ns
                .zip(self.stop_dispatch_deadline_boottime_ns)
                .is_some_and(|(at, until)| at > until && !self.stop_deadline_missed)
        {
            return Err(ProtocolError::invalid(
                "inconsistent finite HSP drain observation",
            ));
        }
        Ok(())
    }
}

/// A bounded snapshot of an existing native session. Reading this snapshot does
/// not tick its watchdog, renew a lease, settle a writer, or contact the device.
/// It may lag the native owner. Stop flags describe native work, never the
/// device's physical state or whether a human can safely use it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspNativeStatus {
    pub runtime_state: HandyHspRuntimeState,
    pub native_active: bool,
    pub current_effect: Option<HandyHspEffectStatus>,
    pub accepted_samples: u64,
    pub pending_points: u32,
    pub retained_points: u32,
    pub buffered_points: u32,
    pub total_added: u64,
    pub last_joined: Option<HandyHspJoinedObservation>,
    pub metadata_pending: bool,
    pub stop_pending: bool,
    pub stop_blocked: bool,
    pub emergency_observed: bool,
    pub post_setup_bound: bool,
    pub retired: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finite_drain: Option<HandyHspFiniteDrainStatus>,
}
impl HandyHspNativeStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if let Some(drain) = &self.finite_drain {
            drain.validate()?;
        }
        // Each accepted point has a strictly increasing u32 millisecond time;
        // the remote append tail is a signed i32 index. The owner can retain at
        // most 4096 pending, in-flight and conservatively buffered points.
        if self.accepted_samples > u64::from(u32::MAX) + 1
            || self.total_added > i32::MAX as u64 + 1
            || self.total_added > self.accepted_samples
            || self.buffered_points > 4096
            || self.pending_points > self.buffered_points
            || self.retained_points > self.buffered_points
            || self.pending_points + self.retained_points > self.buffered_points
            || u64::from(self.buffered_points) > self.accepted_samples
        {
            return Err(ProtocolError::invalid("invalid HSP point counters"));
        }
        if let Some(effect) = self.current_effect {
            effect.validate()?;
        }
        if let Some(joined) = self.last_joined {
            joined.effect.validate()?;
            if self.current_effect.is_some_and(|current| {
                joined.effect.sequence > current.sequence
                    || (joined.effect.sequence == current.sequence
                        && joined.effect.kind != current.kind)
            }) {
                return Err(ProtocolError::invalid(
                    "conflicting HSP effect observations",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspStatus {
    pub project_id: ProjectId,
    pub stream_request_id: RequestId,
    pub stream_record_id: ArtifactId,
    pub generation: u64,
    pub stream_id: u32,
    pub live: LiveIdentity,
    /// None means only retained history is available. If neither native nor
    /// retained history exists, the Engine returns NotFound instead of a DTO.
    pub native: Option<HandyHspNativeStatus>,
    /// Always false until independent HSP qualification is implemented and met.
    /// This is not a claim that the device did not move.
    pub qualified: bool,
    /// True only after the exact retrospective emergency observation is durable.
    /// This is history, not a prewrite claim or proof of physical stopping.
    pub emergency_history_durable: bool,
}
impl HandyHspStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.stream_record_id.as_str())?;
        if self.generation == 0
            || self.stream_id == 0
            || self.project_id != self.live.project
            || self.qualified
            || (self.emergency_history_durable
                && self
                    .native
                    .as_ref()
                    .is_some_and(|native| !native.emergency_observed))
        {
            return Err(ProtocolError::invalid("invalid HSP stream observation"));
        }
        if let Some(native) = &self.native {
            native.validate()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> HandyHspStatus {
        HandyHspStatus {
            project_id: ProjectId::new("project").unwrap(),
            stream_request_id: RequestId::new("original-stream").unwrap(),
            stream_record_id: ArtifactId::new("a".repeat(64)).unwrap(),
            generation: 1,
            stream_id: 9,
            live: LiveIdentity {
                epoch: AttemptId::new("capture-epoch").unwrap(),
                project: ProjectId::new("project").unwrap(),
                revision: RevisionId::new(1),
                source: SourceVersionId::new("source-version").unwrap(),
            },
            native: Some(HandyHspNativeStatus {
                runtime_state: HandyHspRuntimeState::StopRequired,
                native_active: true,
                current_effect: Some(HandyHspEffectStatus {
                    sequence: 5,
                    kind: HandyHspEffectKind::Stop,
                }),
                accepted_samples: 4,
                pending_points: 0,
                retained_points: 2,
                buffered_points: 4,
                total_added: 2,
                last_joined: Some(HandyHspJoinedObservation {
                    effect: HandyHspEffectStatus {
                        sequence: 4,
                        kind: HandyHspEffectKind::Append,
                    },
                    outcome: HandyHspJoinedOutcome::Unknown,
                }),
                metadata_pending: true,
                stop_pending: true,
                stop_blocked: false,
                emergency_observed: false,
                post_setup_bound: true,
                retired: true,
                finite_drain: None,
            }),
            qualified: false,
            emergency_history_durable: false,
        }
    }

    #[test]
    fn handy_hsp_status_is_project_scoped_read_only_revision_free_and_redacted() {
        let mut request = Request::new(
            RequestId::new("read-stream").unwrap(),
            Command::HandyHspStatus {
                stream_request_id: RequestId::new("original-stream").unwrap(),
                owner_proof: "synthetic-hsp-owner-proof".into(),
            },
        );
        request.auth_token = Some("synthetic-auth-token".into());
        assert!(request.command.requires_project());
        assert!(!request.command.requires_revision());
        assert!(!request.command.is_mutating());
        assert!(request.validate().is_err());
        request.project = Some(ProjectId::new("project").unwrap());
        request.validate().unwrap();
        for debug in [format!("{:?}", request), format!("{:?}", request.command)] {
            assert!(!debug.contains("synthetic-hsp-owner-proof"));
            assert!(!debug.contains("synthetic-auth-token"));
        }
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["command"]["operation"], "handy_hsp_status");
        let decoded: Request = serde_json::from_value(json).unwrap();
        decoded.validate().unwrap();
        assert!(
            matches!(decoded.command, Command::HandyHspStatus {stream_request_id, ..}
            if stream_request_id.as_str() == "original-stream")
        );
        for proof in [
            String::new(),
            " ".into(),
            "a".repeat(65),
            "bad\nproof".into(),
        ] {
            request.command = Command::HandyHspStatus {
                stream_request_id: RequestId::new("original-stream").unwrap(),
                owner_proof: proof,
            };
            assert!(request.validate().is_err());
        }
    }

    #[test]
    fn handy_hsp_status_preserves_uncertainty_pending_metadata_and_absent_native_owner() {
        for outcome in [
            HandyHspJoinedOutcome::Acknowledged,
            HandyHspJoinedOutcome::Unknown,
            HandyHspJoinedOutcome::Rejected,
        ] {
            for metadata_pending in [false, true] {
                let mut expected = report();
                let native = expected.native.as_mut().unwrap();
                native.last_joined.as_mut().unwrap().outcome = outcome;
                native.metadata_pending = metadata_pending;
                expected.validate().unwrap();
                let wire = serde_json::to_value(ResponseBody::HandyHsp(Box::new(expected.clone())))
                    .unwrap();
                assert_eq!(wire["kind"], "handy_hsp");
                let ResponseBody::HandyHsp(decoded) = serde_json::from_value(wire).unwrap() else {
                    panic!("HSP status response variant changed");
                };
                decoded.validate().unwrap();
                assert_eq!(*decoded, expected);
            }
        }
        let mut historical = report();
        historical.native = None;
        historical.emergency_history_durable = true;
        historical.validate().unwrap();
        let decoded: HandyHspStatus =
            serde_json::from_value(serde_json::to_value(&historical).unwrap()).unwrap();
        assert_eq!(decoded, historical);
    }

    fn finite_report() -> HandyHspFiniteDrainStatus {
        HandyHspFiniteDrainStatus {
            input_state: HandyHspInputState::TailReached,
            terminal_time_ms: 200,
            terminal_position_percent: 20,
            terminal_source_sequence: 3,
            captured_at_ns: 100,
            accepted_at_ns: 120,
            closed_at_boottime_ns: 110,
            expires_at_boottime_ns: 1000,
            tail_reached_at_boottime_ns: Some(300),
            stop_dispatch_deadline_boottime_ns: Some(400),
            stop_dispatched_at_boottime_ns: Some(350),
            stop_deadline_missed: false,
            aborted: false,
        }
    }

    #[test]
    fn finite_hsp_status_preserves_original_input_and_describes_late_stop() {
        let mut expected = report();
        let wire = serde_json::to_value(&expected).unwrap();
        assert!(wire["native"].get("finite_drain").is_none());
        expected.native.as_mut().unwrap().finite_drain = Some(finite_report());
        expected.validate().unwrap();
        let decoded: HandyHspStatus =
            serde_json::from_value(serde_json::to_value(&expected).unwrap()).unwrap();
        assert_eq!(decoded, expected);
        let drain = expected
            .native
            .as_mut()
            .unwrap()
            .finite_drain
            .as_mut()
            .unwrap();
        // An input close may precede the last worker acceptance. Neither one is
        // substituted for the other's original timestamp.
        assert!(drain.closed_at_boottime_ns < drain.accepted_at_ns);
        drain.stop_dispatched_at_boottime_ns = Some(401);
        assert!(drain.validate().is_err());
        drain.stop_deadline_missed = true;
        drain.validate().unwrap();
        assert!(!expected.qualified);
    }

    #[test]
    fn finite_hsp_status_rejects_unpaired_or_retimed_observations() {
        for case in 0..8 {
            let mut drain = finite_report();
            match case {
                0 => drain.captured_at_ns = drain.closed_at_boottime_ns + 1,
                1 => drain.input_state = HandyHspInputState::Draining,
                2 => drain.tail_reached_at_boottime_ns = None,
                3 => drain.stop_dispatch_deadline_boottime_ns = None,
                4 => drain.tail_reached_at_boottime_ns = Some(drain.expires_at_boottime_ns),
                5 => drain.stop_dispatch_deadline_boottime_ns = drain.tail_reached_at_boottime_ns,
                6 => drain.stop_dispatched_at_boottime_ns = Some(299),
                _ => drain.terminal_source_sequence = 0,
            }
            assert!(drain.validate().is_err(), "case {case}");
        }
        let mut aborted = finite_report();
        aborted.input_state = HandyHspInputState::Sealed;
        aborted.tail_reached_at_boottime_ns = None;
        aborted.stop_dispatch_deadline_boottime_ns = None;
        aborted.stop_dispatched_at_boottime_ns = None;
        aborted.aborted = true;
        aborted.validate().unwrap();
    }

    #[test]
    fn handy_hsp_persisted_emergency_remains_history_without_qualification() {
        let mut value = report();
        value.native.as_mut().unwrap().emergency_observed = true;
        value.emergency_history_durable = true;
        value.validate().unwrap();
        assert!(!value.qualified);
        value.qualified = true;
        assert!(value.validate().is_err());
    }

    #[test]
    fn handy_hsp_status_refuses_invalid_subject_bounds_and_conflicting_observations() {
        for case in 0..13 {
            let mut value = report();
            let native = value.native.as_mut().unwrap();
            match case {
                0 => value.generation = 0,
                1 => value.stream_id = 0,
                2 => value.live.project = ProjectId::new("other").unwrap(),
                3 => value.stream_record_id = ArtifactId::new("not-a-digest").unwrap(),
                4 => value.qualified = true,
                5 => value.emergency_history_durable = true,
                6 => native.accepted_samples = u64::from(u32::MAX) + 2,
                7 => native.total_added = 5,
                8 => native.buffered_points = 4097,
                9 => native.pending_points = 5,
                10 => native.current_effect.as_mut().unwrap().sequence = 0,
                11 => native.last_joined.as_mut().unwrap().effect.sequence = 6,
                _ => native.last_joined.as_mut().unwrap().effect.sequence = 5,
            }
            assert!(value.validate().is_err(), "case {case}");
        }
        let mut bounded = report();
        let native = bounded.native.as_mut().unwrap();
        native.accepted_samples = u64::from(u32::MAX) + 1;
        native.total_added = i32::MAX as u64 + 1;
        native.buffered_points = 4096;
        native.pending_points = 4096;
        native.retained_points = 0;
        bounded.validate().unwrap();
        bounded.native.as_mut().unwrap().total_added += 1;
        assert!(bounded.validate().is_err());
    }

    #[test]
    fn handy_hsp_status_keeps_native_unsent_setup_distinct_from_durable_cancellation() {
        for (state, expected_name) in [
            (HandyHspRuntimeState::SetupUnsent, "setup_unsent"),
            (HandyHspRuntimeState::SetupCancelled, "setup_cancelled"),
        ] {
            let mut value = report();
            let native = value.native.as_mut().unwrap();
            native.runtime_state = state;
            native.native_active = false;
            native.current_effect = None;
            native.accepted_samples = 0;
            native.pending_points = 0;
            native.retained_points = 0;
            native.buffered_points = 0;
            native.total_added = 0;
            native.last_joined = Some(HandyHspJoinedObservation {
                effect: HandyHspEffectStatus {
                    sequence: 1,
                    kind: HandyHspEffectKind::Setup,
                },
                outcome: HandyHspJoinedOutcome::Rejected,
            });
            native.metadata_pending = state == HandyHspRuntimeState::SetupUnsent;
            native.stop_pending = false;
            native.post_setup_bound = false;
            value.validate().unwrap();
            let wire = serde_json::to_value(&value).unwrap();
            assert_eq!(wire["native"]["runtime_state"], expected_name);
            let decoded: HandyHspStatus = serde_json::from_value(wire).unwrap();
            decoded.validate().unwrap();
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn handy_hsp_status_refuses_unknown_authority_and_physical_claim_fields() {
        for field in [
            "physical_stopped",
            "physical_playback",
            "owner_proof",
            "lease_until_ns",
        ] {
            let mut wire = serde_json::to_value(report()).unwrap();
            wire[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyHspStatus>(wire).is_err());
            let mut wire = serde_json::to_value(report()).unwrap();
            wire["native"][field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyHspStatus>(wire).is_err());
        }
        let mut wire = serde_json::to_value(report()).unwrap();
        wire["native"]["last_joined"]["late_acknowledgment_overrides_unknown"] =
            serde_json::json!(true);
        assert!(serde_json::from_value::<HandyHspStatus>(wire).is_err());
    }
}
