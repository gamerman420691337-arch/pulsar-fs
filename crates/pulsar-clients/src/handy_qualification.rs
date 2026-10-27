//! Owner-only evidence review operations. No automatic acceptance or physical action.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::{Path, PathBuf};
fn proof(directory: &Path) -> Result<String> {
    crate::project_package::read_local_owner_proof(directory, "handy-owner.token")
}
pub fn context(client: &mut impl EngineApi, directory: &Path) -> Result<HandyQualificationContext> {
    context_for_transport(client, directory, HandyQualificationTransport::Hssp)
}
pub fn context_for_transport(
    client: &mut impl EngineApi,
    directory: &Path,
    transport: HandyQualificationTransport,
) -> Result<HandyQualificationContext> {
    let ResponseBody::HandyQualificationContext(c) = client.execute(
        Command::HandyQualificationContext {
            transport,
            owner_proof: proof(directory)?,
        },
        None,
        None,
    )?
    else {
        anyhow::bail!("qualification context response mismatch");
    };
    c.validate()?;
    ensure!(
        c.transport.as_str() == transport.transport_id(),
        "qualification transport response mismatch"
    );
    Ok(*c)
}
pub fn status(
    client: &mut impl EngineApi,
    directory: &Path,
    id: ArtifactId,
) -> Result<HandyQualificationReport> {
    let ResponseBody::HandyQualification(r) = client.execute(
        Command::HandyQualificationStatus {
            dossier_id: id.clone(),
            owner_proof: proof(directory)?,
        },
        None,
        None,
    )?
    else {
        anyhow::bail!("qualification status response mismatch");
    };
    r.validate()?;
    ensure!(r.dossier_id == id, "qualification identity changed");
    Ok(*r)
}
pub fn import(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    path: PathBuf,
    sha256: String,
) -> Result<HandyQualificationReceipt> {
    validate_sha256(&sha256)?;
    let ResponseBody::HandyQualificationSaved(r) = client.execute_identified(
        id.clone(),
        Command::ImportHandyQualification {
            manifest_path: path,
            manifest_sha256: sha256.clone(),
            owner_proof: proof(directory)?,
        },
        None,
        None,
    )?
    else {
        anyhow::bail!("qualification import response mismatch");
    };
    r.validate()?;
    ensure!(
        r.request_id == id && r.input_sha256.as_ref() == Some(&sha256),
        "qualification import request identity changed"
    );
    Ok(*r)
}
pub struct Review {
    pub dossier: ArtifactId,
    pub revision: u64,
    pub note: String,
    pub acceptance: Option<(DeviceSessionId, DeviceSessionId)>,
}
pub fn review(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &Review,
    physical_ack: bool,
    online_consent: bool,
) -> Result<HandyQualificationReceipt> {
    validate_handy_qualification_note(&intent.note)?;
    validate_sha256(intent.dossier.as_str())?;
    ensure!(
        intent.revision > 0 && intent.revision < i64::MAX as u64,
        "invalid review revision"
    );
    if intent.acceptance.is_some() {
        ensure!(
            physical_ack && online_consent,
            "Physical evidence acceptance and fresh online inspection require explicit consent"
        );
    }
    let owner_proof = proof(directory)?;
    let command = if let Some((epoch, session)) = &intent.acceptance {
        Command::AcceptHandyQualification {
            dossier_id: intent.dossier.clone(),
            expected_qualification_revision: intent.revision,
            engine_epoch: epoch.clone(),
            device_session: session.clone(),
            note: intent.note.clone(),
            acknowledge_physical_evidence: true,
            allow_online_inspection: true,
            owner_proof,
        }
    } else {
        Command::WithdrawHandyQualification {
            dossier_id: intent.dossier.clone(),
            expected_qualification_revision: intent.revision,
            note: intent.note.clone(),
            owner_proof,
        }
    };
    let ResponseBody::HandyQualificationSaved(r) =
        client.execute_identified(id.clone(), command, None, None)?
    else {
        anyhow::bail!("qualification review response mismatch");
    };
    r.validate()?;
    ensure!(
        r.request_id == id
            && r.input_sha256.is_none()
            && r.report
                .decision
                .as_ref()
                .is_some_and(|d| d.request_id == id && d.note == intent.note)
            && r.report.dossier_id == intent.dossier
            && r.report.revision == intent.revision + 1
            && r.report.state
                == if intent.acceptance.is_some() {
                    HandyQualificationState::Accepted
                } else {
                    HandyQualificationState::Withdrawn
                },
        "qualification review receipt changed captured intent"
    );
    Ok(*r)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn qualification_context_checks_exact_requested_transport() {
        use std::os::unix::fs::PermissionsExt;
        struct Api {
            response: HandyQualificationTransport,
        }
        impl EngineApi for Api {
            fn execute(
                &mut self,
                command: Command,
                project: Option<ProjectId>,
                revision: Option<RevisionId>,
            ) -> Result<ResponseBody> {
                assert!(matches!(
                    command,
                    Command::HandyQualificationContext {
                        transport: HandyQualificationTransport::Hsp,
                        ..
                    }
                ));
                assert!(project.is_none() && revision.is_none());
                Ok(ResponseBody::HandyQualificationContext(Box::new(
                    HandyQualificationContext {
                        schema_version: 1,
                        control_image: ArtifactIdentity {
                            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
                            sha256: "a".repeat(64),
                            byte_len: 1,
                        },
                        platform: "fixture-platform".into(),
                        transport: ArtifactId::new(self.response.transport_id()).unwrap(),
                    },
                )))
            }
        }
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let token = directory.path().join("handy-owner.token");
        std::fs::write(&token, "a".repeat(64)).unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(context_for_transport(
            &mut Api {
                response: HandyQualificationTransport::Hssp
            },
            directory.path(),
            HandyQualificationTransport::Hsp
        )
        .is_err());
        assert_eq!(
            context_for_transport(
                &mut Api {
                    response: HandyQualificationTransport::Hsp
                },
                directory.path(),
                HandyQualificationTransport::Hsp
            )
            .unwrap()
            .transport
            .as_str(),
            HANDY_HSP_QUALIFICATION_TRANSPORT
        );
    }
    struct NoCalls;
    impl EngineApi for NoCalls {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            panic!("missing consent must not dispatch")
        }
    }
    #[test]
    fn qualification_client_requires_both_explicit_consents_before_owner_lookup_or_rpc() {
        let intent = Review {
            dossier: ArtifactId::new("a".repeat(64)).unwrap(),
            revision: 1,
            note: "fixture only".into(),
            acceptance: Some((
                DeviceSessionId::new("boot").unwrap(),
                DeviceSessionId::new("session").unwrap(),
            )),
        };
        let mut client = NoCalls;
        for (physical, online) in [(false, false), (false, true), (true, false)] {
            assert!(review(
                &mut client,
                Path::new("/absent/fixture"),
                RequestId::new("review").unwrap(),
                &intent,
                physical,
                online
            )
            .unwrap_err()
            .to_string()
            .contains("explicit consent"));
        }
    }
}
