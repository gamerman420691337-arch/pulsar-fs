//! Explicit authored gap proposals through the ordinary values upload broker.
use crate::{motion::ResponseBody, EngineApi};
use anyhow::{bail, ensure, Result};
use pulsar_protocol::*;

pub(crate) fn propose(
    api: &mut impl EngineApi,
    project: ProjectId,
    revision: RevisionId,
    axis: Axis,
    gaps: &[TimeRange],
) -> Result<crate::motion::CandidateSnapshot> {
    let snapshot = crate::get_snapshot(api, &project)?;
    ensure!(
        snapshot.project_id == project && snapshot.revision == revision,
        "gap recipe requires the exact current project revision"
    );
    let ResponseBody::Diagnostics(diagnostics) = api.execute(
        Command::Diagnostics {
            candidate_id: None,
            maximum_speed_percent_per_second: None,
        },
        Some(project.clone()),
        None,
    )?
    else {
        bail!("gap protection diagnostics response differs")
    };
    ensure!(
        diagnostics.project_id == project && diagnostics.revision == revision,
        "gap protection snapshot changed"
    );
    let recipe = resolve_selected_gaps(
        &snapshot.program,
        axis,
        gaps,
        &diagnostics.protected_regions,
    )?;
    let label = format!(
        "gap-resolution-v1 linear-interior/nearest-edge-hold; selected={}",
        recipe.filled.len()
    );
    let candidate = api.upload_edit(project.clone(), revision, &recipe.program, &label)?;
    ensure!(
        candidate.project_id == project && candidate.base_revision == revision,
        "gap candidate scope differs"
    );
    ensure!(
        candidate.job_id.is_none() && candidate.program.as_ref() == &recipe.program,
        "gap candidate values or evidence differ from the checked recipe"
    );
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::{fixture_descriptor, CandidateSnapshot, ProjectSnapshot};
    use std::sync::Arc;
    fn gap() -> TimeRange {
        TimeRange::new(
            ProjectTime::from_nanos(150_000_000),
            ProjectTime::from_nanos(250_000_000),
        )
        .unwrap()
    }
    fn program() -> MotionProgram {
        MotionProgram::new(vec![MotionTrack::with_gaps(
            Axis::Stroke,
            [(100_000_000, 0.2), (300_000_000, 0.8)]
                .into_iter()
                .map(|(t, p)| {
                    MotionAction::new(
                        ProjectTime::from_nanos(t),
                        NormalizedPosition::new(p).unwrap(),
                        EvidenceKind::Inferred,
                    )
                    .unwrap()
                })
                .collect(),
            vec![gap()],
        )
        .unwrap()])
        .unwrap()
    }
    struct Api {
        revision: u64,
        protected: bool,
        uploads: usize,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            let project = project.unwrap();
            let revision = RevisionId::new(self.revision);
            match command {
                Command::GetSnapshot => {
                    let p = program();
                    Ok(ResponseBody::Project(Box::new(ProjectSnapshot {
                        project_id: project.clone(),
                        revision,
                        name: "gap fixture".into(),
                        motion: fixture_descriptor(
                            &p,
                            MotionBinding::ProjectRevision {
                                project_id: project,
                                revision,
                            },
                        ),
                        program: Arc::new(p),
                        sources: vec![],
                        placements: vec![],
                    })))
                }
                Command::Diagnostics { .. } => Ok(ResponseBody::Diagnostics(DiagnosticsReport {
                    project_id: project,
                    revision,
                    candidate_id: None,
                    base_revision: None,
                    protected_regions: if self.protected {
                        vec![ProtectedRegion {
                            axis: Some(Axis::Stroke),
                            range: gap(),
                        }]
                    } else {
                        vec![]
                    },
                    segment_speed: None,
                    issues: vec![],
                })),
                _ => panic!("gap proposal must not commit or export"),
            }
        }
        fn upload_edit(
            &mut self,
            project: ProjectId,
            revision: RevisionId,
            p: &MotionProgram,
            _: &str,
        ) -> Result<CandidateSnapshot> {
            self.uploads += 1;
            let id = CandidateId::new("gap-proposal").unwrap();
            Ok(CandidateSnapshot {
                project_interval: None,
                candidate_id: id.clone(),
                project_id: project.clone(),
                base_revision: revision,
                motion: fixture_descriptor(
                    p,
                    MotionBinding::Candidate {
                        project_id: project,
                        candidate_id: id,
                        base_revision: revision,
                    },
                ),
                program: Arc::new(p.clone()),
                job_id: None,
                review: vec![],
            })
        }
    }
    #[test]
    fn recipe_proposes_without_committing_and_refuses_before_upload() {
        let project = ProjectId::new("gap-project").unwrap();
        let mut api = Api {
            revision: 1,
            protected: false,
            uploads: 0,
        };
        let c = propose(
            &mut api,
            project.clone(),
            RevisionId::new(1),
            Axis::Stroke,
            &[gap()],
        )
        .unwrap();
        assert!(c.program.track(Axis::Stroke).unwrap().gaps().is_empty());
        assert_eq!(api.uploads, 1);
        api.uploads = 0;
        assert!(propose(
            &mut api,
            project.clone(),
            RevisionId::new(0),
            Axis::Stroke,
            &[gap()]
        )
        .is_err());
        assert_eq!(api.uploads, 0);
        api.protected = true;
        assert!(propose(
            &mut api,
            project,
            RevisionId::new(1),
            Axis::Stroke,
            &[gap()]
        )
        .is_err());
        assert_eq!(api.uploads, 0);
    }
}
