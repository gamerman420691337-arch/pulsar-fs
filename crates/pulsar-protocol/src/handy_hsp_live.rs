//! Exact live-source selection and descriptive HSP history. These records do
//! not reconstruct native ownership or prove model quality or physical motion.
use crate::*;

fn validate_error(error: &Option<ProtocolError>) -> Result<(), ProtocolError> {
    if error.as_ref().is_some_and(|error| {
        error.message.is_empty() || error.message.len() > 2048 || error.message.contains('\0')
    }) {
        return Err(ProtocolError::invalid("invalid HSP live error observation"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspLiveSelection {
    pub preparation_request_id: RequestId,
    pub capture_epoch: AttemptId,
    pub inference_attempt_id: AttemptId,
    /// Opaque identity of an existing native browser timing binding.
    pub browser_timing: ArtifactId,
    pub source_qualification: HandyHspSourceQualificationBinding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finite_drain_policy_sha256: Option<String>,
}
impl HandyHspLiveSelection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.source_qualification.validate()?;
        if let Some(policy) = &self.finite_drain_policy_sha256 {
            validate_sha256(policy)?;
        }
        if self.capture_epoch == self.inference_attempt_id {
            return Err(ProtocolError::invalid(
                "HSP live capture and inference attempt identities must be distinct",
            ));
        }
        Ok(())
    }
}

/// The exact first accepted source observation retained before enqueue. Its
/// original position is evidence, never an invented starting or holding point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspLiveOrigin {
    pub stream_record_id: ArtifactId,
    pub frame: FrameId,
    pub source_time: SourceTimestamp,
    pub source_sequence: u64,
    pub first_position: NormalizedPosition,
    pub capture_earliest_boottime_ns: u64,
    pub capture_latest_boottime_ns: u64,
    pub accepted_earliest_boottime_ns: u64,
    pub accepted_latest_boottime_ns: u64,
    pub generation: u64,
    pub stream_id: u32,
}
impl HandyHspLiveOrigin {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.stream_record_id.as_str())?;
        if self.source_sequence == 0
            || self.generation == 0
            || self.stream_id == 0
            || self.capture_earliest_boottime_ns > self.capture_latest_boottime_ns
            || self.accepted_earliest_boottime_ns > self.accepted_latest_boottime_ns
            || self.accepted_earliest_boottime_ns < self.capture_earliest_boottime_ns
            || self.accepted_latest_boottime_ns < self.capture_latest_boottime_ns
        {
            return Err(ProtocolError::invalid("invalid HSP live source origin"));
        }
        Ok(())
    }
}

/// Descriptive current source cursor and its most recent native transfer.
/// None in legacy mode. Neither this value nor an original history can create
/// a capture, media/timing capability or source-transition token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspLiveActiveSource {
    pub generation: u64,
    pub capture_epoch: AttemptId,
    pub inference_attempt_id: AttemptId,
    pub browser_timing: ArtifactId,
    pub media_preparation_request_id: RequestId,
    pub predecessor_capture_epoch: Option<AttemptId>,
    pub last_transfer_request_id: Option<RequestId>,
    pub pending_successor_epoch: Option<AttemptId>,
    pub pending_transfer_request_id: Option<RequestId>,
}
impl HandyHspLiveActiveSource {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.generation == 0
            || self.capture_epoch == self.inference_attempt_id
            || self.predecessor_capture_epoch.as_ref() == Some(&self.capture_epoch)
            || self.pending_successor_epoch.as_ref() == Some(&self.capture_epoch)
            || self.predecessor_capture_epoch.is_some() != self.last_transfer_request_id.is_some()
            || (self.generation == 1) != self.predecessor_capture_epoch.is_none()
            || (self.pending_transfer_request_id.is_some()
                && self.pending_successor_epoch.is_none())
            || self
                .last_transfer_request_id
                .as_ref()
                .is_some_and(|id| self.pending_transfer_request_id.as_ref() == Some(id))
        {
            return Err(ProtocolError::invalid(
                "invalid active HSP source description",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspLiveNativeStatus {
    pub retired: bool,
    pub original_play_request_id: Option<RequestId>,
    pub original_stop_request_id: Option<RequestId>,
    pub delivered_samples: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_source: Option<HandyHspLiveActiveSource>,
    pub error: Option<ProtocolError>,
}
impl HandyHspLiveNativeStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_error(&self.error)?;
        if let Some(source) = &self.active_source {
            source.validate()?;
            if source.generation > self.delivered_samples {
                return Err(ProtocolError::invalid(
                    "active source generation exceeds delivered originals",
                ));
            }
        }
        if self.delivered_samples > u64::from(u32::MAX) + 1
            || self
                .original_play_request_id
                .as_ref()
                .is_some_and(|play| self.original_stop_request_id.as_ref() == Some(play))
        {
            return Err(ProtocolError::invalid(
                "invalid HSP live native observation",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyHspLiveStatus {
    pub project_id: ProjectId,
    pub stream_request_id: RequestId,
    pub selection: HandyHspLiveSelection,
    /// Both witnesses are absent for the original admission reservation. Once
    /// retained, they remain descriptive history after the native owner ends.
    pub source_subject: Option<HandyHspSourceSubject>,
    pub origin: Option<HandyHspLiveOrigin>,
    pub native: Option<HandyHspLiveNativeStatus>,
    pub stream: Option<HandyHspStatus>,
    /// Retained terminal admission failure or uncertainty, including when no
    /// native owner exists. This never grants retry or replacement authority.
    pub error: Option<ProtocolError>,
    /// Always false; this observation never supplies physical qualification.
    pub qualified: bool,
}
impl HandyHspLiveStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.selection.validate()?;
        validate_error(&self.error)?;
        if self.qualified
            || self.stream_request_id == self.selection.preparation_request_id
            || self.source_subject.is_some() != self.origin.is_some()
        {
            return Err(ProtocolError::invalid("invalid HSP live observation"));
        }
        if let Some(subject) = &self.source_subject {
            subject.validate()?;
            if subject.project_id != self.project_id {
                return Err(ProtocolError::invalid(
                    "HSP live source subject belongs to another project",
                ));
            }
            if self
                .selection
                .finite_drain_policy_sha256
                .as_ref()
                .is_some_and(|selected| {
                    subject.finite_drain_policy_sha256.as_ref() != Some(selected)
                })
            {
                return Err(ProtocolError::invalid(
                    "selected finite HSP drain differs from its source witness",
                ));
            }
        }
        if let Some(origin) = &self.origin {
            origin.validate()?;
        }
        if let Some(native) = &self.native {
            native.validate()?;
            if let Some(active) = &native.active_source {
                if active.generation == 1
                    && (active.capture_epoch != self.selection.capture_epoch
                        || active.inference_attempt_id != self.selection.inference_attempt_id
                        || active.browser_timing != self.selection.browser_timing)
                {
                    return Err(ProtocolError::invalid(
                        "initial active source differs from original selection",
                    ));
                }
            }
            for request in native
                .original_play_request_id
                .iter()
                .chain(native.original_stop_request_id.iter())
            {
                if request == &self.stream_request_id
                    || request == &self.selection.preparation_request_id
                {
                    return Err(ProtocolError::invalid(
                        "HSP live effects require distinct original request identities",
                    ));
                }
            }
        }
        if let Some(stream) = &self.stream {
            stream.validate()?;
            let (Some(subject), Some(origin)) = (&self.source_subject, &self.origin) else {
                return Err(ProtocolError::invalid(
                    "HSP live stream lacks its original source witnesses",
                ));
            };
            if stream.project_id != self.project_id
                || stream.stream_request_id != self.stream_request_id
                || stream.live.epoch != self.selection.capture_epoch
                || stream.live.source != subject.selection.source_version
                || stream.live.revision != subject.revision
                || stream.generation != origin.generation
                || stream.stream_id != origin.stream_id
                || stream.stream_record_id != origin.stream_record_id
            {
                return Err(ProtocolError::invalid(
                    "HSP live stream differs from its original source witnesses",
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_handy_hsp_live_request(request: &Request) -> Result<(), ProtocolError> {
    match &request.command {
        Command::BindHandyHspLive {
            selection,
            acknowledge_current_environment,
            allow_online_control,
            owner_proof,
        } => {
            selection.validate()?;
            bounded_text(owner_proof, 64, "Handy owner capability")?;
            if request.request_id == selection.preparation_request_id {
                return Err(ProtocolError::invalid(
                    "HSP live binding requires its own request identity",
                ));
            }
            if !acknowledge_current_environment {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "HSP live binding requires explicit current environment acknowledgment",
                ));
            }
            if !allow_online_control {
                return Err(ProtocolError::new(
                    ErrorCode::Forbidden,
                    "HSP live binding requires explicit online control consent",
                ));
            }
        }
        Command::PlayHandyHspLive {
            stream_request_id,
            owner_proof,
        }
        | Command::StopHandyHspLive {
            stream_request_id,
            owner_proof,
        } => {
            bounded_text(owner_proof, 64, "Handy owner capability")?;
            if &request.request_id == stream_request_id {
                return Err(ProtocolError::invalid(
                    "HSP live effect requires its own request identity",
                ));
            }
        }
        Command::HandyHspLiveStatus { owner_proof, .. } => {
            bounded_text(owner_proof, 64, "Handy owner capability")?;
        }
        _ => return Ok(()),
    }
    if matches!(
        request.command,
        Command::StopHandyHspLive { .. } | Command::HandyHspLiveStatus { .. }
    ) && request.expected_revision.is_some()
    {
        return Err(ProtocolError::invalid(
            "HSP live Stop and history require no project revision",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selection() -> HandyHspLiveSelection {
        HandyHspLiveSelection {
            preparation_request_id: RequestId::new("prepare").unwrap(),
            capture_epoch: AttemptId::new("capture").unwrap(),
            inference_attempt_id: AttemptId::new("inference").unwrap(),
            browser_timing: ArtifactId::new("opaque-browser-timing").unwrap(),
            finite_drain_policy_sha256: None,
            source_qualification: HandyHspSourceQualificationBinding {
                dossier_id: ArtifactId::new("a".repeat(64)).unwrap(),
                revision: 2,
            },
        }
    }
    fn report() -> HandyHspLiveStatus {
        HandyHspLiveStatus {
            project_id: ProjectId::new("project").unwrap(),
            stream_request_id: RequestId::new("stream").unwrap(),
            selection: selection(),
            source_subject: None,
            origin: None,
            native: None,
            stream: None,
            error: None,
            qualified: false,
        }
    }
    fn origin() -> HandyHspLiveOrigin {
        HandyHspLiveOrigin {
            stream_record_id: ArtifactId::new("b".repeat(64)).unwrap(),
            frame: FrameId::new(3),
            source_time: SourceTimestamp::new(7, 2).unwrap(),
            source_sequence: 2,
            first_position: NormalizedPosition::new(0.37).unwrap(),
            capture_earliest_boottime_ns: 100,
            capture_latest_boottime_ns: 110,
            accepted_earliest_boottime_ns: 120,
            accepted_latest_boottime_ns: 130,
            generation: 1,
            stream_id: 9,
        }
    }
    fn native() -> HandyHspLiveNativeStatus {
        HandyHspLiveNativeStatus {
            retired: true,
            original_play_request_id: Some(RequestId::new("play").unwrap()),
            original_stop_request_id: Some(RequestId::new("stop").unwrap()),
            delivered_samples: 2,
            active_source: None,
            error: Some(ProtocolError::new(
                ErrorCode::UnknownOutcome,
                "outcome unknown",
            )),
        }
    }
    fn populated() -> HandyHspLiveStatus {
        let subject = crate::handy_hsp_source_qualification::authored_hsp_source_subject_for_test();
        let origin = origin();
        let mut value = report();
        value.project_id = subject.project_id.clone();
        value.stream = Some(HandyHspStatus {
            project_id: subject.project_id.clone(),
            stream_request_id: value.stream_request_id.clone(),
            stream_record_id: ArtifactId::new("b".repeat(64)).unwrap(),
            generation: origin.generation,
            stream_id: origin.stream_id,
            live: LiveIdentity {
                epoch: value.selection.capture_epoch.clone(),
                project: subject.project_id.clone(),
                revision: subject.revision,
                source: subject.selection.source_version.clone(),
            },
            native: None,
            qualified: false,
            emergency_history_durable: false,
        });
        value.origin = Some(origin);
        value.source_subject = Some(subject);
        value
    }

    #[test]
    fn hsp_live_selection_keeps_opaque_timing_and_refuses_unreviewed_source() {
        let value = selection();
        value.validate().unwrap();
        let encoded = serde_json::to_value(&value).unwrap();
        let decoded: HandyHspLiveSelection = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(decoded, value);
        let mut extra = encoded;
        extra["native_authority"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HandyHspLiveSelection>(extra).is_err());
        for case in 0..3 {
            let mut bad = value.clone();
            match case {
                0 => bad.source_qualification.revision = 1,
                1 => bad.source_qualification.dossier_id = ArtifactId::new("not-a-hash").unwrap(),
                _ => bad.inference_attempt_id = bad.capture_epoch.clone(),
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
    }

    #[test]
    fn finite_hsp_selection_is_opt_in_and_bound_to_original_source_witness() {
        let old = selection();
        assert!(serde_json::to_value(&old)
            .unwrap()
            .get("finite_drain_policy_sha256")
            .is_none());
        let mut report = populated();
        report.selection.finite_drain_policy_sha256 = Some("d".repeat(64));
        assert!(report.validate().is_err());
        report
            .source_subject
            .as_mut()
            .unwrap()
            .finite_drain_policy_sha256 = Some("d".repeat(64));
        report.validate().unwrap();
        let wire = serde_json::to_value(&report).unwrap();
        let decoded: HandyHspLiveStatus = serde_json::from_value(wire.clone()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
        report.selection.finite_drain_policy_sha256 = Some("e".repeat(64));
        assert!(report.validate().is_err());
        report.selection.finite_drain_policy_sha256 = None;
        report.validate().unwrap();
        report.selection.finite_drain_policy_sha256 = Some("not-a-hash".into());
        assert!(report.selection.validate().is_err());
    }

    #[test]
    fn hsp_live_origin_preserves_first_position_and_bounded_original_times() {
        let expected = origin();
        expected.validate().unwrap();
        let value = serde_json::to_value(&expected).unwrap();
        assert_eq!(value["first_position"], serde_json::json!(0.37));
        let actual: HandyHspLiveOrigin = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(actual, expected);
        for case in 0..7 {
            let mut bad = expected.clone();
            match case {
                0 => bad.source_sequence = 0,
                1 => bad.generation = 0,
                2 => bad.stream_id = 0,
                3 => bad.capture_earliest_boottime_ns = 111,
                4 => bad.accepted_earliest_boottime_ns = 131,
                5 => bad.accepted_earliest_boottime_ns = 99,
                _ => bad.accepted_latest_boottime_ns = 109,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
        let mut invalid_position = value;
        invalid_position["first_position"] = serde_json::json!(1.01);
        assert!(serde_json::from_value::<HandyHspLiveOrigin>(invalid_position).is_err());
    }

    #[test]
    fn hsp_live_reservation_is_honest_and_witness_pair_cannot_be_partial() {
        let expected = report();
        expected.validate().unwrap();
        let encoded = serde_json::to_value(&expected).unwrap();
        let actual: HandyHspLiveStatus = serde_json::from_value(encoded.clone()).unwrap();
        actual.validate().unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), encoded);
        let mut partial = report();
        partial.origin = Some(origin());
        assert!(partial.validate().is_err());
        let mut qualified = report();
        qualified.qualified = true;
        assert!(qualified.validate().is_err());
        let mut reused = report();
        reused.stream_request_id = reused.selection.preparation_request_id.clone();
        assert!(reused.validate().is_err());
        let mut extra = encoded;
        extra["physical_playback"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HandyHspLiveStatus>(extra).is_err());
        let mut failed = report();
        failed.error = Some(ProtocolError::new(
            ErrorCode::UnknownOutcome,
            "original admission outcome is unknown",
        ));
        failed.validate().unwrap();
        assert!(failed.native.is_none() && failed.origin.is_none() && failed.stream.is_none());
        for message in [String::new(), "x".repeat(2049), "bad\0message".into()] {
            failed.error.as_mut().unwrap().message = message;
            assert!(failed.validate().is_err());
        }
    }

    #[test]
    fn hsp_live_native_observations_keep_unknown_and_refuse_identity_counter_errors() {
        let mut expected = report();
        expected.native = Some(native());
        expected.validate().unwrap();
        for case in 0..7 {
            let mut bad = expected.clone();
            let value = bad.native.as_mut().unwrap();
            match case {
                0 => value.delivered_samples = u64::from(u32::MAX) + 2,
                1 => value.original_stop_request_id = value.original_play_request_id.clone(),
                2 => value.original_play_request_id = Some(bad.stream_request_id.clone()),
                3 => {
                    value.original_stop_request_id =
                        Some(bad.selection.preparation_request_id.clone())
                }
                4 => value.error.as_mut().unwrap().message.clear(),
                5 => value.error.as_mut().unwrap().message = "x".repeat(2049),
                _ => value.error.as_mut().unwrap().message = "bad\0message".into(),
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
    }

    #[test]
    fn hsp_live_populated_history_preserves_exact_source_and_refuses_substituted_stream() {
        let expected = populated();
        expected.validate().unwrap();
        let wire = serde_json::to_value(&expected).unwrap();
        let actual: HandyHspLiveStatus = serde_json::from_value(wire.clone()).unwrap();
        actual.validate().unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), wire);
        for case in 0..10 {
            let mut bad = expected.clone();
            let stream = bad.stream.as_mut().unwrap();
            match case {
                0 => bad.source_subject = None,
                1 => bad.origin = None,
                2 => {
                    bad.source_subject.as_mut().unwrap().project_id =
                        ProjectId::new("other").unwrap()
                }
                3 => stream.stream_request_id = RequestId::new("other").unwrap(),
                4 => stream.live.epoch = AttemptId::new("other").unwrap(),
                5 => stream.live.source = SourceVersionId::new("other").unwrap(),
                6 => stream.live.revision = RevisionId::new(99),
                7 => stream.generation += 1,
                8 => stream.stream_id += 1,
                _ => stream.qualified = true,
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
        let mut pre_enqueue = expected;
        pre_enqueue.stream = None;
        pre_enqueue.validate().unwrap();
    }

    #[test]
    fn hsp_live_commands_have_explicit_identity_consent_and_retirement_scope() {
        let mut request = Request::new(
            RequestId::new("stream").unwrap(),
            Command::BindHandyHspLive {
                selection: selection(),
                acknowledge_current_environment: true,
                allow_online_control: true,
                owner_proof: "synthetic-owner-proof".into(),
            },
        );
        request.auth_token = Some("synthetic-auth-token".into());
        assert!(request.command.requires_project());
        assert!(request.command.requires_revision());
        assert!(request.command.is_mutating());
        assert!(request.validate().is_err());
        request.project = Some(ProjectId::new("project").unwrap());
        assert!(request.validate().is_err());
        request.expected_revision = Some(RevisionId::new(2));
        request.validate().unwrap();
        for debug in [format!("{request:?}"), format!("{:?}", request.command)] {
            assert!(!debug.contains("synthetic-owner-proof"));
            assert!(!debug.contains("synthetic-auth-token"));
        }
        let wire = serde_json::to_value(&request).unwrap();
        assert_eq!(wire["command"]["operation"], "bind_handy_hsp_live");
        let mut missing_environment = wire.clone();
        missing_environment["command"]["arguments"]
            .as_object_mut()
            .unwrap()
            .remove("acknowledge_current_environment");
        assert!(serde_json::from_value::<Request>(missing_environment).is_err());
        serde_json::from_value::<Request>(wire)
            .unwrap()
            .validate()
            .unwrap();
        for case in 0..4 {
            let mut bad = request.clone();
            let Command::BindHandyHspLive {
                selection,
                acknowledge_current_environment,
                allow_online_control,
                owner_proof,
            } = &mut bad.command
            else {
                unreachable!()
            };
            match case {
                0 => *allow_online_control = false,
                1 => owner_proof.clear(),
                2 => bad.request_id = selection.preparation_request_id.clone(),
                _ => *acknowledge_current_environment = false,
            }
            if case == 3 {
                assert_ne!(
                    request_fingerprint(&bad).unwrap(),
                    request_fingerprint(&request).unwrap(),
                    "environment acknowledgment must belong to the exact original fingerprint"
                );
                assert_eq!(bad.validate().unwrap_err().code, ErrorCode::Forbidden);
            }
            assert!(bad.validate().is_err(), "case {case}");
        }
        request.request_id = RequestId::new("play").unwrap();
        request.command = Command::PlayHandyHspLive {
            stream_request_id: RequestId::new("stream").unwrap(),
            owner_proof: "synthetic-owner-proof".into(),
        };
        assert!(request.command.requires_revision() && request.command.is_mutating());
        request.validate().unwrap();
        request.expected_revision = None;
        assert!(request.validate().is_err());
        for stop in [false, true] {
            request.request_id = RequestId::new("read-or-stop").unwrap();
            request.command = if stop {
                Command::StopHandyHspLive {
                    stream_request_id: RequestId::new("stream").unwrap(),
                    owner_proof: "synthetic-owner-proof".into(),
                }
            } else {
                Command::HandyHspLiveStatus {
                    stream_request_id: RequestId::new("stream").unwrap(),
                    owner_proof: "synthetic-owner-proof".into(),
                }
            };
            assert!(request.command.requires_project() && !request.command.requires_revision());
            assert_eq!(request.command.is_mutating(), stop);
            request.validate().unwrap();
            request.expected_revision = Some(RevisionId::new(2));
            assert!(request.validate().is_err());
            request.expected_revision = None;
            if stop {
                request.request_id = RequestId::new("stream").unwrap();
                assert!(request.validate().is_err());
            }
        }
    }
    #[test]
    fn active_source_extension_preserves_absent_legacy_native_bytes() {
        let value = native();
        let encoded = serde_json::to_value(&value).unwrap();
        assert!(encoded.get("active_source").is_none());
        let decoded: HandyHspLiveNativeStatus = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn active_source_description_keeps_original_binding_and_checks_lineage() {
        let mut value = populated();
        value.native = Some(native());
        let initial = HandyHspLiveActiveSource {
            generation: 1,
            capture_epoch: value.selection.capture_epoch.clone(),
            inference_attempt_id: value.selection.inference_attempt_id.clone(),
            browser_timing: value.selection.browser_timing.clone(),
            media_preparation_request_id: RequestId::new("original-media-preparation").unwrap(),
            predecessor_capture_epoch: None,
            last_transfer_request_id: None,
            pending_successor_epoch: None,
            pending_transfer_request_id: None,
        };
        value.native.as_mut().unwrap().active_source = Some(initial.clone());
        value.validate().unwrap();
        let original = value.origin.clone();
        let selection = value.selection.clone();
        let mut next = initial.clone();
        next.generation = 2;
        next.predecessor_capture_epoch = Some(initial.capture_epoch.clone());
        next.capture_epoch = AttemptId::new("actual-successor-epoch").unwrap();
        next.inference_attempt_id = AttemptId::new("actual-successor-attempt").unwrap();
        next.last_transfer_request_id = Some(RequestId::new("original-transfer").unwrap());
        value.native.as_mut().unwrap().active_source = Some(next.clone());
        value.validate().unwrap();
        assert_eq!(value.origin, original);
        assert_eq!(value.selection, selection);
        for mutation in 0..5 {
            let mut bad = next.clone();
            match mutation {
                0 => bad.generation = 0,
                1 => bad.last_transfer_request_id = None,
                2 => bad.predecessor_capture_epoch = Some(bad.capture_epoch.clone()),
                3 => bad.pending_successor_epoch = Some(bad.capture_epoch.clone()),
                _ => {
                    bad.pending_transfer_request_id =
                        Some(RequestId::new("pending-without-source").unwrap())
                }
            }
            assert!(bad.validate().is_err(), "mutation {mutation}");
        }
        let mut changed_initial = initial;
        changed_initial.capture_epoch = AttemptId::new("different-initial-source").unwrap();
        value.native.as_mut().unwrap().active_source = Some(changed_initial);
        assert!(value.validate().is_err());
    }
}
