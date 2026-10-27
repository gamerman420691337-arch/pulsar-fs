//! Explicit browser/source dossier intake and review. No acceptance is inferred.
use crate::{motion::ResponseBody, session::EngineApi};
use anyhow::{ensure, Result};
use pulsar_protocol::*;
use std::path::{Path, PathBuf};

fn proof(directory: &Path) -> Result<String> {
    crate::project_package::read_local_owner_proof(directory, "handy-owner.token")
}
fn subject(
    report: &HandyBrowserStartQualificationReport,
    project: &ProjectId,
    revision: Option<RevisionId>,
) -> Result<()> {
    report.validate()?;
    ensure!(
        &report.manifest.subject.project_id == project
            && revision.is_none_or(|revision| revision == report.manifest.subject.revision),
        "browser dossier project/revision differs"
    );
    Ok(())
}
pub fn import(
    api: &mut impl EngineApi,
    directory: &Path,
    request: RequestId,
    project: ProjectId,
    revision: RevisionId,
    path: PathBuf,
    sha256: String,
) -> Result<HandyBrowserStartQualificationReceipt> {
    validate_sha256(&sha256)?;
    ensure!(
        path.is_absolute(),
        "browser evidence input path must be absolute"
    );
    let ResponseBody::HandyBrowserStartQualificationSaved(receipt) = api.execute_identified(
        request.clone(),
        Command::ImportHandyBrowserStartQualification {
            path,
            manifest_sha256: sha256.clone(),
            owner_proof: proof(directory)?,
        },
        Some(project.clone()),
        Some(revision),
    )?
    else {
        anyhow::bail!("browser dossier import response differs");
    };
    receipt.validate()?;
    subject(&receipt.report, &project, Some(revision))?;
    ensure!(
        receipt.request_id == request && receipt.input_sha256.as_ref() == Some(&sha256),
        "browser dossier import identity differs"
    );
    Ok(*receipt)
}
pub fn status(
    api: &mut impl EngineApi,
    directory: &Path,
    project: ProjectId,
    dossier: ArtifactId,
) -> Result<HandyBrowserStartQualificationReport> {
    validate_sha256(dossier.as_str())?;
    let ResponseBody::HandyBrowserStartQualification(report) = api.execute(
        Command::HandyBrowserStartQualificationStatus {
            dossier_id: dossier.clone(),
            owner_proof: proof(directory)?,
        },
        Some(project.clone()),
        None,
    )?
    else {
        anyhow::bail!("browser dossier status response differs");
    };
    subject(&report, &project, None)?;
    ensure!(
        report.dossier_id == dossier,
        "browser dossier identity differs"
    );
    Ok(*report)
}
pub struct Review {
    pub project: ProjectId,
    pub project_revision: Option<RevisionId>,
    pub dossier: ArtifactId,
    pub record_revision: u64,
    pub operator: Option<String>,
    pub reason: String,
}
pub fn review(
    api: &mut impl EngineApi,
    directory: &Path,
    request: RequestId,
    intent: &Review,
    acknowledge_measured_evidence: bool,
) -> Result<HandyBrowserStartQualificationReceipt> {
    validate_sha256(intent.dossier.as_str())?;
    ensure!(
        intent.record_revision > 0 && intent.record_revision < i64::MAX as u64,
        "invalid browser dossier record revision"
    );
    let accepting = intent.operator.is_some();
    ensure!(
        accepting == intent.project_revision.is_some(),
        "browser dossier acceptance requires a project revision; withdrawal is historical"
    );
    if let Some(operator) = &intent.operator {
        ensure!(
            acknowledge_measured_evidence,
            "explicit measured-evidence acknowledgment is required"
        );
        validate_handy_qualification_note(operator)?;
    } else {
        ensure!(
            !intent.reason.trim().is_empty(),
            "bounded withdrawal reason required"
        );
        validate_handy_qualification_note(&intent.reason)?;
    }
    let owner_proof = proof(directory)?;
    let command = if let Some(operator) = &intent.operator {
        Command::AcceptHandyBrowserStartQualification {
            dossier_id: intent.dossier.clone(),
            expected_record_revision: intent.record_revision,
            operator: operator.clone(),
            acknowledge_measured_evidence: true,
            owner_proof,
        }
    } else {
        Command::WithdrawHandyBrowserStartQualification {
            dossier_id: intent.dossier.clone(),
            expected_record_revision: intent.record_revision,
            reason: intent.reason.clone(),
            owner_proof,
        }
    };
    let ResponseBody::HandyBrowserStartQualificationSaved(receipt) = api.execute_identified(
        request.clone(),
        command,
        Some(intent.project.clone()),
        intent.project_revision,
    )?
    else {
        anyhow::bail!("browser dossier review response differs");
    };
    receipt.validate()?;
    subject(&receipt.report, &intent.project, intent.project_revision)?;
    ensure!(
        receipt.request_id == request
            && receipt.input_sha256.is_none()
            && receipt.report.dossier_id == intent.dossier
            && receipt.report.revision == intent.record_revision + 1
            && receipt.report.state
                == if accepting {
                    HandyQualificationState::Accepted
                } else {
                    HandyQualificationState::Withdrawn
                }
            && receipt
                .report
                .decision
                .as_ref()
                .is_some_and(|decision| decision.request_id == request
                    && decision.operator == intent.operator
                    && decision.note == if accepting { "" } else { &intent.reason }),
        "browser dossier review differs from the explicit original intent"
    );
    Ok(*receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct NoCalls;
    impl EngineApi for NoCalls {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            panic!("invalid review must not reach IPC")
        }
    }
    #[test]
    fn acceptance_consent_and_revision_validate_before_owner_proof_or_ipc() {
        let mut intent = Review {
            project: ProjectId::new("project").unwrap(),
            project_revision: Some(RevisionId::new(1)),
            dossier: ArtifactId::new("a".repeat(64)).unwrap(),
            record_revision: 1,
            operator: Some("explicit operator".into()),
            reason: String::new(),
        };
        let error = review(
            &mut NoCalls,
            Path::new("/absent-owner-directory"),
            RequestId::new("accept").unwrap(),
            &intent,
            false,
        )
        .unwrap_err();
        assert!(error.to_string().contains("acknowledgment"));
        intent.project_revision = None;
        assert!(review(
            &mut NoCalls,
            Path::new("/absent"),
            RequestId::new("accept").unwrap(),
            &intent,
            true
        )
        .unwrap_err()
        .to_string()
        .contains("project revision"));
        intent.operator = None;
        intent.reason = " ".into();
        assert!(review(
            &mut NoCalls,
            Path::new("/absent"),
            RequestId::new("withdraw").unwrap(),
            &intent,
            false
        )
        .unwrap_err()
        .to_string()
        .contains("withdrawal reason"));
    }
}
