use super::*;

pub(super) fn select_media_source(
    sources: &[pulsar_protocol::SourceSummary],
    preferred: Option<&SourceVersionId>,
) -> Option<SourceVersionId> {
    sources
        .iter()
        .find(|source| {
            source.kind == pulsar_protocol::SourceKind::Media
                && preferred == Some(&source.source_version)
        })
        .or_else(|| {
            sources
                .iter()
                .find(|source| source.kind == pulsar_protocol::SourceKind::Media)
        })
        .map(|source| source.source_version.clone())
}

/// Snapshot-driven placement/source selection starts at the declared valid
/// anchor. Ordinary refreshes preserve the user's playhead and preview.
pub(super) fn snapshot_preview_seek(
    current: f64,
    placement: Option<&SourcePlacementSpec>,
    placement_changed: bool,
    source_changed: bool,
) -> Option<f64> {
    if !placement_changed && !source_changed {
        return None;
    }
    Some(
        placement
            .map(|p| p.project_anchor.as_nanos() as f64 / 1e9)
            .unwrap_or(if source_changed { 0.0 } else { current }),
    )
}

pub(super) fn independent_audio_rate(
    numerator: &str,
    denominator: &str,
) -> anyhow::Result<(i64, u32)> {
    let numerator = numerator.parse::<i64>().map_err(|_| {
        anyhow::anyhow!("Audio clock rate numerator must be a positive 64-bit integer")
    })?;
    let denominator = denominator.parse::<u32>().map_err(|_| {
        anyhow::anyhow!("Audio clock rate denominator must be a nonzero 32-bit integer")
    })?;
    AudioClockRate::new(numerator, denominator)?;
    Ok((numerator, denominator))
}

pub(super) fn independent_audio_selection(
    sources: &[SourceSummary],
    primary: Option<&SourceVersionId>,
    selected: Option<&SourceVersionId>,
    anchors: [(i64, u32); 2],
    rate: (i64, u32),
) -> anyhow::Result<IndependentAudio> {
    let source_version = selected
        .filter(|selected| Some(*selected) != primary)
        .filter(|selected| {
            sources.iter().any(|source| {
                &source.source_version == *selected && source.kind == SourceKind::Media
            })
        })
        .ok_or_else(|| anyhow::anyhow!("Select a distinct imported media source for audio"))?
        .clone();
    Ok(IndependentAudio {
        source_version,
        alignment: AudioAlignment {
            video_anchor: SourceTimestamp::new(anchors[0].0, anchors[0].1)?,
            audio_anchor: SourceTimestamp::new(anchors[1].0, anchors[1].1)?,
            rate: {
                let checked = AudioClockRate::new(rate.0, rate.1)?;
                (rate.0 != i64::from(rate.1)).then_some(checked)
            },
        },
    })
}

pub(super) fn diagnostics_match_view(
    report: &DiagnosticsReport,
    project: Option<(&ProjectId, RevisionId)>,
    candidate: Option<&CandidateId>,
) -> bool {
    project.is_some_and(|(id, revision)| id == &report.project_id && revision == report.revision)
        && (report.candidate_id.is_none() || report.candidate_id.as_ref() == candidate)
}

pub(super) fn accept_diagnostics_report(
    current: &mut Option<DiagnosticsReport>,
    report: DiagnosticsReport,
    project: Option<(&ProjectId, RevisionId)>,
    candidate: Option<&CandidateId>,
) -> bool {
    if !diagnostics_match_view(&report, project, candidate) {
        return false;
    }
    *current = Some(report);
    true
}

pub(super) fn execute_work(
    client: &mut impl EngineApi,
    work: RpcWork,
) -> anyhow::Result<ResponseBody> {
    match work.command {
        RpcCommand::Identified(id, command) => {
            client.execute_identified(id, command, work.project, work.revision)
        }
        RpcCommand::Wire(Command::CommitCandidate { candidate_id }) => crate::commit_candidate(
            client,
            work.project
                .ok_or_else(|| anyhow::anyhow!("Candidate commit lacks project identity"))?,
            work.revision
                .ok_or_else(|| anyhow::anyhow!("Candidate commit lacks reviewed revision"))?,
            candidate_id,
            None,
        )
        .map(|snapshot| ResponseBody::Project(Box::new(snapshot))),
        RpcCommand::Wire(Command::MergeCandidate {
            candidate_id,
            axes,
            range,
        }) => crate::merge_candidate(
            client,
            work.project
                .ok_or_else(|| anyhow::anyhow!("Candidate merge lacks project identity"))?,
            work.revision
                .ok_or_else(|| anyhow::anyhow!("Candidate merge lacks reviewed revision"))?,
            candidate_id,
            axes,
            range,
        )
        .map(|snapshot| ResponseBody::Project(Box::new(snapshot))),
        RpcCommand::Wire(Command::ExportAxis { path, axis }) => crate::export_axis(
            client,
            work.project
                .ok_or_else(|| anyhow::anyhow!("Axis export lacks project identity"))?,
            work.revision
                .ok_or_else(|| anyhow::anyhow!("Axis export lacks committed revision"))?,
            path,
            axis,
        ),
        RpcCommand::Wire(
            command @ (Command::ImportFunscript { .. } | Command::ImportFunscriptForRepair { .. }),
        ) => {
            let project = work
                .project
                .ok_or_else(|| anyhow::anyhow!("Script import lacks project identity"))?;
            let revision = work
                .revision
                .ok_or_else(|| anyhow::anyhow!("Script import lacks base revision"))?;
            crate::import_script_candidate(client, project, revision, command)
                .map(|candidate| ResponseBody::Candidate(Box::new(candidate)))
        }
        RpcCommand::Wire(command) => client.execute(command, work.project, work.revision),
        RpcCommand::HandyInspect(_) | RpcCommand::HandyRegistration(_) => {
            anyhow::bail!("Handy inspection requires the trusted session-bound owner directory")
        }
        RpcCommand::FallbackGeneration(_) => {
            anyhow::bail!("Fallback generation requires the captured authenticated session")
        }
        RpcCommand::Equivalence(_) => {
            anyhow::bail!("Evidence review requires the trusted session-bound owner directory")
        }
        RpcCommand::DismissPackRecovery(_) => {
            anyhow::bail!("Recovery dismissal requires the session-bound local store")
        }
        RpcCommand::Repair(request) => {
            if work.project.as_ref() != Some(&request.project)
                || work.revision != Some(request.revision)
            {
                anyhow::bail!("Repair work differs from its captured project/revision");
            }
            crate::repair_panel::propose(client, request)
                .map(|candidate| ResponseBody::Candidate(Box::new(candidate)))
        }
        RpcCommand::Edit(proposal) => {
            let project = work
                .project
                .ok_or_else(|| anyhow::anyhow!("Edit draft lacks project identity"))?;
            let revision = work
                .revision
                .ok_or_else(|| anyhow::anyhow!("Edit draft lacks base revision"))?;
            let candidate = client.upload_edit(
                project.clone(),
                revision,
                &proposal.program,
                &proposal.label,
            )?;
            let candidate_id = candidate.candidate_id;
            crate::commit_candidate(client, project, revision, candidate_id.clone(), None)
                .map(|snapshot| ResponseBody::Project(Box::new(snapshot)))
                .map_err(|error| {
                    let message = format!("Edit candidate {candidate_id} exists; commit was not confirmed. Explicit review/rebase is required before a new commit. Cause: {error}");
                    error.context(message)
                })
        }
    }
}

pub(super) fn execute_recoverable_work(
    client: &mut SessionClient,
    directory: &std::path::Path,
    work: RpcWork,
) -> anyhow::Result<ResponseBody> {
    if let RpcCommand::FallbackGeneration(intent) = &work.command {
        let session = inference_fallback_panel::SessionBinding::current(client);
        return intent.execute(client, &session, work.project.as_ref(), work.revision);
    }
    if let RpcCommand::Equivalence(action) = &work.command {
        anyhow::ensure!(
            work.project.is_none() && work.revision.is_none(),
            "Evidence review is global owner scope"
        );
        return crate::equivalence_panel::execute(directory, client, action);
    }
    if let RpcCommand::HandyRegistration(action) = &work.command {
        anyhow::ensure!(
            work.project.is_none() && work.revision.is_none(),
            "Handy registration is not project-scoped"
        );
        return crate::handy_registration_recovery::execute(directory, client, action);
    }
    if let RpcCommand::HandyInspect(inspection) = &work.command {
        anyhow::ensure!(
            work.project.is_none() && work.revision.is_none(),
            "Handy inspection must not carry project edit scope"
        );
        return crate::handy_panel::inspect(client, directory, inspection.clone())
            .map(ResponseBody::HandyInspection);
    }
    let binding = client.recovery_credential_binding();
    if let RpcCommand::DismissPackRecovery(id) = &work.command {
        crate::pack_recovery::dismiss(directory, client.session_id(), &binding, id)?;
        return Ok(ResponseBody::Ack);
    }
    if let RpcCommand::Identified(id, command) = &work.command {
        if matches!(
            command,
            Command::ImportModelPack { .. }
                | Command::SelectModelPack { .. }
                | Command::WithdrawModelPackApproval { .. }
                | Command::RefreshModelCatalog { .. }
                | Command::SelectCatalogModelPack { .. }
        ) {
            return crate::pack_recovery::execute(directory, client, id.clone(), command.clone());
        }
    }
    execute_work(client, work)
}

pub(super) enum RpcCommand {
    Identified(RequestId, Command),
    Wire(Command),
    Edit(EditProposal),
    Repair(crate::repair_panel::RepairRequest),
    HandyInspect(pulsar_protocol::HandyInspectionRequest),
    HandyRegistration(Box<crate::handy_registration_recovery::Action>),
    DismissPackRecovery(RequestId),
    Equivalence(Box<crate::equivalence_panel::Action>),
    FallbackGeneration(Box<inference_fallback_panel::Intent>),
}

pub(super) struct RpcWork {
    pub(super) command: RpcCommand,
    pub(super) project: Option<ProjectId>,
    pub(super) revision: Option<RevisionId>,
}
pub(super) struct RpcReply {
    pub(super) fallback_session: Option<inference_fallback_panel::SessionBinding>,
    pub(super) fallback_binding: Option<inference_fallback_panel::Binding>,
    pub(super) equivalence_action: Option<Box<crate::equivalence_panel::Action>>,
    pub(super) equivalence_scope: Option<crate::equivalence_panel::Scope>,
    pub(super) equivalence_recovery:
        Option<Result<Vec<crate::inference_equivalence_recovery::Entry>, String>>,
    pub(super) handy_action: Option<Box<crate::handy_registration_recovery::Action>>,
    pub(super) handy_recovery:
        Option<Result<Vec<crate::handy_registration_recovery::RecoveryEntry>, String>>,
    pub(super) response: Result<ResponseBody, String>,
    pub(super) repair_binding: Option<(ProjectId, RevisionId)>,
    pub(super) frame: Option<Result<DecodedFrame, String>>,
    pub(super) previous_frame: Option<Result<DecodedFrame, String>>,
    pub(super) recovery: Result<Vec<crate::pack_recovery::PackRecoveryEntry>, String>,
}
