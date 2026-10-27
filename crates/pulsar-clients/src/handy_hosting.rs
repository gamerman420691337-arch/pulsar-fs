//! Explicit temporary public disclosure; never implicit preparation or device setup.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::Path;
pub struct HostingIntent {
    pub project: ProjectId,
    pub revision: RevisionId,
    pub preparation: RequestId,
    pub script_sha256: String,
    pub engine_epoch: DeviceSessionId,
}
fn proof(directory: &Path) -> Result<String> {
    crate::project_package::read_local_owner_proof(directory, "handy-owner.token")
}
pub fn host(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    intent: &HostingIntent,
    consent: bool,
) -> Result<HandyHostingReport> {
    ensure!(
        consent,
        "Temporary public script hosting requires explicit consent"
    );
    validate_sha256(&intent.script_sha256)?;
    ensure!(
        id != intent.preparation,
        "Hosting needs a distinct request ID"
    );
    let ResponseBody::HandyHosting(report) = client.execute_identified(
        id.clone(),
        Command::HostHandyStrokeScript {
            preparation_request_id: intent.preparation.clone(),
            script_sha256: intent.script_sha256.clone(),
            engine_epoch: intent.engine_epoch.clone(),
            allow_temporary_public_hosting: true,
            owner_proof: proof(directory)?,
        },
        Some(intent.project.clone()),
        Some(intent.revision),
    )?
    else {
        anyhow::bail!("Handy hosting response mismatch");
    };
    report.validate()?;
    ensure!(
        report.request_id == id
            && report.preparation_request_id == intent.preparation
            && report.project_id == intent.project
            && report.revision == intent.revision
            && report.script.sha256 == intent.script_sha256
            && report.engine_epoch == intent.engine_epoch,
        "Handy hosting changed captured disclosure intent"
    );
    Ok(*report)
}
pub fn status(
    client: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    upload: RequestId,
) -> Result<HandyHostingReport> {
    let ResponseBody::HandyHosting(report) = client.execute(
        Command::HandyHostingStatus {
            upload_request_id: upload.clone(),
            owner_proof: proof(directory)?,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("Handy hosting status mismatch");
    };
    report.validate()?;
    ensure!(
        report.project_id == project && report.request_id == upload,
        "Handy hosting status binding mismatch"
    );
    Ok(*report)
}
pub fn forget(
    client: &mut impl EngineApi,
    directory: &Path,
    id: RequestId,
    project: ProjectId,
    upload: RequestId,
) -> Result<HandyHostingReport> {
    ensure!(id != upload, "Forgetting needs its own request ID");
    let ResponseBody::HandyHosting(report) = client.execute_identified(
        id,
        Command::ForgetHandyHosting {
            upload_request_id: upload.clone(),
            owner_proof: proof(directory)?,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("Handy hosting retirement mismatch");
    };
    report.validate()?;
    ensure!(
        report.project_id == project
            && report.request_id == upload
            && report.retired
            && !report.link_available,
        "Handy hosting retirement binding mismatch"
    );
    Ok(*report)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Api {
        calls: Vec<(RequestId, Command, Option<ProjectId>, Option<RevisionId>)>,
        reply: Option<ResponseBody>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            anyhow::bail!("unexpected unpinned call")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            c: Command,
            p: Option<ProjectId>,
            r: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.calls.push((id, c, p, r));
            self.reply
                .take()
                .ok_or_else(|| anyhow::anyhow!("lost reply"))
        }
    }
    #[test]
    fn handy_hosting_client_replays_captured_consent_and_rejects_changed_subject() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let key = dir.path().join("handy-owner.token");
        std::fs::write(&key, "c".repeat(64)).unwrap();
        std::fs::set_permissions(key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let intent = HostingIntent {
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(1),
            preparation: RequestId::new("prepare").unwrap(),
            script_sha256: "a".repeat(64),
            engine_epoch: DeviceSessionId::new("boot").unwrap(),
        };
        let id = RequestId::new("upload").unwrap();
        let mut api = Api {
            calls: vec![],
            reply: None,
        };
        assert!(host(&mut api, dir.path(), id.clone(), &intent, false).is_err());
        assert!(api.calls.is_empty());
        assert!(host(&mut api, dir.path(), id.clone(), &intent, true).is_err());
        let report = HandyHostingReport {
            request_id: id.clone(),
            preparation_request_id: intent.preparation.clone(),
            project_id: intent.project.clone(),
            revision: intent.revision,
            engine_epoch: intent.engine_epoch.clone(),
            master_sha256: "b".repeat(64),
            script: ArtifactIdentity {
                artifact_id: ArtifactId::new(intent.script_sha256.clone()).unwrap(),
                sha256: intent.script_sha256.clone(),
                byte_len: 40,
            },
            outcome: HandyHostingOutcome::Unknown,
            downloaded: None,
            retired: false,
            link_available: false,
            upload_in_flight: false,
        };
        api.reply = Some(ResponseBody::HandyHosting(Box::new(report.clone())));
        assert_eq!(
            host(&mut api, dir.path(), id.clone(), &intent, true)
                .unwrap()
                .outcome,
            HandyHostingOutcome::Unknown
        );
        assert_eq!(
            serde_json::to_value(&api.calls[0]).unwrap(),
            serde_json::to_value(&api.calls[1]).unwrap()
        );
        let mut wrong = report;
        wrong.preparation_request_id = RequestId::new("different").unwrap();
        api.reply = Some(ResponseBody::HandyHosting(Box::new(wrong)));
        assert!(host(&mut api, dir.path(), id, &intent, true).is_err());
    }
}
