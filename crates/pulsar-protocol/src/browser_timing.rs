//! Browser execution brackets and declared media phase; no display or device authority.
use crate::*;

const NS_PER_MS: u64 = 1_000_000;

fn invalid(message: &str) -> ProtocolError {
    ProtocolError::invalid(format!("browser timing: {message}"))
}
fn core_error(error: CoreError) -> ProtocolError {
    invalid(&error.to_string())
}
fn validate_player(player: &HandyPlayerIdentity) -> Result<(), ProtocolError> {
    player.validate()?;
    if player.time_basis != LiveSourceTimeBasis::SourceMediaPresentation {
        return Err(invalid(
            "timing requires the source media presentation basis",
        ));
    }
    Ok(())
}

/// Software expiry policy, not measured physical accuracy or stopping bounds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTimingPolicy {
    pub probe_timeout_ms: u16,
    pub observation_timeout_ms: u16,
}
impl BrowserTimingPolicy {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if !(50..=2000).contains(&self.probe_timeout_ms)
            || !(100..=5000).contains(&self.observation_timeout_ms)
            || self.probe_timeout_ms > self.observation_timeout_ms
        {
            return Err(invalid("invalid expiry policy"));
        }
        Ok(())
    }
}

/// One callback's atomic producer observation. `sample.browser_tick_us` is the
/// performance.now() sample inside that callback; `callback_tick_us` is its rVFC
/// argument. Composition submission and predicted display stay separate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTimingFrame {
    pub sample: HandyPlayerSample,
    pub callback_tick_us: u64,
    pub presentation_time_us: Option<u64>,
    pub expected_display_time_us: Option<u64>,
}
impl BrowserTimingFrame {
    /// Stopped/hidden observations remain structurally representable so the
    /// native owner can retire their exact binding instead of losing the event.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.sample.validate()?;
        if self.callback_tick_us > self.sample.browser_tick_us
            || self
                .presentation_time_us
                .is_some_and(|t| t > BROWSER_MEDIA_MAX_EXACT_INTEGER)
            || self
                .expected_display_time_us
                .is_some_and(|t| t > BROWSER_MEDIA_MAX_EXACT_INTEGER)
        {
            return Err(invalid("invalid callback timestamp bounds"));
        }
        Ok(())
    }
    /// Eligibility only; this never proves nonce freshness, origin or display.
    pub fn is_current(&self) -> bool {
        self.validate().is_ok()
            && self.sample.playing
            && !self.sample.seeking
            && !self.sample.ended
            && self.sample.visible
            && self.sample.playback_rate_milli == 1000
            && self.sample.presented_frames != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTimingBinding {
    pub request_id: RequestId,
    pub binding_id: ArtifactId,
    pub engine_epoch: AttemptId,
    pub project_id: ProjectId,
    pub revision: RevisionId,
    pub player: HandyPlayerIdentity,
    pub browser_media: BrowserMediaBinding,
    pub policy: BrowserTimingPolicy,
}
impl BrowserTimingBinding {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_player(&self.player)?;
        self.browser_media.validate()?;
        self.policy.validate()?;
        if self.engine_epoch != self.browser_media.engine_epoch
            || self.project_id != self.browser_media.project_id
            || self.revision != self.browser_media.revision
            || self.request_id == self.browser_media.preparation_request_id
        {
            return Err(invalid(
                "binding differs from its media subject or original request",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTimingProbe {
    pub binding_id: ArtifactId,
    pub player: HandyPlayerIdentity,
    pub sequence: u64,
    pub probe_id: ArtifactId,
    pub issued_at_boottime_ns: u64,
    pub expires_at_boottime_ns: u64,
}
impl BrowserTimingProbe {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_player(&self.player)?;
        let duration = self
            .expires_at_boottime_ns
            .checked_sub(self.issued_at_boottime_ns)
            .ok_or_else(|| invalid("reversed probe lifetime"))?;
        if self.sequence == 0 || !(50 * NS_PER_MS..=2000 * NS_PER_MS).contains(&duration) {
            return Err(invalid("invalid probe sequence or lifetime"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTimingObservation {
    pub observation_id: ArtifactId,
    pub binding_id: ArtifactId,
    pub sequence: u64,
    pub frame: BrowserTimingFrame,
    pub bracket: BrowserClockBracket,
    pub project_phase: ExactSeconds,
    pub expires_at_boottime_ns: u64,
}
impl BrowserTimingObservation {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.frame.validate()?;
        self.bracket.validate().map_err(core_error)?;
        let lifetime = self
            .expires_at_boottime_ns
            .checked_sub(self.bracket.native_issue_ns)
            .ok_or_else(|| invalid("reversed observation lifetime"))?;
        if self.sequence == 0
            || self.frame.sample.browser_tick_us != self.bracket.browser_sample_us
            || self.bracket.native_receive_ns - self.bracket.native_issue_ns >= 2000 * NS_PER_MS
            || !(100 * NS_PER_MS..=5000 * NS_PER_MS).contains(&lifetime)
            || self.bracket.native_receive_ns >= self.expires_at_boottime_ns
        {
            return Err(invalid(
                "inconsistent observation bracket, sample or expiry",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTimingReport {
    pub binding: BrowserTimingBinding,
    pub current: bool,
    pub observation: Option<BrowserTimingObservation>,
    pub qualified: bool,
}
impl BrowserTimingReport {
    /// Structural correspondence is checked even for historical observations.
    /// Only the Engine's live registry can decide whether a report is current.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.binding.validate()?;
        if self.qualified {
            return Err(invalid("this observation is not qualified"));
        }
        if let Some(observation) = &self.observation {
            observation.validate()?;
            let media = &self.binding.browser_media;
            let tick = i64::try_from(observation.frame.sample.media_time_us)
                .map_err(|_| invalid("media timestamp overflow"))?;
            let time = SourceTimestamp::new(tick, 1_000_000).map_err(core_error)?;
            let phase = media
                .clock
                .project_at(time, &media.selection.placement.spec)
                .map_err(core_error)?;
            let expected_expiry = observation
                .bracket
                .native_issue_ns
                .checked_add(u64::from(self.binding.policy.observation_timeout_ms) * NS_PER_MS)
                .ok_or_else(|| invalid("observation expiry overflow"))?;
            if observation.binding_id != self.binding.binding_id
                || phase != observation.project_phase
                || observation.expires_at_boottime_ns != expected_expiry
                || observation.bracket.native_receive_ns - observation.bracket.native_issue_ns
                    >= u64::from(self.binding.policy.probe_timeout_ms) * NS_PER_MS
                || (self.current && !observation.frame.is_current())
            {
                return Err(invalid(
                    "observation differs from its binding, phase or policy",
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_request(request: &Request) -> Result<(), ProtocolError> {
    match &request.command {
        Command::OpenBrowserTiming {
            player,
            browser_media,
            policy,
        }
        | Command::PrepareBrowserTimingSuccessor {
            player,
            browser_media,
            policy,
            ..
        } => {
            validate_player(player)?;
            browser_media.validate()?;
            policy.validate()?;
            if request.project.as_ref() != Some(&browser_media.project_id)
                || request.expected_revision != Some(browser_media.revision)
                || request.request_id == browser_media.preparation_request_id
            {
                return Err(invalid(
                    "open requires its own request and exact media project/revision",
                ));
            }
            return Ok(());
        }
        Command::BrowserTimingOpenStatus {
            original_request_id,
        } => {
            if request.request_id == *original_request_id {
                return Err(invalid("history request must differ from original open"));
            }
        }
        Command::IssueBrowserTimingProbe {
            player, sequence, ..
        }
        | Command::ObserveBrowserTiming {
            player, sequence, ..
        } => {
            validate_player(player)?;
            if *sequence == 0 {
                return Err(invalid("probe sequence must be nonzero"));
            }
            if let Command::ObserveBrowserTiming { frame, .. } = &request.command {
                frame.validate()?;
            }
        }
        Command::BrowserTimingStatus { player, .. }
        | Command::ReleaseBrowserTiming { player, .. } => validate_player(player)?,
        _ => return Ok(()),
    }
    if request.expected_revision.is_some() {
        return Err(invalid(
            "status and ephemeral operations require no expected_revision",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> BrowserTimingBinding {
        let media = crate::browser_media::test_binding();
        BrowserTimingBinding {
            request_id: RequestId::new("timing-open").unwrap(),
            binding_id: ArtifactId::new("timing-binding").unwrap(),
            engine_epoch: media.engine_epoch.clone(),
            project_id: media.project_id.clone(),
            revision: media.revision,
            player: HandyPlayerIdentity {
                launch_id: AttemptId::new("launch").unwrap(),
                page_id: AttemptId::new("page").unwrap(),
                player_id: AttemptId::new("player").unwrap(),
                source_id: AttemptId::new("selection-epoch").unwrap(),
                generation: 1,
                time_basis: LiveSourceTimeBasis::SourceMediaPresentation,
            },
            browser_media: media,
            policy: BrowserTimingPolicy {
                probe_timeout_ms: 200,
                observation_timeout_ms: 1000,
            },
        }
    }
    fn frame() -> BrowserTimingFrame {
        BrowserTimingFrame {
            sample: HandyPlayerSample {
                browser_tick_us: 5000,
                media_time_us: 6_000_000,
                presented_frames: 4,
                playing: true,
                seeking: false,
                ended: false,
                visible: true,
                playback_rate_milli: 1000,
            },
            callback_tick_us: 4999,
            presentation_time_us: Some(4998),
            expected_display_time_us: Some(5002),
        }
    }
    fn report() -> BrowserTimingReport {
        let binding = binding();
        BrowserTimingReport {
            observation: Some(BrowserTimingObservation {
                observation_id: ArtifactId::new("observation").unwrap(),
                binding_id: binding.binding_id.clone(),
                sequence: 1,
                frame: frame(),
                bracket: BrowserClockBracket {
                    native_issue_ns: 1_000_000_000,
                    native_receive_ns: 1_001_000_000,
                    browser_sample_us: 5000,
                },
                project_phase: ExactSeconds::new(1, 1).unwrap(),
                expires_at_boottime_ns: 2_000_000_000,
            }),
            binding,
            current: true,
            qualified: false,
        }
    }

    #[test]
    fn policy_bounds_are_explicit_and_probe_never_outlives_observation() {
        for (probe, observation, valid) in [
            (50, 100, true),
            (2000, 5000, true),
            (2000, 2000, true),
            (49, 100, false),
            (2001, 5000, false),
            (50, 99, false),
            (50, 5001, false),
            (200, 100, false),
        ] {
            assert_eq!(
                BrowserTimingPolicy {
                    probe_timeout_ms: probe,
                    observation_timeout_ms: observation
                }
                .validate()
                .is_ok(),
                valid
            );
        }
        assert!(serde_json::from_str::<BrowserTimingPolicy>("{}").is_err());
    }

    #[test]
    fn stopped_hidden_and_zero_progress_frames_are_structural_but_not_current() {
        for mutation in 0..6 {
            let mut r = report();
            let s = &mut r.observation.as_mut().unwrap().frame.sample;
            match mutation {
                0 => s.playing = false,
                1 => s.visible = false,
                2 => s.seeking = true,
                3 => s.ended = true,
                4 => s.playback_rate_milli = 500,
                _ => s.presented_frames = 0,
            }
            r.observation.as_ref().unwrap().frame.validate().unwrap();
            assert!(!r.observation.as_ref().unwrap().frame.is_current());
            assert!(r.validate().is_err());
            r.current = false;
            r.validate().unwrap();
            let request = Request::new(
                RequestId::new("observe").unwrap(),
                Command::ObserveBrowserTiming {
                    binding_id: r.binding.binding_id.clone(),
                    player: r.binding.player.clone(),
                    sequence: 1,
                    probe_id: ArtifactId::new("probe").unwrap(),
                    frame: r.observation.unwrap().frame,
                },
            )
            .in_project(r.binding.project_id, None);
            request.validate().unwrap();
        }
    }

    #[test]
    fn frame_timestamp_domains_remain_distinct_and_bounds_refuse() {
        let f = frame();
        f.validate().unwrap();
        assert!(f.is_current());
        for mutation in 0..5 {
            let mut bad = f.clone();
            match mutation {
                0 => bad.callback_tick_us = bad.sample.browser_tick_us + 1,
                1 => bad.presentation_time_us = Some(BROWSER_MEDIA_MAX_EXACT_INTEGER + 1),
                2 => bad.expected_display_time_us = Some(BROWSER_MEDIA_MAX_EXACT_INTEGER + 1),
                3 => bad.sample.media_time_us = BROWSER_MEDIA_MAX_EXACT_INTEGER + 1,
                _ => bad.sample.browser_tick_us = BROWSER_MEDIA_MAX_EXACT_INTEGER + 1,
            }
            assert!(bad.validate().is_err());
        }
        let mut omitted = f;
        omitted.presentation_time_us = None;
        omitted.expected_display_time_us = None;
        omitted.validate().unwrap();
    }

    #[test]
    fn exact_binding_phase_trim_and_issue_anchored_expiry_are_required() {
        report().validate().unwrap();
        for mutation in 0..13 {
            let mut r = report();
            match mutation {
                0 => r.binding.engine_epoch = AttemptId::new("other").unwrap(),
                1 => r.binding.project_id = ProjectId::new("other").unwrap(),
                2 => r.binding.revision = RevisionId::new(3),
                3 => r.binding.request_id = r.binding.browser_media.preparation_request_id.clone(),
                4 => r.observation.as_mut().unwrap().binding_id = ArtifactId::new("other").unwrap(),
                5 => r.observation.as_mut().unwrap().bracket.browser_sample_us += 1,
                6 => {
                    r.observation.as_mut().unwrap().project_phase = ExactSeconds::new(2, 1).unwrap()
                }
                7 => r.observation.as_mut().unwrap().frame.sample.media_time_us = 15_000_000,
                8 => r.observation.as_mut().unwrap().expires_at_boottime_ns += 1_000_000,
                9 => r.observation.as_mut().unwrap().bracket.native_receive_ns = 1_200_000_000,
                10 => r.qualified = true,
                11 => r.observation.as_mut().unwrap().sequence = 0,
                _ => r.binding.player.time_basis = LiveSourceTimeBasis::ProducerDeclared,
            }
            assert!(r.validate().is_err(), "mutation {mutation}");
            r.current = false;
            assert!(r.validate().is_err(), "history mutation {mutation}");
        }
    }

    #[test]
    fn report_checks_declared_elapsed_trim_at_both_half_open_boundaries() {
        let mut r = report();
        // Browser [5s,15s) maps to source elapsed [0s,10s).
        let o = r.observation.as_mut().unwrap();
        o.frame.sample.media_time_us = 5_000_000;
        o.project_phase = ExactSeconds::new(0, 1).unwrap();
        r.validate().unwrap();
        for (media_time, phase) in [
            (4_999_999, ExactSeconds::new(-1, 1_000_000).unwrap()),
            (15_000_000, ExactSeconds::new(10, 1).unwrap()),
        ] {
            let mut bad = r.clone();
            let o = bad.observation.as_mut().unwrap();
            o.frame.sample.media_time_us = media_time;
            o.project_phase = phase;
            assert!(bad.validate().is_err());
            bad.current = false;
            assert!(bad.validate().is_err());
        }
    }

    #[test]
    fn probe_and_observation_native_reversal_or_overflow_refuse() {
        let b = binding();
        let mut p = BrowserTimingProbe {
            binding_id: b.binding_id,
            player: b.player,
            sequence: 1,
            probe_id: ArtifactId::new("probe").unwrap(),
            issued_at_boottime_ns: 100,
            expires_at_boottime_ns: 50_000_100,
        };
        p.validate().unwrap();
        p.expires_at_boottime_ns -= 1;
        assert!(p.validate().is_err());
        p.expires_at_boottime_ns = 99;
        assert!(p.validate().is_err());
        let mut r = report();
        r.observation.as_mut().unwrap().bracket.native_receive_ns = 999_999_999;
        assert!(r.validate().is_err());
        r = report();
        let o = r.observation.as_mut().unwrap();
        o.bracket.native_issue_ns = u64::MAX - 10;
        o.bracket.native_receive_ns = u64::MAX - 5;
        o.expires_at_boottime_ns = u64::MAX;
        assert!(r.validate().is_err());
    }

    #[test]
    fn timing_commands_require_exact_scope_and_only_open_has_revision() {
        let b = binding();
        let commands = [
            Command::OpenBrowserTiming {
                player: b.player.clone(),
                browser_media: b.browser_media.clone(),
                policy: b.policy.clone(),
            },
            Command::BrowserTimingOpenStatus {
                original_request_id: b.request_id.clone(),
            },
            Command::IssueBrowserTimingProbe {
                binding_id: b.binding_id.clone(),
                player: b.player.clone(),
                sequence: 1,
            },
            Command::ObserveBrowserTiming {
                binding_id: b.binding_id.clone(),
                player: b.player.clone(),
                sequence: 1,
                probe_id: ArtifactId::new("probe").unwrap(),
                frame: frame(),
            },
            Command::BrowserTimingStatus {
                binding_id: b.binding_id.clone(),
                player: b.player.clone(),
            },
            Command::ReleaseBrowserTiming {
                binding_id: b.binding_id.clone(),
                player: b.player.clone(),
            },
        ];
        for (i, command) in commands.into_iter().enumerate() {
            assert!(command.requires_project());
            assert_eq!(command.requires_revision(), i == 0);
            assert_eq!(command.is_mutating(), i != 1 && i != 4);
            let request = Request::new(RequestId::new("operation").unwrap(), command)
                .in_project(b.project_id.clone(), (i == 0).then_some(b.revision));
            request.validate().unwrap();
            let mut bad = request.clone();
            bad.project = None;
            assert!(bad.validate().is_err());
            bad = request.clone();
            bad.expected_revision = if i == 0 { None } else { Some(b.revision) };
            assert!(bad.validate().is_err());
            if i == 0 {
                bad = request.clone();
                bad.project = Some(ProjectId::new("other").unwrap());
                assert!(bad.validate().is_err());
                bad = request;
                bad.request_id = b.browser_media.preparation_request_id.clone();
                assert!(bad.validate().is_err());
            }
        }
        let reused = Request::new(
            b.request_id.clone(),
            Command::BrowserTimingOpenStatus {
                original_request_id: b.request_id,
            },
        )
        .in_project(b.project_id, None);
        assert!(reused.validate().is_err());
    }

    #[test]
    fn reports_roundtrip_as_metadata_without_extra_authority_fields() {
        let report = report();
        let body = ResponseBody::BrowserTiming(Box::new(report.clone()));
        let restored: ResponseBody =
            serde_json::from_value(serde_json::to_value(body).unwrap()).unwrap();
        let ResponseBody::BrowserTiming(restored) = restored else {
            panic!("wrong response kind")
        };
        assert_eq!(*restored, report);
        restored.validate().unwrap();
        for key in ["displayed_at_ns", "playback_enabled", "owner_proof"] {
            let mut value = serde_json::to_value(&report).unwrap();
            value[key] = serde_json::json!(true);
            assert!(serde_json::from_value::<BrowserTimingReport>(value).is_err());
        }
        let mut historical = report;
        historical.current = false;
        historical.validate().unwrap();
        historical.observation = None;
        historical.validate().unwrap();
        historical.current = true;
        historical.validate().unwrap(); // Newly opened native binding, no observation yet.
    }
    #[test]
    fn timing_successor_command_requires_exact_media_scope_and_preserves_existing_report_wire_shape(
    ) {
        let b = binding();
        let r = Request::new(
            RequestId::new("timing-successor").unwrap(),
            Command::PrepareBrowserTimingSuccessor {
                predecessor_binding_id: b.binding_id.clone(),
                player: b.player.clone(),
                browser_media: b.browser_media.clone(),
                policy: b.policy.clone(),
            },
        )
        .in_project(b.project_id.clone(), Some(b.revision));
        r.validate().unwrap();
        assert!(r.command.is_mutating());
        assert!(r.command.requires_revision());
        let copy: Request = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
        copy.validate().unwrap();
        for case in 0..4 {
            let mut bad = r.clone();
            match case {
                0 => bad.expected_revision = None,
                1 => bad.request_id = b.browser_media.preparation_request_id.clone(),
                2 => {
                    if let Command::PrepareBrowserTimingSuccessor { player, .. } = &mut bad.command
                    {
                        player.time_basis = LiveSourceTimeBasis::DisplayCapturePresentation;
                    }
                }
                _ => {
                    if let Command::PrepareBrowserTimingSuccessor { policy, .. } = &mut bad.command
                    {
                        policy.observation_timeout_ms = 0;
                    }
                }
            }
            assert!(bad.validate().is_err());
        }
        let original = report();
        let wire = serde_json::to_value(&original).unwrap();
        assert!(wire["binding"].get("predecessor_binding_id").is_none());
        let restored: BrowserTimingReport = serde_json::from_value(wire).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored, original);
    }
}
