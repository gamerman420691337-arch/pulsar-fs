//! Captured reconciliation plans and historical outcomes, never live authority.
use crate::*;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRecoverySelection {
    pub device_id: DeviceId,
    pub registration_revision: u64,
    pub engine_epoch: DeviceSessionId,
    pub device_session: DeviceSessionId,
    pub qualification: HandyQualificationBinding,
}
impl HandyRecoverySelection {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.qualification.validate()?;
        if self.registration_revision == 0
            || self.registration_revision > i64::MAX as u64
            || self.engine_epoch == self.device_session
        {
            return Err(ProtocolError::invalid("invalid Handy recovery selection"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyCustodyCounts {
    pub setup: u32,
    pub clock_sync: u32,
    pub play: u32,
    pub effects: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub hsp_streams: u32,
    #[serde(default, skip_serializing_if = "zero")]
    pub hsp_preparations: u32,
}
fn zero(value: &u32) -> bool {
    *value == 0
}
impl HandyCustodyCounts {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let counts = [
            self.setup,
            self.clock_sync,
            self.play,
            self.effects,
            self.hsp_streams,
            self.hsp_preparations,
        ];
        let total: u64 = counts.iter().map(|value| u64::from(*value)).sum();
        if counts.iter().any(|value| *value > 1024)
            || self.hsp_streams > 64
            || self.hsp_preparations > 64
            || !(1..=4096).contains(&total)
        {
            return Err(ProtocolError::invalid(
                "Handy custody counts exceed bounds or are empty",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRecoveryPlan {
    pub plan_id: ArtifactId,
    pub snapshot_id: ArtifactId,
    pub project_id: ProjectId,
    pub selection: HandyRecoverySelection,
    pub registration: HandyRegistration,
    pub route_sha256: String,
    pub route_namespace_sha256: String,
    pub counts: HandyCustodyCounts,
    pub projects: Vec<ProjectId>,
    pub devices: Vec<DeviceId>,
    pub contract: HandyRecoveryQualification,
    pub playback_enabled: bool,
}
impl HandyRecoveryPlan {
    /// Hash every captured field except plan_id, in declared field order.
    /// This encodes identity only; callers must separately validate the plan.
    pub fn identity(&self) -> Result<ArtifactId, ProtocolError> {
        let bytes = serde_json::to_vec(&(
            &self.snapshot_id,
            &self.project_id,
            &self.selection,
            &self.registration,
            &self.route_sha256,
            &self.route_namespace_sha256,
            &self.counts,
            &self.projects,
            &self.devices,
            &self.contract,
            self.playback_enabled,
        ))
        .map_err(|_| ProtocolError::invalid("Handy recovery plan identity encoding failed"))?;
        ArtifactId::new(format!("{:x}", Sha256::digest(bytes)))
            .map_err(|error| ProtocolError::invalid(error.to_string()))
    }

    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_sha256(self.plan_id.as_str())?;
        validate_sha256(self.snapshot_id.as_str())?;
        validate_sha256(&self.route_sha256)?;
        validate_sha256(&self.route_namespace_sha256)?;
        self.selection.validate()?;
        self.registration.validate()?;
        self.counts.validate()?;
        self.contract.validate()?;
        let hsp = self.contract.transport == HandyQualificationTransport::Hsp;
        if hsp != (self.counts.hsp_streams > 0 || self.counts.hsp_preparations > 0)
            || (hsp
                && [
                    self.counts.setup,
                    self.counts.clock_sync,
                    self.counts.play,
                    self.counts.effects,
                ]
                .iter()
                .any(|n| *n != 0))
        {
            return Err(ProtocolError::invalid(
                "recovery contract transport differs from custody",
            ));
        }
        if self.playback_enabled
            || self.registration.disabled
            || self.registration.device_id != self.selection.device_id
            || self.registration.revision != self.selection.registration_revision
            || self.projects.is_empty()
            || self.projects.len() > 128
            || self.devices.is_empty()
            || self.devices.len() > 128
            || self
                .projects
                .windows(2)
                .any(|pair| pair[0].as_str() >= pair[1].as_str())
            || self
                .devices
                .windows(2)
                .any(|pair| pair[0].as_str() >= pair[1].as_str())
            || !self.devices.contains(&self.selection.device_id)
            || self.plan_id != self.identity()?
        {
            return Err(ProtocolError::invalid(
                "invalid captured Handy recovery plan",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandyRecoveryState {
    Pending,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRecoveryResolution {
    pub record_id: ArtifactId,
    pub basis_snapshot_id: ArtifactId,
    pub snapshot_id: ArtifactId,
    pub counts: HandyCustodyCounts,
    pub evidence: HandyRecoveryEvidence,
}
impl HandyRecoveryResolution {
    pub fn validate(&self, plan: &HandyRecoveryPlan) -> Result<(), ProtocolError> {
        validate_sha256(self.record_id.as_str())?;
        validate_sha256(self.basis_snapshot_id.as_str())?;
        validate_sha256(self.snapshot_id.as_str())?;
        self.counts.validate()?;
        self.evidence.validate(&plan.contract)?;
        if self.basis_snapshot_id != plan.snapshot_id
            || self.counts.setup != plan.counts.setup
            || self.counts.clock_sync != plan.counts.clock_sync
            || self.counts.play != plan.counts.play
            || self.counts.hsp_streams != plan.counts.hsp_streams
            || self.counts.hsp_preparations != plan.counts.hsp_preparations
            || self.counts.effects < plan.counts.effects
        {
            return Err(ProtocolError::invalid(
                "Handy recovery resolution changed its captured basis",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandyRecoveryStatus {
    pub project_id: ProjectId,
    pub reconciliation_request_id: RequestId,
    pub plan: HandyRecoveryPlan,
    pub state: HandyRecoveryState,
    pub resolution: Option<HandyRecoveryResolution>,
    pub error: Option<ProtocolError>,
    pub playback_enabled: bool,
    pub authority_restored: bool,
}
impl HandyRecoveryStatus {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.plan.validate()?;
        if self.project_id != self.plan.project_id
            || self.playback_enabled
            || self.authority_restored
        {
            return Err(ProtocolError::invalid(
                "Handy recovery history cannot restore authority",
            ));
        }
        let valid = match self.state {
            HandyRecoveryState::Completed => self.resolution.is_some() && self.error.is_none(),
            HandyRecoveryState::Pending | HandyRecoveryState::Cancelled => {
                self.resolution.is_none() && self.error.is_none()
            }
            HandyRecoveryState::Failed | HandyRecoveryState::Interrupted => {
                self.resolution.is_none() && self.error.is_some()
            }
        };
        if !valid {
            return Err(ProtocolError::invalid(
                "Handy recovery state and outcome differ",
            ));
        }
        if let Some(resolution) = &self.resolution {
            resolution.validate(&self.plan)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> HandyRecoveryPlan {
        let correlation: HandyProgramCorrelation = serde_json::from_str(include_str!(
            "../tests/fixtures/handy-program-correlation.json"
        ))
        .unwrap();
        let registration = correlation.inspection.registration;
        let mut plan = HandyRecoveryPlan {
            plan_id: ArtifactId::new("a".repeat(64)).unwrap(),
            snapshot_id: ArtifactId::new("b".repeat(64)).unwrap(),
            project_id: ProjectId::new("requested-project").unwrap(),
            selection: HandyRecoverySelection {
                device_id: registration.device_id.clone(),
                registration_revision: registration.revision,
                engine_epoch: DeviceSessionId::new("boot").unwrap(),
                device_session: DeviceSessionId::new("session").unwrap(),
                qualification: HandyQualificationBinding {
                    dossier_id: ArtifactId::new("c".repeat(64)).unwrap(),
                    revision: 2,
                },
            },
            devices: vec![registration.device_id.clone()],
            registration,
            route_sha256: "d".repeat(64),
            route_namespace_sha256: "e".repeat(64),
            counts: HandyCustodyCounts {
                setup: 1,
                clock_sync: 1,
                play: 1,
                effects: 1,
                hsp_streams: 0,
                hsp_preparations: 0,
            },
            projects: vec![ProjectId::new("historical-project").unwrap()],
            contract: HandyRecoveryQualification {
                transport: HandyQualificationTransport::Hssp,
                model: HandyRecoveryModel::DrainedHostThenStoppedSamples,
                remote_state: HandyRecoveryRemoteState::Stopped,
                prior_command_settle_ns: 1_000_000_000,
                stopped_sample_separation_ns: 1_000_000_000,
                maximum_sample_span_ns: 1_000_000_000,
            },
            playback_enabled: false,
        };
        plan.plan_id = plan.identity().unwrap();
        plan.validate().unwrap();
        plan
    }

    fn completed() -> HandyRecoveryStatus {
        let plan = plan();
        let correlation: HandyProgramCorrelation = serde_json::from_str(include_str!(
            "../tests/fixtures/handy-program-correlation.json"
        ))
        .unwrap();
        let observation = correlation.inspection.device;
        let resolution = HandyRecoveryResolution {
            record_id: ArtifactId::new("f".repeat(64)).unwrap(),
            basis_snapshot_id: plan.snapshot_id.clone(),
            snapshot_id: ArtifactId::new("1".repeat(64)).unwrap(),
            counts: plan.counts.clone(),
            evidence: HandyRecoveryEvidence {
                drained_boottime_ns: 1_000_000_000,
                first: HandyRecoverySample {
                    started_boottime_ns: 2_000_000_000,
                    finished_boottime_ns: 2_100_000_000,
                    observation: observation.clone(),
                },
                second: HandyRecoverySample {
                    started_boottime_ns: 3_100_000_000,
                    finished_boottime_ns: 3_200_000_000,
                    observation,
                },
            },
        };
        let status = HandyRecoveryStatus {
            project_id: plan.project_id.clone(),
            reconciliation_request_id: RequestId::new("reconcile").unwrap(),
            plan,
            state: HandyRecoveryState::Completed,
            resolution: Some(resolution),
            error: None,
            playback_enabled: false,
            authority_restored: false,
        };
        status.validate().unwrap();
        status
    }

    #[test]
    fn handy_recovery_plan_identity_binds_every_captured_field_except_itself() {
        let original = plan();
        assert!(!original.projects.contains(&original.project_id));
        for case in 0..11 {
            let mut changed = original.clone();
            match case {
                0 => changed.snapshot_id = ArtifactId::new("9".repeat(64)).unwrap(),
                1 => changed.project_id = ProjectId::new("different-project").unwrap(),
                2 => {
                    changed.selection.device_session =
                        DeviceSessionId::new("different-session").unwrap()
                }
                3 => changed.registration.settings.label = "different label".into(),
                4 => changed.route_sha256 = "8".repeat(64),
                5 => changed.route_namespace_sha256 = "7".repeat(64),
                6 => changed.counts.effects += 1,
                7 => changed.projects.push(ProjectId::new("z-project").unwrap()),
                8 => changed.devices.push(DeviceId::new("z-device").unwrap()),
                9 => changed.contract.prior_command_settle_ns += 1,
                _ => changed.playback_enabled = true,
            }
            assert_ne!(
                changed.identity().unwrap(),
                original.plan_id,
                "field case {case}"
            );
            assert!(changed.validate().is_err(), "stale identity case {case}");
        }
        let mut changed = original.clone();
        changed.plan_id = ArtifactId::new("0".repeat(64)).unwrap();
        assert_eq!(changed.identity().unwrap(), original.plan_id);
        assert!(changed.validate().is_err());
    }

    #[test]
    fn handy_recovery_plan_validates_scope_lists_registration_and_no_authority() {
        for case in 0..20 {
            let mut value = plan();
            match case {
                0 => value.playback_enabled = true,
                1 => value.registration.disabled = true,
                2 => value.registration.device_id = DeviceId::new("other-device").unwrap(),
                3 => value.registration.revision += 1,
                4 => value.selection.engine_epoch = value.selection.device_session.clone(),
                5 => value.projects.clear(),
                6 => value.devices.clear(),
                7 => value.projects.push(value.projects[0].clone()),
                8 => value.devices.push(value.devices[0].clone()),
                9 => {
                    value.projects =
                        vec![ProjectId::new("z").unwrap(), ProjectId::new("a").unwrap()]
                }
                10 => {
                    value.devices = vec![
                        DeviceId::new("z").unwrap(),
                        value.selection.device_id.clone(),
                    ]
                }
                11 => {
                    value.projects = (0..129)
                        .map(|i| ProjectId::new(format!("project-{i:03}")).unwrap())
                        .collect()
                }
                12 => {
                    value.devices = (0..129)
                        .map(|i| DeviceId::new(format!("device-{i:03}")).unwrap())
                        .collect()
                }
                13 => value.devices = vec![DeviceId::new("other-device").unwrap()],
                14 => value.snapshot_id = ArtifactId::new("bad-digest").unwrap(),
                15 => value.route_sha256 = "invalid".into(),
                16 => value.route_namespace_sha256 = "invalid".into(),
                17 => value.selection.registration_revision = 0,
                18 => value.selection.qualification.revision = 1,
                _ => value.counts.effects = 1025,
            }
            value.plan_id = value.identity().unwrap();
            assert!(value.validate().is_err(), "case {case}");
        }
        HandyCustodyCounts {
            setup: 1024,
            clock_sync: 1024,
            play: 1024,
            effects: 1024,
            hsp_streams: 0,
            hsp_preparations: 0,
        }
        .validate()
        .unwrap();
        for counts in [
            HandyCustodyCounts {
                setup: 0,
                clock_sync: 0,
                play: 0,
                effects: 0,
                hsp_streams: 0,
                hsp_preparations: 0,
            },
            HandyCustodyCounts {
                setup: 1025,
                clock_sync: 0,
                play: 0,
                effects: 0,
                hsp_streams: 0,
                hsp_preparations: 0,
            },
            HandyCustodyCounts {
                setup: 0,
                clock_sync: 1025,
                play: 0,
                effects: 0,
                hsp_streams: 0,
                hsp_preparations: 0,
            },
            HandyCustodyCounts {
                setup: 0,
                clock_sync: 0,
                play: 1025,
                effects: 0,
                hsp_streams: 0,
                hsp_preparations: 0,
            },
        ] {
            assert!(counts.validate().is_err());
        }
    }

    #[test]
    fn hsp_preparation_custody_counts_preserve_legacy_bytes_and_bind_aggregate_recovery() {
        let old = br#"{"setup":1,"clock_sync":1,"play":1,"effects":1}"#;
        let counts: HandyCustodyCounts = serde_json::from_slice(old).unwrap();
        counts.validate().unwrap();
        assert_eq!(counts.hsp_streams, 0);
        assert_eq!(counts.hsp_preparations, 0);
        assert_eq!(serde_json::to_vec(&counts).unwrap(), old.as_slice());
        for (streams, preparations) in [(0, 1), (1, 0), (1, 1), (64, 64)] {
            let mut value = plan();
            value.counts = HandyCustodyCounts {
                setup: 0,
                clock_sync: 0,
                play: 0,
                effects: 0,
                hsp_streams: streams,
                hsp_preparations: preparations,
            };
            value.contract.transport = HandyQualificationTransport::Hsp;
            value.plan_id = value.identity().unwrap();
            value.validate().unwrap();
            let original = value.plan_id.clone();
            value.counts.hsp_preparations += 1;
            assert_ne!(value.identity().unwrap(), original);
            value.counts.hsp_preparations = 65;
            value.plan_id = value.identity().unwrap();
            assert!(value.validate().is_err());
            value.counts.hsp_preparations = preparations;
            value.counts.setup = 1;
            value.plan_id = value.identity().unwrap();
            assert!(value.validate().is_err());
        }
        let mut wrong_transport = plan();
        wrong_transport.counts = HandyCustodyCounts {
            setup: 0,
            clock_sync: 0,
            play: 0,
            effects: 0,
            hsp_streams: 0,
            hsp_preparations: 1,
        };
        wrong_transport.plan_id = wrong_transport.identity().unwrap();
        assert!(wrong_transport.validate().is_err());
        for preparations in [false, true] {
            let mut status = completed();
            let resolution = status.resolution.as_mut().unwrap();
            if preparations {
                resolution.counts.hsp_preparations = 1;
            } else {
                resolution.counts.hsp_streams = 1;
            }
            assert!(resolution.validate(&status.plan).is_err());
        }
    }
    #[test]
    fn handy_recovery_status_validates_state_outcome_matrix_and_resolution_basis() {
        let good = completed();
        for state in [
            HandyRecoveryState::Pending,
            HandyRecoveryState::Completed,
            HandyRecoveryState::Failed,
            HandyRecoveryState::Cancelled,
            HandyRecoveryState::Interrupted,
        ] {
            for with_resolution in [false, true] {
                for with_error in [false, true] {
                    let mut value = good.clone();
                    value.state = state;
                    if !with_resolution {
                        value.resolution = None;
                    }
                    value.error = with_error.then(|| ProtocolError::invalid("synthetic failure"));
                    let expected = match state {
                        HandyRecoveryState::Completed => with_resolution && !with_error,
                        HandyRecoveryState::Pending | HandyRecoveryState::Cancelled => {
                            !with_resolution && !with_error
                        }
                        HandyRecoveryState::Failed | HandyRecoveryState::Interrupted => {
                            !with_resolution && with_error
                        }
                    };
                    assert_eq!(value.validate().is_ok(), expected);
                }
            }
        }
        for case in 0..10 {
            let mut value = good.clone();
            match case {
                0 => value.playback_enabled = true,
                1 => value.authority_restored = true,
                2 => value.project_id = ProjectId::new("other").unwrap(),
                3 => {
                    value.resolution.as_mut().unwrap().basis_snapshot_id =
                        ArtifactId::new("9".repeat(64)).unwrap()
                }
                4 => value.resolution.as_mut().unwrap().counts.setup += 1,
                5 => value.resolution.as_mut().unwrap().counts.clock_sync += 1,
                6 => value.resolution.as_mut().unwrap().counts.play += 1,
                7 => value.resolution.as_mut().unwrap().counts.effects = 0,
                8 => {
                    value.resolution.as_mut().unwrap().record_id =
                        ArtifactId::new("invalid").unwrap()
                }
                _ => {
                    value
                        .resolution
                        .as_mut()
                        .unwrap()
                        .evidence
                        .second
                        .started_boottime_ns = 1
                }
            }
            assert!(value.validate().is_err(), "case {case}");
        }
        let mut more_stops = good;
        more_stops.resolution.as_mut().unwrap().counts.effects += 1;
        more_stops.validate().unwrap();
    }

    #[test]
    fn handy_recovery_commands_bind_consent_scope_and_cancellation_identity() {
        let plan = plan();
        let commands = [
            Command::PlanHandyReconciliation {
                selection: plan.selection.clone(),
                owner_proof: "private-proof".into(),
            },
            Command::ReconcileHandyEffects {
                plan_id: plan.plan_id.clone(),
                allow_online_inspection: true,
                allow_resolution: true,
                owner_proof: "private-proof".into(),
            },
            Command::HandyReconciliationStatus {
                reconciliation_request_id: RequestId::new("original").unwrap(),
                owner_proof: "private-proof".into(),
            },
            Command::CancelHandyReconciliation {
                reconciliation_request_id: RequestId::new("original").unwrap(),
                owner_proof: "private-proof".into(),
            },
        ];
        for (index, command) in commands.into_iter().enumerate() {
            let mut request = Request::new(RequestId::new("new-request").unwrap(), command);
            assert!(request.command.requires_project());
            assert!(!request.command.requires_revision());
            assert_eq!(request.command.is_mutating(), index == 1 || index == 3);
            assert!(!format!("{request:?}").contains("private-proof"));
            assert!(request.validate().is_err());
            request.project = Some(plan.project_id.clone());
            request.validate().unwrap();
            let roundtrip: Request =
                serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
            roundtrip.validate().unwrap();
            match &mut request.command {
                Command::PlanHandyReconciliation { owner_proof, .. }
                | Command::ReconcileHandyEffects { owner_proof, .. }
                | Command::HandyReconciliationStatus { owner_proof, .. }
                | Command::CancelHandyReconciliation { owner_proof, .. } => {
                    *owner_proof = "a".repeat(65)
                }
                _ => unreachable!(),
            }
            assert!(request.validate().is_err());
        }
        for (online, resolve) in [(false, false), (true, false), (false, true)] {
            let request = Request::new(
                RequestId::new("start").unwrap(),
                Command::ReconcileHandyEffects {
                    plan_id: plan.plan_id.clone(),
                    allow_online_inspection: online,
                    allow_resolution: resolve,
                    owner_proof: "proof".into(),
                },
            )
            .in_project(plan.project_id.clone(), None);
            assert_eq!(request.validate().unwrap_err().code, ErrorCode::Forbidden);
        }
        let request = Request::new(
            RequestId::new("same").unwrap(),
            Command::CancelHandyReconciliation {
                reconciliation_request_id: RequestId::new("same").unwrap(),
                owner_proof: "proof".into(),
            },
        )
        .in_project(plan.project_id, None);
        assert!(request.validate().is_err());
    }

    #[test]
    fn handy_recovery_wire_roundtrips_history_and_refuses_authority_extensions() {
        let original = completed();
        for body in [
            ResponseBody::HandyRecoveryPlan(Box::new(original.plan.clone())),
            ResponseBody::HandyRecovery(Box::new(original.clone())),
        ] {
            let encoded = serde_json::to_value(body).unwrap();
            let restored: ResponseBody = serde_json::from_value(encoded.clone()).unwrap();
            assert_eq!(serde_json::to_value(restored).unwrap(), encoded);
        }
        for field in ["physical_stopped", "current", "native_authority"] {
            let mut malformed = serde_json::to_value(&original).unwrap();
            malformed[field] = serde_json::json!(true);
            assert!(serde_json::from_value::<HandyRecoveryStatus>(malformed).is_err());
        }
        let mut malformed = serde_json::to_value(&original.plan).unwrap();
        malformed["physical_stopped"] = serde_json::json!(true);
        assert!(serde_json::from_value::<HandyRecoveryPlan>(malformed).is_err());
    }
}
