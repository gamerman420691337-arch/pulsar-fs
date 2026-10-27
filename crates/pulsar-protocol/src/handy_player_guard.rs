//! Browser liveness observations and historical bindings, never playback authority.
use crate::*;

const MAX_BROWSER_INTEGER: u64 = (1u64 << 53) - 1;

/// One browser source selection. These IDs do not identify immutable media bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlayerIdentity {
    pub launch_id: AttemptId,
    pub page_id: AttemptId,
    pub player_id: AttemptId,
    pub source_id: AttemptId,
    pub generation: u64,
    pub time_basis: LiveSourceTimeBasis,
}
impl HandyPlayerIdentity {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.generation > i64::MAX as u64 {
            return Err(ProtocolError::invalid(
                "player generation exceeds its bound",
            ));
        }
        Ok(())
    }
}

/// Explicit software liveness limits; these are not physical stopping bounds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlayerGuardPolicy {
    pub heartbeat_timeout_ms: u16,
    pub media_stall_timeout_ms: u16,
    pub challenge_timeout_ms: u16,
}
impl HandyPlayerGuardPolicy {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !(100..=5000).contains(&self.heartbeat_timeout_ms)
            || !(100..=5000).contains(&self.media_stall_timeout_ms)
            || self.challenge_timeout_ms == 0
            || self.challenge_timeout_ms > self.heartbeat_timeout_ms
        {
            return Err(ProtocolError::invalid("invalid player guard policy"));
        }
        Ok(())
    }
}

/// Browser reports remain structurally valid when stopped, hidden or unsupported
/// for continued playback, so the Engine can retire their exact native binding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlayerSample {
    pub browser_tick_us: u64,
    pub media_time_us: u64,
    pub presented_frames: u64,
    pub playing: bool,
    pub seeking: bool,
    pub ended: bool,
    pub visible: bool,
    pub playback_rate_milli: u32,
}
impl HandyPlayerSample {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.browser_tick_us > MAX_BROWSER_INTEGER
            || self.media_time_us > MAX_BROWSER_INTEGER
            || self.presented_frames > MAX_BROWSER_INTEGER
            || self.playback_rate_milli > 16_000
        {
            return Err(ProtocolError::invalid("invalid player observation bounds"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlayerGuardBinding {
    pub request_id: RequestId,
    pub binding_id: ArtifactId,
    pub selection: HandyContinuousTimingSelection,
    pub player: HandyPlayerIdentity,
    pub policy: HandyPlayerGuardPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_media: Option<BrowserMediaBinding>,
    /// Immutable original Playing controller snapshot, not current authority.
    pub controller: HandyControllerLease,
}
impl HandyPlayerGuardBinding {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.binding_id.as_str())?;
        self.selection.validate()?;
        self.player.validate()?;
        self.policy.validate()?;
        if let Some(binding) = &self.browser_media {
            binding.validate()?;
        }
        self.controller.validate()?;
        if self.request_id == self.selection.acquisition_request_id
            || self.request_id == self.selection.play_request_id
            || self.controller.state != HandyControllerState::Playing
            || self.selection.acquisition_request_id != self.controller.acquisition_request_id
            || self.selection.controller_id != self.controller.controller_id
            || self.selection.expected_generation != self.controller.generation
        {
            return Err(ProtocolError::invalid("invalid player guard binding"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlayerGuardReport {
    pub binding: HandyPlayerGuardBinding,
    /// Native timestamps must not be interpreted as exact JavaScript numbers.
    pub bound_at_boottime_ns: u64,
    pub heartbeat_deadline_boottime_ns: u64,
    pub media_progress_deadline_boottime_ns: u64,
    pub last_sequence: u64,
    pub last_sample: Option<HandyPlayerSample>,
    pub current: bool,
    pub synchronized: bool,
    pub playback_enabled: bool,
    pub physical_stop_pending: bool,
}
impl HandyPlayerGuardReport {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.binding.validate()?;
        if let Some(sample) = &self.last_sample {
            sample.validate()?;
        }
        let contradictory_current = self.current
            && (!self.physical_stop_pending
                || self.last_sample.as_ref().is_some_and(|sample| {
                    !sample.playing
                        || sample.seeking
                        || sample.ended
                        || !sample.visible
                        || sample.playback_rate_milli != 1000
                }));
        if self.synchronized
            || self.playback_enabled
            || contradictory_current
            || self.heartbeat_deadline_boottime_ns <= self.bound_at_boottime_ns
            || self.media_progress_deadline_boottime_ns <= self.bound_at_boottime_ns
            || (self.last_sequence == 0) != self.last_sample.is_none()
        {
            return Err(ProtocolError::invalid("invalid player guard report"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyPlayerChallenge {
    pub binding_id: ArtifactId,
    pub player: HandyPlayerIdentity,
    pub sequence: u64,
    pub challenge_id: ArtifactId,
    pub issued_at_boottime_ns: u64,
    pub expires_at_boottime_ns: u64,
    pub current: bool,
}
impl HandyPlayerChallenge {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.binding_id.as_str())?;
        validate_sha256(self.challenge_id.as_str())?;
        self.player.validate()?;
        if self.sequence == 0 || self.expires_at_boottime_ns <= self.issued_at_boottime_ns {
            return Err(ProtocolError::invalid("invalid player challenge"));
        }
        Ok(())
    }
}

pub(crate) fn validate_request(request: &Request) -> Result<(), ProtocolError> {
    let proof = match &request.command {
        Command::BindHandyPlayerGuard {
            selection,
            player,
            policy,
            browser_media,
            allow_monitoring,
            owner_proof,
        } => {
            selection.validate()?;
            player.validate()?;
            policy.validate()?;
            if let Some(binding) = browser_media {
                binding.validate()?;
            }
            if request.request_id == selection.acquisition_request_id
                || request.request_id == selection.play_request_id
            {
                return Err(ProtocolError::invalid(
                    "player guard binding requires its own request identity",
                ));
            }
            if !allow_monitoring {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "player guard requires explicit monitoring consent",
                ));
            }
            owner_proof
        }
        Command::HandyPlayerGuardStatus { owner_proof, .. } => owner_proof,
        Command::IssueHandyPlayerChallenge {
            binding_id,
            player,
            sequence,
            owner_proof,
        }
        | Command::ObserveHandyPlayer {
            binding_id,
            player,
            sequence,
            owner_proof,
            ..
        } => {
            validate_sha256(binding_id.as_str())?;
            player.validate()?;
            if *sequence == 0 {
                return Err(ProtocolError::invalid("player sequence must be nonzero"));
            }
            if let Command::ObserveHandyPlayer {
                challenge_id,
                sample,
                ..
            } = &request.command
            {
                validate_sha256(challenge_id.as_str())?;
                sample.validate()?;
            }
            owner_proof
        }
        Command::ReleaseHandyPlayerGuard {
            binding_id,
            player,
            owner_proof,
        } => {
            validate_sha256(binding_id.as_str())?;
            player.validate()?;
            owner_proof
        }
        _ => return Ok(()),
    };
    bounded_text(proof, 64, "Handy owner capability")?;
    if !matches!(request.command, Command::BindHandyPlayerGuard { .. })
        && request.expected_revision.is_some()
    {
        return Err(ProtocolError::invalid(
            "player guard status and ephemeral operations require no expected_revision",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handy_player_optional_media_preserves_legacy_and_requires_valid_unit_mapping() {
        let mut b = binding();
        assert!(serde_json::to_value(&b)
            .unwrap()
            .get("browser_media")
            .is_none());
        b.browser_media = Some(crate::browser_media::test_binding());
        b.validate().unwrap();
        let restored: HandyPlayerGuardBinding =
            serde_json::from_value(serde_json::to_value(&b).unwrap()).unwrap();
        assert_eq!(restored.browser_media, b.browser_media);
        let mut r = requests().remove(0);
        if let Command::BindHandyPlayerGuard { browser_media, .. } = &mut r.command {
            *browser_media = b.browser_media.clone();
        }
        r.validate().unwrap();
        b.browser_media
            .as_mut()
            .unwrap()
            .selection
            .placement
            .spec
            .rate_numerator = 2;
        assert!(b.validate().is_err());
        if let Command::BindHandyPlayerGuard { browser_media, .. } = &mut r.command {
            *browser_media = b.browser_media;
        }
        assert!(r.validate().is_err());
    }

    fn player() -> HandyPlayerIdentity {
        HandyPlayerIdentity {
            launch_id: AttemptId::new("launch").unwrap(),
            page_id: AttemptId::new("page").unwrap(),
            player_id: AttemptId::new("player").unwrap(),
            source_id: AttemptId::new("source").unwrap(),
            generation: 1,
            time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
        }
    }

    fn policy() -> HandyPlayerGuardPolicy {
        HandyPlayerGuardPolicy {
            heartbeat_timeout_ms: 1000,
            media_stall_timeout_ms: 1000,
            challenge_timeout_ms: 500,
        }
    }

    fn sample() -> HandyPlayerSample {
        HandyPlayerSample {
            browser_tick_us: 1000,
            media_time_us: 2000,
            presented_frames: 3,
            playing: true,
            seeking: false,
            ended: false,
            visible: true,
            playback_rate_milli: 1000,
        }
    }

    fn binding() -> HandyPlayerGuardBinding {
        let mut controller: HandyControllerReceipt =
            serde_json::from_str(include_str!("../tests/fixtures/handy-controller.json")).unwrap();
        controller.lease.state = HandyControllerState::Playing;
        controller.lease.physical_stop_pending = true;
        HandyPlayerGuardBinding {
            request_id: RequestId::new("guard-bind").unwrap(),
            binding_id: ArtifactId::new("a".repeat(64)).unwrap(),
            selection: HandyContinuousTimingSelection {
                acquisition_request_id: controller.lease.acquisition_request_id.clone(),
                controller_id: controller.lease.controller_id.clone(),
                expected_generation: controller.lease.generation,
                play_request_id: RequestId::new("original-play").unwrap(),
                play_record_id: ArtifactId::new("b".repeat(64)).unwrap(),
            },
            player: player(),
            policy: policy(),
            browser_media: None,
            controller: controller.lease,
        }
    }

    fn report() -> HandyPlayerGuardReport {
        HandyPlayerGuardReport {
            binding: binding(),
            bound_at_boottime_ns: 100,
            heartbeat_deadline_boottime_ns: 1000,
            media_progress_deadline_boottime_ns: 2000,
            last_sequence: 0,
            last_sample: None,
            current: true,
            synchronized: false,
            playback_enabled: false,
            physical_stop_pending: true,
        }
    }

    fn challenge() -> HandyPlayerChallenge {
        HandyPlayerChallenge {
            binding_id: binding().binding_id,
            player: player(),
            sequence: 1,
            challenge_id: ArtifactId::new("c".repeat(64)).unwrap(),
            issued_at_boottime_ns: 100,
            expires_at_boottime_ns: 200,
            current: true,
        }
    }

    fn requests() -> Vec<Request> {
        let b = binding();
        let proof = "synthetic-private-player-owner".to_owned();
        vec![
            Command::BindHandyPlayerGuard {
                selection: b.selection.clone(),
                player: b.player.clone(),
                policy: b.policy.clone(),
                browser_media: None,
                allow_monitoring: true,
                owner_proof: proof.clone(),
            },
            Command::HandyPlayerGuardStatus {
                binding_request_id: b.request_id.clone(),
                owner_proof: proof.clone(),
            },
            Command::IssueHandyPlayerChallenge {
                binding_id: b.binding_id.clone(),
                player: b.player.clone(),
                sequence: 1,
                owner_proof: proof.clone(),
            },
            Command::ObserveHandyPlayer {
                binding_id: b.binding_id.clone(),
                player: b.player.clone(),
                sequence: 1,
                challenge_id: challenge().challenge_id,
                sample: sample(),
                owner_proof: proof.clone(),
            },
            Command::ReleaseHandyPlayerGuard {
                binding_id: b.binding_id.clone(),
                player: b.player.clone(),
                owner_proof: proof,
            },
        ]
        .into_iter()
        .enumerate()
        .map(|(i, command)| {
            Request::new(
                RequestId::new(format!("guard-operation-{i}")).unwrap(),
                command,
            )
            .in_project(
                b.controller.project_id.clone(),
                (i == 0).then_some(b.controller.revision),
            )
        })
        .collect()
    }

    #[test]
    fn handy_player_identity_keeps_exact_source_selection_without_promising_media_bytes() {
        let value = player();
        value.validate().unwrap();
        for case in 0..6 {
            let mut other = value.clone();
            match case {
                0 => other.launch_id = AttemptId::new("other-launch").unwrap(),
                1 => other.page_id = AttemptId::new("other-page").unwrap(),
                2 => other.player_id = AttemptId::new("other-player").unwrap(),
                3 => other.source_id = AttemptId::new("other-source").unwrap(),
                4 => other.generation += 1,
                _ => other.time_basis = LiveSourceTimeBasis::DisplayCapturePresentation,
            }
            other.validate().unwrap();
            assert_ne!(value, other);
        }
        for basis in [
            LiveSourceTimeBasis::ProducerDeclared,
            LiveSourceTimeBasis::SourceMediaPresentation,
            LiveSourceTimeBasis::DisplayCapturePresentation,
            LiveSourceTimeBasis::CameraCapturePresentation,
        ] {
            let mut value = player();
            value.time_basis = basis;
            value.generation = i64::MAX as u64;
            value.validate().unwrap();
            value.generation += 1;
            assert!(value.validate().is_err());
        }
    }

    #[test]
    fn handy_player_policy_checks_independent_software_limits_without_defaults() {
        for (heartbeat, stall, challenge) in [(100, 5000, 1), (5000, 100, 5000)] {
            HandyPlayerGuardPolicy {
                heartbeat_timeout_ms: heartbeat,
                media_stall_timeout_ms: stall,
                challenge_timeout_ms: challenge,
            }
            .validate()
            .unwrap();
        }
        for case in 0..6 {
            let mut value = policy();
            match case {
                0 => value.heartbeat_timeout_ms = 99,
                1 => value.heartbeat_timeout_ms = 5001,
                2 => value.media_stall_timeout_ms = 99,
                3 => value.media_stall_timeout_ms = 5001,
                4 => value.challenge_timeout_ms = 0,
                _ => value.challenge_timeout_ms = value.heartbeat_timeout_ms + 1,
            }
            assert!(value.validate().is_err());
        }
        assert!(serde_json::from_str::<HandyPlayerGuardPolicy>("{}").is_err());
    }

    #[test]
    fn handy_player_samples_preserve_stop_signals_and_exact_browser_integer_bounds() {
        for flags in 0..16 {
            let mut value = sample();
            value.playing = flags & 1 != 0;
            value.seeking = flags & 2 != 0;
            value.ended = flags & 4 != 0;
            value.visible = flags & 8 != 0;
            for rate in [0, 1000, 16_000] {
                value.playback_rate_milli = rate;
                value.browser_tick_us = MAX_BROWSER_INTEGER;
                value.media_time_us = MAX_BROWSER_INTEGER;
                value.presented_frames = MAX_BROWSER_INTEGER;
                value.validate().unwrap();
            }
        }
        for case in 0..4 {
            let mut bad = sample();
            match case {
                0 => bad.browser_tick_us = MAX_BROWSER_INTEGER + 1,
                1 => bad.media_time_us = MAX_BROWSER_INTEGER + 1,
                2 => bad.presented_frames = MAX_BROWSER_INTEGER + 1,
                _ => bad.playback_rate_milli = 16_001,
            }
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn handy_player_binding_requires_original_playing_controller_and_exact_selection() {
        binding().validate().unwrap();
        for case in 0..8 {
            let mut bad = binding();
            match case {
                0 => bad.binding_id = ArtifactId::new("not-a-digest").unwrap(),
                1 => bad.selection.controller_id = ClientId::new("other").unwrap(),
                2 => bad.selection.acquisition_request_id = RequestId::new("other").unwrap(),
                3 => bad.selection.expected_generation += 1,
                4 => {
                    bad.controller.state = HandyControllerState::Active;
                    bad.controller.physical_stop_pending = false;
                    bad.controller.validate().unwrap();
                }
                5 => bad.request_id = bad.selection.acquisition_request_id.clone(),
                6 => bad.request_id = bad.selection.play_request_id.clone(),
                _ => bad.selection.play_record_id = ArtifactId::new("not-a-digest").unwrap(),
            }
            assert!(bad.validate().is_err(), "binding case {case}");
        }
    }

    #[test]
    fn handy_player_report_preserves_native_integer_deadlines_and_cannot_claim_synchronization() {
        report().validate().unwrap();
        let mut retired = report();
        retired.current = false;
        retired.physical_stop_pending = false;
        retired.bound_at_boottime_ns = u64::MAX - 2;
        retired.heartbeat_deadline_boottime_ns = u64::MAX - 1;
        retired.media_progress_deadline_boottime_ns = u64::MAX;
        retired.last_sequence = u64::MAX;
        let mut stopped = sample();
        stopped.playing = false;
        stopped.visible = false;
        stopped.playback_rate_milli = 0;
        retired.last_sample = Some(stopped);
        retired.validate().unwrap();
        let encoded = serde_json::to_vec(&retired).unwrap();
        let decoded: HandyPlayerGuardReport = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.last_sequence, u64::MAX);
        assert_eq!(decoded.media_progress_deadline_boottime_ns, u64::MAX);
        decoded.validate().unwrap();
        for case in 0..7 {
            let mut bad = report();
            match case {
                0 => bad.synchronized = true,
                1 => bad.playback_enabled = true,
                2 => bad.heartbeat_deadline_boottime_ns = bad.bound_at_boottime_ns,
                3 => bad.media_progress_deadline_boottime_ns = bad.bound_at_boottime_ns - 1,
                4 => bad.last_sequence = 1,
                5 => bad.last_sample = Some(sample()),
                _ => {
                    bad.last_sequence = 1;
                    let mut sample = sample();
                    sample.presented_frames = u64::MAX;
                    bad.last_sample = Some(sample);
                }
            }
            assert!(bad.validate().is_err(), "report case {case}");
        }
    }

    #[test]
    fn handy_player_challenge_validates_nonce_and_native_expiry_without_recreating_authority() {
        let value = challenge();
        value.validate().unwrap();
        for case in 0..6 {
            let mut bad = value.clone();
            match case {
                0 => bad.binding_id = ArtifactId::new("not-a-digest").unwrap(),
                1 => bad.challenge_id = ArtifactId::new("not-a-digest").unwrap(),
                2 => bad.sequence = 0,
                3 => bad.expires_at_boottime_ns = bad.issued_at_boottime_ns,
                4 => bad.expires_at_boottime_ns = bad.issued_at_boottime_ns - 1,
                _ => bad.player.generation = u64::MAX,
            }
            assert!(bad.validate().is_err());
        }
        let mut historical = value;
        historical.current = false;
        historical.sequence = u64::MAX;
        historical.issued_at_boottime_ns = u64::MAX - 1;
        historical.expires_at_boottime_ns = u64::MAX;
        historical.validate().unwrap();
    }

    #[test]
    fn handy_player_current_report_rejects_stop_signals_or_missing_physical_stop_custody() {
        for case in 0..7 {
            let mut value = report();
            value.last_sequence = 1;
            let mut observation = sample();
            match case {
                0 => observation.playing = false,
                1 => observation.seeking = true,
                2 => observation.ended = true,
                3 => observation.visible = false,
                4 => observation.playback_rate_milli = 0,
                5 => observation.playback_rate_milli = 16_000,
                _ => value.physical_stop_pending = false,
            }
            observation.validate().unwrap();
            value.last_sample = Some(observation);
            assert!(value.validate().is_err(), "forged current case {case}");
            value.current = false;
            value.validate().unwrap();
        }
        let mut initial = report();
        initial.physical_stop_pending = false;
        assert!(initial.validate().is_err());
        initial.current = false;
        initial.validate().unwrap();
    }

    #[test]
    fn handy_player_commands_are_scoped_revision_separated_and_owner_debug_redacted() {
        for (i, request) in requests().into_iter().enumerate() {
            request.validate().unwrap();
            assert!(request.command.requires_project());
            assert_eq!(request.command.requires_revision(), i == 0);
            assert_eq!(request.command.is_mutating(), i != 1);
            for debug in [format!("{request:?}"), format!("{:?}", request.command)] {
                assert!(!debug.contains("synthetic-private-player-owner"));
            }
            let encoded = serde_json::to_value(&request).unwrap();
            let roundtrip: Request = serde_json::from_value(encoded.clone()).unwrap();
            roundtrip.validate().unwrap();
            assert_eq!(serde_json::to_value(&roundtrip).unwrap(), encoded);
            let mut wrong_scope = request.clone();
            wrong_scope.project = None;
            assert!(wrong_scope.validate().is_err());
            let mut wrong_revision = request;
            wrong_revision.expected_revision = if i == 0 {
                None
            } else {
                Some(binding().controller.revision)
            };
            assert!(wrong_revision.validate().is_err());
        }
    }

    #[test]
    fn handy_player_bind_requires_consent_and_distinct_original_request() {
        let request = requests().remove(0);
        for case in 0..3 {
            let mut bad = request.clone();
            let Command::BindHandyPlayerGuard {
                selection,
                allow_monitoring,
                ..
            } = &mut bad.command
            else {
                panic!();
            };
            match case {
                0 => *allow_monitoring = false,
                1 => bad.request_id = selection.acquisition_request_id.clone(),
                _ => bad.request_id = selection.play_request_id.clone(),
            }
            let error = bad.validate().unwrap_err();
            if case == 0 {
                assert_eq!(error.code, ErrorCode::Forbidden);
            }
        }
    }

    #[test]
    fn handy_player_fast_commands_validate_hashes_and_sequence_but_preserve_stopped_samples() {
        let mut observation = requests().remove(3);
        if let Command::ObserveHandyPlayer { sample, .. } = &mut observation.command {
            sample.playing = false;
            sample.seeking = true;
            sample.ended = true;
            sample.visible = false;
            sample.playback_rate_milli = 0;
        }
        observation.validate().unwrap();
        for request in requests().into_iter().skip(2) {
            let mut bad = request.clone();
            match &mut bad.command {
                Command::IssueHandyPlayerChallenge { binding_id, .. }
                | Command::ObserveHandyPlayer { binding_id, .. }
                | Command::ReleaseHandyPlayerGuard { binding_id, .. } => {
                    *binding_id = ArtifactId::new("not-a-digest").unwrap();
                }
                _ => panic!(),
            }
            assert!(bad.validate().is_err());
            let mut bad = request;
            match &mut bad.command {
                Command::IssueHandyPlayerChallenge { sequence, .. }
                | Command::ObserveHandyPlayer { sequence, .. } => *sequence = 0,
                Command::ReleaseHandyPlayerGuard { player, .. } => player.generation = u64::MAX,
                _ => panic!(),
            }
            assert!(bad.validate().is_err());
        }
        if let Command::ObserveHandyPlayer { challenge_id, .. } = &mut observation.command {
            *challenge_id = ArtifactId::new("not-a-digest").unwrap();
        }
        assert!(observation.validate().is_err());
    }

    #[test]
    fn handy_player_all_commands_bound_owner_proofs() {
        for request in requests() {
            for proof in [String::new(), "x".repeat(65), "invalid\nproof".into()] {
                let mut bad = request.clone();
                match &mut bad.command {
                    Command::BindHandyPlayerGuard { owner_proof, .. }
                    | Command::HandyPlayerGuardStatus { owner_proof, .. }
                    | Command::IssueHandyPlayerChallenge { owner_proof, .. }
                    | Command::ObserveHandyPlayer { owner_proof, .. }
                    | Command::ReleaseHandyPlayerGuard { owner_proof, .. } => *owner_proof = proof,
                    _ => panic!(),
                }
                assert!(bad.validate().is_err());
            }
        }
    }

    #[test]
    fn handy_player_wire_responses_preserve_history_and_refuse_invented_authority_fields() {
        for (kind, value) in [
            (
                "handy_player_guard",
                ResponseBody::HandyPlayerGuard(Box::new(report())),
            ),
            (
                "handy_player_challenge",
                ResponseBody::HandyPlayerChallenge(Box::new(challenge())),
            ),
        ] {
            let wire = serde_json::to_value(value).unwrap();
            assert_eq!(wire["kind"], kind);
            let roundtrip: ResponseBody = serde_json::from_value(wire).unwrap();
            match roundtrip {
                ResponseBody::HandyPlayerGuard(report) => report.validate().unwrap(),
                ResponseBody::HandyPlayerChallenge(challenge) => challenge.validate().unwrap(),
                _ => panic!(),
            }
        }
        for field in ["authority_restored", "display_verified", "physical_stopped"] {
            let mut wire = serde_json::to_value(report()).unwrap();
            wire[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyPlayerGuardReport>(wire).is_err());
            let mut wire = serde_json::to_value(challenge()).unwrap();
            wire[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyPlayerChallenge>(wire).is_err());
        }
        let mut wire = serde_json::to_value(sample()).unwrap();
        wire["browser_tick_us"] = serde_json::json!(-1);
        assert!(serde_json::from_value::<HandyPlayerSample>(wire).is_err());
        let mut wire = serde_json::to_value(player()).unwrap();
        wire["time_basis"] = serde_json::json!("qualified_display_time");
        assert!(serde_json::from_value::<HandyPlayerIdentity>(wire).is_err());
    }
}
