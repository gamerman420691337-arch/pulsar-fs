//! Disposable desktop presentation; durable requests use the shared private intent store.
use crate::{
    assistant_client::{self, Intent},
    motion::ResponseBody,
    session::{EngineApi, SessionClient},
};
use eframe::egui;
use pulsar_protocol::*;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
};
type Binding = (ProjectId, RevisionId);
#[derive(Clone)]
struct CreativeDraft {
    enabled: bool,
    axes: Vec<Axis>,
    seconds: f64,
    sample_hz: u32,
    max_amplitude: f64,
}
impl Default for CreativeDraft {
    fn default() -> Self {
        Self {
            enabled: false,
            axes: vec![Axis::Stroke],
            seconds: 2.0,
            sample_hz: 8,
            max_amplitude: 0.1,
        }
    }
}
impl CreativeDraft {
    fn scope(&self) -> anyhow::Result<Option<CreativePatternScope>> {
        if !self.enabled {
            return Ok(None);
        }
        anyhow::ensure!(
            self.seconds.is_finite() && self.seconds > 0.0 && self.seconds <= 86_400.0,
            "Duration must be positive and at most one day"
        );
        let scope = CreativePatternScope {
            axes: self.axes.clone(),
            duration: ProjectTime::from_nanos((self.seconds * 1_000_000_000.0).round() as i64),
            sample_hz: self.sample_hz,
            max_amplitude: self.max_amplitude,
        };
        scope.validate()?;
        Ok(Some(scope))
    }
}
#[derive(Clone)]
struct Prepared {
    path: PathBuf,
    digest: String,
    request: RequestId,
    spec: AssistantStart,
    revision: RevisionId,
}
impl Prepared {
    fn capture(path: PathBuf, intent: &Intent) -> anyhow::Result<Self> {
        Ok(Self {
            path,
            digest: format!("{:x}", Sha256::digest(serde_json::to_vec(intent)?)),
            request: intent.request_id.clone(),
            spec: intent.spec.clone(),
            revision: intent.revision,
        })
    }
}
#[derive(Clone)]
struct PreparedGeneration {
    path: PathBuf,
    digest: String,
    intent: crate::assistant_generation_intent::GenerationIntent,
}
impl PreparedGeneration {
    fn capture(
        path: PathBuf,
        intent: crate::assistant_generation_intent::GenerationIntent,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            path,
            digest: intent.digest()?,
            intent,
        })
    }
}
#[derive(Clone)]
struct PreparedEdit {
    path: PathBuf,
    digest: String,
    intent: crate::assistant_edit_intent::EditIntent,
}
impl PreparedEdit {
    fn capture(
        path: PathBuf,
        intent: crate::assistant_edit_intent::EditIntent,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            path,
            digest: intent.digest()?,
            intent,
        })
    }
}
#[derive(Clone)]
struct GenerationDraft {
    path: PathBuf,
    request: RequestId,
    report: AssistantReport,
    source: SourceVersionId,
    preset: String,
    settings: GenerationSettings,
    model: Option<PathBuf>,
}
#[derive(Clone)]
enum Action {
    Profiles,
    Targets,
    Recipes,
    ApplyEdit {
        report: Box<AssistantReport>,
        request: RequestId,
        approved: bool,
    },
    PrepareEdit {
        path: PathBuf,
        request: RequestId,
        report: Box<AssistantReport>,
    },
    RestoreEdit(PathBuf),
    ApplySavedEdit {
        prepared: Box<PreparedEdit>,
        approved: bool,
    },
    Sources,
    GenerationStatus {
        job_id: JobId,
        base_revision: RevisionId,
    },
    PrepareGeneration(Box<GenerationDraft>),
    PrepareCreative {
        path: PathBuf,
        request: RequestId,
        report: Box<AssistantReport>,
    },
    RestoreGeneration(PathBuf),
    Generate {
        prepared: Box<PreparedGeneration>,
        approved: bool,
    },
    Explain(Box<AssistantReport>),
    Prepare {
        path: PathBuf,
        request: RequestId,
        spec: AssistantStart,
        image_source: Option<SourceVersionId>,
    },
    Restore(PathBuf),
    Start {
        prepared: Prepared,
        approved: bool,
    },
    Resolve(Prepared),
    Status(AttemptId),
    List,
    Cancel {
        attempt: AttemptId,
        request: RequestId,
    },
}
#[derive(Clone)]
struct Work {
    binding: Binding,
    action: Action,
}
enum Value {
    PreparedEdit(Box<PreparedEdit>),
    PreparedGeneration(Box<PreparedGeneration>),
    Prepared(Box<Prepared>),
    Explanation(String),
    Response(ResponseBody),
}
struct Reply {
    work: Work,
    result: Result<Value, String>,
}
struct Namespace<'a> {
    endpoint: &'a Path,
    session: &'a SessionId,
    credential: &'a str,
}
fn checked_intent(path: &Path, ns: &Namespace<'_>, binding: &Binding) -> anyhow::Result<Intent> {
    let intent = assistant_client::load(path)?;
    anyhow::ensure!(
        intent.endpoint == ns.endpoint
            && &intent.session == ns.session
            && intent.credential_binding == ns.credential
            && intent.project == binding.0,
        "Saved assistant request belongs to a different connection, project or revision"
    );
    Ok(intent)
}
fn execute(client: &mut impl EngineApi, ns: &Namespace<'_>, work: &Work) -> anyhow::Result<Value> {
    let project = Some(work.binding.0.clone());
    let body = match &work.action {
        Action::PrepareEdit {
            path,
            request,
            report,
        } => {
            anyhow::ensure!(
                report.project_id == work.binding.0
                    && report.base_revision == work.binding.1
                    && report.session_id == *ns.session,
                "edit proposal belongs to another owner/project/revision"
            );
            let ResponseBody::AssistantEditRecipes(recipes) =
                client.execute(Command::AssistantEditRecipes, project, None)?
            else {
                anyhow::bail!("expected current recipes");
            };
            let Some(AssistantProposal::ApplyEditRecipe { recipe_id }) = &report.proposal else {
                anyhow::bail!("edit proposal required");
            };
            let recipe = recipes
                .into_iter()
                .find(|r| &r.recipe_id == recipe_id)
                .ok_or_else(|| anyhow::anyhow!("selected edit recipe is absent"))?;
            let intent = crate::assistant_edit_intent::EditIntent::from_report(
                ns.endpoint,
                ns.session,
                ns.credential,
                request.clone(),
                report,
                recipe,
            )?;
            crate::assistant_edit_intent::save(path, &intent)?;
            return Ok(Value::PreparedEdit(Box::new(PreparedEdit::capture(
                path.clone(),
                intent,
            )?)));
        }
        Action::RestoreEdit(path) => {
            let intent = crate::assistant_edit_intent::load(path)?;
            intent.check_namespace(ns.endpoint, ns.session, ns.credential)?;
            anyhow::ensure!(
                intent.project == work.binding.0,
                "saved edit belongs to another project"
            );
            return Ok(Value::PreparedEdit(Box::new(PreparedEdit::capture(
                path.clone(),
                intent,
            )?)));
        }
        Action::ApplySavedEdit { prepared, approved } => {
            anyhow::ensure!(
                prepared.intent.project == work.binding.0,
                "saved edit belongs to another project"
            );
            crate::assistant_edit_intent::dispatch(
                client,
                ns.endpoint,
                ns.session,
                ns.credential,
                &prepared.path,
                Some(&prepared.digest),
                *approved,
            )?
        }
        Action::Recipes => {
            let body = client.execute(Command::AssistantEditRecipes, project, None)?;
            let ResponseBody::AssistantEditRecipes(recipes) = &body else {
                anyhow::bail!("expected edit recipes");
            };
            anyhow::ensure!(recipes.len() <= 12, "too many recipes");
            for recipe in recipes {
                recipe.validate()?;
                anyhow::ensure!(
                    recipe.project_id == work.binding.0 && recipe.revision == work.binding.1,
                    "recipe belongs to another view"
                );
            }
            body
        }
        Action::ApplyEdit {
            report,
            request,
            approved,
        } => {
            anyhow::ensure!(*approved, "explicit edit approval required");
            anyhow::ensure!(
                report.project_id == work.binding.0 && report.session_id == *ns.session,
                "edit proposal belongs to another owner/project"
            );
            client.execute_identified(
                request.clone(),
                Command::ApplyAssistantEdit {
                    approval: AssistantEditApproval::from_report(report)?,
                },
                project,
                Some(report.base_revision),
            )?
        }
        Action::Targets => {
            let body = client.execute(Command::AssistantTargets, project, None)?;
            let ResponseBody::AssistantTargets(catalog) = &body else {
                anyhow::bail!("expected target catalog");
            };
            catalog.validate()?;
            anyhow::ensure!(
                catalog.project_id == work.binding.0
                    && catalog.revision == work.binding.1
                    && catalog.session_id == *ns.session,
                "target catalog belongs to another view or session"
            );
            body
        }
        Action::Sources => client.execute(Command::GetSnapshot, project, None)?,
        Action::GenerationStatus { job_id, .. } => client.execute(
            Command::JobStatus {
                job_id: job_id.clone(),
            },
            project,
            None,
        )?,
        Action::PrepareCreative {
            path,
            request,
            report,
        } => {
            anyhow::ensure!(
                report.project_id == work.binding.0
                    && report.base_revision == work.binding.1
                    && report.session_id == *ns.session,
                "creative proposal belongs to another view or session"
            );
            let approval = AssistantCreativeApproval::from_report(report)?;
            let ResponseBody::Project(current) =
                client.execute(Command::GetSnapshot, project, None)?
            else {
                anyhow::bail!("expected current project");
            };
            anyhow::ensure!(
                current.project_id == work.binding.0 && current.revision == work.binding.1,
                "current view differs"
            );
            let Some(AssistantProposal::SynthesizePatterns { patterns }) = &report.proposal else {
                anyhow::bail!("synthetic proposal required");
            };
            let intent = crate::assistant_generation_intent::GenerationIntent {
                schema_version: 1,
                endpoint: ns.endpoint.to_path_buf(),
                session: ns.session.clone(),
                credential_binding: ns.credential.into(),
                request_id: request.clone(),
                project: work.binding.0.clone(),
                revision: work.binding.1,
                command: Command::GenerateCreativeWithAssistant { approval },
                proposed_patterns: Some(patterns.clone()),
                proposed_stride: None,
                proposed_target: None,
            };
            crate::assistant_generation_intent::save(path, &intent)?;
            return Ok(Value::PreparedGeneration(Box::new(
                PreparedGeneration::capture(path.clone(), intent)?,
            )));
        }
        Action::PrepareGeneration(draft) => {
            anyhow::ensure!(
                draft.report.project_id == work.binding.0
                    && draft.report.base_revision == work.binding.1
                    && draft.report.session_id == *ns.session,
                "generation proposal belongs to another view or session"
            );
            let ResponseBody::Project(current) =
                client.execute(Command::GetSnapshot, project, None)?
            else {
                anyhow::bail!("expected current project")
            };
            anyhow::ensure!(
                current.project_id == work.binding.0
                    && current.revision == work.binding.1
                    && current
                        .sources
                        .iter()
                        .any(|s| s.source_version == draft.source && s.kind == SourceKind::Media),
                "selected media source or current view differs"
            );
            let command = assistant_client::approved_generation_command(
                &draft.report,
                draft.source.clone(),
                draft.preset.clone(),
                draft.settings.clone(),
                draft.model.clone(),
                true,
            )?;
            let intent = crate::assistant_generation_intent::GenerationIntent {
                proposed_patterns: None,
                schema_version: 1,
                endpoint: ns.endpoint.to_path_buf(),
                session: ns.session.clone(),
                credential_binding: ns.credential.into(),
                request_id: draft.request.clone(),
                project: work.binding.0.clone(),
                revision: work.binding.1,
                proposed_stride: match &draft.report.proposal {
                    Some(AssistantProposal::SetSampleStride { stride }) => Some(*stride),
                    _ => None,
                },
                proposed_target: match &draft.report.proposal {
                    Some(AssistantProposal::SelectTarget { target_id }) => Some(target_id.clone()),
                    _ => None,
                },
                command,
            };
            crate::assistant_generation_intent::save(&draft.path, &intent)?;
            return Ok(Value::PreparedGeneration(Box::new(
                PreparedGeneration::capture(draft.path.clone(), intent)?,
            )));
        }
        Action::RestoreGeneration(path) => {
            let intent = crate::assistant_generation_intent::load(path)?;
            intent.check_namespace(ns.endpoint, ns.session, ns.credential)?;
            anyhow::ensure!(
                intent.project == work.binding.0,
                "saved generation belongs to another project"
            );
            return Ok(Value::PreparedGeneration(Box::new(
                PreparedGeneration::capture(path.clone(), intent)?,
            )));
        }
        Action::Generate { prepared, approved } => {
            anyhow::ensure!(
                prepared.intent.project == work.binding.0,
                "saved generation belongs to another project"
            );
            crate::assistant_generation_intent::dispatch(
                client,
                ns.endpoint,
                ns.session,
                ns.credential,
                &prepared.path,
                Some(&prepared.digest),
                *approved,
            )?
        }
        Action::Explain(report) => {
            return Ok(Value::Explanation(
                assistant_client::recorded_review_explanation(
                    client,
                    ns.session,
                    &work.binding.0,
                    work.binding.1,
                    report,
                )?,
            ))
        }
        Action::Profiles => client.execute(Command::AssistantProfiles, project, None)?,
        Action::Prepare {
            path,
            request,
            spec,
            image_source,
        } => {
            let spec = assistant_client::prepare_start(
                client,
                ns.session,
                &work.binding.0,
                work.binding.1,
                spec.clone(),
                image_source.as_ref(),
            )?;
            let intent = Intent {
                schema_version: 1,
                endpoint: ns.endpoint.to_path_buf(),
                session: ns.session.clone(),
                credential_binding: ns.credential.into(),
                request_id: request.clone(),
                project: work.binding.0.clone(),
                revision: work.binding.1,
                spec,
            };
            assistant_client::save(path, &intent)?;
            return Ok(Value::Prepared(Box::new(Prepared::capture(
                path.clone(),
                &intent,
            )?)));
        }
        Action::Restore(path) => {
            return Ok(Value::Prepared(Box::new(Prepared::capture(
                path.clone(),
                &checked_intent(path, ns, &work.binding)?,
            )?)))
        }
        Action::Start { prepared, approved } => {
            anyhow::ensure!(
                *approved,
                "Approve local inference before starting or retrying"
            );
            let intent = checked_intent(&prepared.path, ns, &work.binding)?;
            anyhow::ensure!(
                intent.revision == work.binding.1,
                "Saved assistant start belongs to an earlier revision"
            );
            anyhow::ensure!(
                Prepared::capture(prepared.path.clone(), &intent)?.digest == prepared.digest,
                "Saved assistant request changed; original retry was not dispatched"
            );
            assistant_client::execute_intent(
                client,
                ns.endpoint,
                ns.session,
                ns.credential,
                intent,
                *approved,
            )?
        }
        Action::Resolve(prepared) => {
            let intent = checked_intent(&prepared.path, ns, &work.binding)?;
            anyhow::ensure!(
                Prepared::capture(prepared.path.clone(), &intent)?.digest == prepared.digest,
                "Saved assistant request changed; resolution was not dispatched"
            );
            assistant_client::resolve_intent(
                client,
                ns.endpoint,
                ns.session,
                ns.credential,
                intent,
            )?
        }
        Action::Status(attempt) => client.execute(
            Command::AssistantStatus {
                attempt_id: attempt.clone(),
            },
            project,
            None,
        )?,
        Action::List => client.execute(Command::ListAssistantAttempts, project, None)?,
        Action::Cancel { attempt, request } => client.execute_identified(
            request.clone(),
            Command::CancelAssistant {
                attempt_id: attempt.clone(),
            },
            project,
            None,
        )?,
    };
    Ok(Value::Response(body))
}
pub(crate) struct AssistantPanel {
    sender: SyncSender<Work>,
    receiver: Receiver<Reply>,
    binding: Option<Binding>,
    directory: Option<PathBuf>,
    busy: bool,
    profiles: Vec<AssistantProfile>,
    selected: Option<AttemptId>,
    prompt: String,
    creative: CreativeDraft,
    pattern_reviews: crate::pattern_review::PatternReviews,
    review_focus: String,
    target_focus: String,
    edit_focus: String,
    recipes: Vec<AssistantEditRecipe>,
    edit_approved: bool,
    edit_path: String,
    edit_prepared: Option<PreparedEdit>,
    edit_saved_approved: bool,
    edit_result: Option<(CandidateId, RevisionId)>,
    edit_event: Option<crate::motion::CandidateSnapshot>,
    targets: Vec<AssistantObservedTarget>,
    explanation: Option<String>,
    intent_path: String,
    prepared: Option<Prepared>,
    approved: bool,
    report: Option<AssistantReport>,
    attempts: Vec<AssistantReport>,
    retry: Option<Work>,
    error: Option<String>,
    sources: Vec<SourceSummary>,
    image_source: Option<SourceVersionId>,
    generation_source: Option<SourceVersionId>,
    generation_settings: String,
    generation_model: String,
    generation_preset: String,
    generation_path: String,
    generation_prepared: Option<PreparedGeneration>,
    generation_approved: bool,
    generation_job: Option<JobSnapshot>,
    generation_event: Option<JobSnapshot>,
    last_poll: std::time::Instant,
}
impl AssistantPanel {
    pub(crate) fn new(endpoint: PathBuf, bootstrap: PathBuf, repaint: egui::Context) -> Self {
        let directory = bootstrap.parent().map(Path::to_path_buf);
        let (sender, requests) = mpsc::sync_channel::<Work>(4);
        let (replies, receiver) = mpsc::sync_channel(4);
        std::thread::spawn(move || {
            let mut client = None;
            while let Ok(work) = requests.recv() {
                let result = (|| -> anyhow::Result<Value> {
                    if client.is_none() {
                        client = Some(SessionClient::connect(&endpoint, &bootstrap)?);
                    }
                    let client = client.as_mut().expect("connected session");
                    let session = client.session_id().clone();
                    let credential = client.recovery_credential_binding();
                    execute(
                        client,
                        &Namespace {
                            endpoint: &endpoint,
                            session: &session,
                            credential: &credential,
                        },
                        &work,
                    )
                })()
                .map_err(|e| e.to_string());
                if replies.send(Reply { work, result }).is_err() {
                    break;
                }
                repaint.request_repaint();
            }
        });
        Self {
            sender,
            receiver,
            binding: None,
            directory,
            busy: false,
            profiles: vec![],
            selected: None,
            prompt: String::new(),
            creative: CreativeDraft::default(),
            pattern_reviews: Default::default(),
            review_focus: String::new(),
            target_focus: String::new(),
            edit_focus: String::new(),
            recipes: vec![],
            edit_approved: false,
            edit_path: String::new(),
            edit_prepared: None,
            edit_saved_approved: false,
            edit_result: None,
            edit_event: None,
            targets: vec![],
            explanation: None,
            intent_path: String::new(),
            prepared: None,
            approved: false,
            report: None,
            attempts: vec![],
            retry: None,
            error: None,
            sources: vec![],
            image_source: None,
            generation_source: None,
            generation_settings: String::new(),
            generation_model: String::new(),
            generation_preset: "fast".into(),
            generation_path: String::new(),
            generation_prepared: None,
            generation_approved: false,
            generation_job: None,
            generation_event: None,
            last_poll: std::time::Instant::now(),
        }
    }
    fn send(&mut self, action: Action) {
        if self.busy {
            return;
        }
        if let Some(pending) = &self.retry {
            let involves_generation = matches!(
                pending.action,
                Action::Generate { .. } | Action::ApplyEdit { .. } | Action::ApplySavedEdit { .. }
            ) || matches!(
                action,
                Action::Generate { .. }
                    | Action::ApplyEdit { .. }
                    | Action::ApplySavedEdit { .. }
                    | Action::PrepareGeneration(_)
                    | Action::PrepareCreative { .. }
                    | Action::PrepareEdit { .. }
                    | Action::RestoreEdit(_)
                    | Action::RestoreGeneration(_)
            );
            let mutation = matches!(
                action,
                Action::Generate { .. }
                    | Action::ApplyEdit { .. }
                    | Action::ApplySavedEdit { .. }
                    | Action::Start { .. }
                    | Action::Cancel { .. }
                    | Action::Resolve(_)
                    | Action::PrepareGeneration(_)
                    | Action::PrepareCreative { .. }
                    | Action::PrepareEdit { .. }
                    | Action::RestoreEdit(_)
                    | Action::RestoreGeneration(_)
            );
            if involves_generation && mutation {
                let exact = matches!((&pending.action, &action), (Action::ApplyEdit { report:old, request:old_id, .. }, Action::ApplyEdit { report:new, request:new_id, approved:true }) if old_id == new_id && old.attempt_id == new.attempt_id && old.project_id == new.project_id && old.session_id == new.session_id && old.base_revision == new.base_revision && serde_json::to_vec(&old.proposal).ok() == serde_json::to_vec(&new.proposal).ok() && old.context_sha256 == new.context_sha256)
                    || matches!((&pending.action, &action), (Action::ApplySavedEdit { prepared: old, .. }, Action::ApplySavedEdit { prepared: new, approved: true }) if old.digest == new.digest)
                    || matches!((&pending.action, &action), (Action::Generate { prepared: old, .. }, Action::Generate { prepared: new, approved: true }) if old.digest == new.digest);
                if !exact {
                    self.error = Some("Reconcile the pending original request before another mutation or generation approval".into());
                    return;
                }
            }
        }
        if let Some(binding) = self.binding.clone() {
            let work = Work { binding, action };
            match self.sender.try_send(work.clone()) {
                Ok(()) => {
                    self.busy = true;
                    if matches!(
                        work.action,
                        Action::Explain(_)
                            | Action::Status(_)
                            | Action::Start { .. }
                            | Action::Restore(_)
                            | Action::Prepare { .. }
                            | Action::Resolve(_)
                    ) {
                        self.explanation = None;
                    }
                    if matches!(
                        work.action,
                        Action::Start { .. }
                            | Action::Cancel { .. }
                            | Action::Resolve(_)
                            | Action::Generate { .. }
                            | Action::ApplyEdit { .. }
                            | Action::ApplySavedEdit { .. }
                    ) {
                        self.retry = Some(work);
                    }
                    self.error = None;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
    fn receive(&mut self, reply: Reply) {
        self.busy = false;
        if self.binding.as_ref() != Some(&reply.work.binding) {
            return;
        }
        let mutation = matches!(
            reply.work.action,
            Action::Start { .. }
                | Action::Cancel { .. }
                | Action::Resolve(_)
                | Action::Generate { .. }
                | Action::ApplyEdit { .. }
                | Action::ApplySavedEdit { .. }
        );
        match reply.result {
            Ok(Value::PreparedEdit(prepared)) => {
                self.edit_prepared = Some(*prepared);
                self.edit_saved_approved = false;
            }
            Ok(Value::PreparedGeneration(prepared)) => {
                self.generation_prepared = Some(*prepared);
                self.generation_approved = false;
                self.generation_job = None;
            }
            Ok(Value::Response(ResponseBody::AssistantEditRecipes(recipes))) => {
                self.recipes = recipes
            }
            Ok(Value::Response(ResponseBody::Candidate(candidate))) => {
                if matches!(&reply.work.action, Action::ApplyEdit { report, .. } if candidate.project_id == report.project_id && candidate.base_revision == report.base_revision)
                    || matches!(&reply.work.action, Action::ApplySavedEdit { prepared, .. } if candidate.project_id == prepared.intent.project && candidate.base_revision == prepared.intent.revision)
                {
                    self.edit_result =
                        Some((candidate.candidate_id.clone(), candidate.base_revision));
                    self.edit_event = Some(*candidate);
                    self.edit_saved_approved = false;
                    self.edit_approved = false;
                    self.retry = None;
                } else {
                    self.error = Some("Edit candidate differs from approved proposal".into());
                }
            }
            Ok(Value::Response(ResponseBody::AssistantTargets(catalog))) => {
                if catalog.project_id == reply.work.binding.0
                    && catalog.revision == reply.work.binding.1
                {
                    self.targets = catalog.targets;
                } else {
                    self.error = Some("Target catalog differs from current view".into());
                }
            }
            Ok(Value::Response(ResponseBody::Project(project))) => {
                if project.project_id == reply.work.binding.0
                    && project.revision == reply.work.binding.1
                {
                    self.sources = project
                        .sources
                        .into_iter()
                        .filter(|s| s.kind == SourceKind::Media)
                        .collect();
                    self.image_source = self
                        .image_source
                        .take()
                        .filter(|id| self.sources.iter().any(|s| &s.source_version == id));
                    self.generation_source = self
                        .generation_source
                        .take()
                        .filter(|id| self.sources.iter().any(|s| &s.source_version == id));
                } else {
                    self.error = Some("Source list belongs to another project revision".into());
                }
            }
            Ok(Value::Response(ResponseBody::Job(job))) => {
                let expected = match &reply.work.action {
                    Action::Generate { prepared, .. } => {
                        job.project_id == prepared.intent.project
                            && job.base_revision == prepared.intent.revision
                    }
                    Action::GenerationStatus {
                        job_id,
                        base_revision,
                    } => {
                        job.project_id == reply.work.binding.0
                            && &job.job_id == job_id
                            && job.base_revision == *base_revision
                    }
                    _ => false,
                };
                if expected {
                    self.generation_event = Some(job.clone());
                    self.generation_job = Some(job);
                    if mutation {
                        self.generation_approved = false;
                        self.retry = None;
                    }
                } else {
                    self.error = Some("Generation reply belongs to another request view".into());
                }
            }
            Ok(Value::Explanation(text)) => {
                if let Action::Explain(requested) = &reply.work.action {
                    if self.report.as_ref().is_some_and(|current| {
                        current.attempt_id == requested.attempt_id
                            && current.context_sha256 == requested.context_sha256
                    }) {
                        self.explanation = Some(text);
                    }
                }
            }
            Ok(Value::Prepared(value)) => {
                self.prepared = Some(*value);
                self.approved = false;
                self.retry = None;
            }
            Ok(Value::Response(ResponseBody::AssistantStartResolution(value))) => {
                self.retry = None;
                match value.outcome {
                    AssistantStartOutcome::Accepted { report } => {
                        self.explanation = None;
                        self.edit_approved = false;
                        self.report = Some(*report);
                        self.error = None;
                    }
                    AssistantStartOutcome::Withdrawn => {
                        self.prepared = None;
                        self.approved = false;
                        self.report = None;
                        self.explanation = None;
                        self.error = Some("The unpublished original request ID was permanently withdrawn. Save a new request to run inference.".into());
                    }
                }
            }
            Ok(Value::Response(ResponseBody::AssistantProfiles(profiles))) => {
                self.selected = self
                    .selected
                    .take()
                    .filter(|id| profiles.iter().any(|p| &p.profile_id == id));
                self.profiles = profiles;
            }
            Ok(Value::Response(ResponseBody::Assistant(report))) => {
                if self.report.as_ref().is_none_or(|old| {
                    old.attempt_id != report.attempt_id
                        || old.context_sha256 != report.context_sha256
                        || serde_json::to_vec(&old.proposal).ok()
                            != serde_json::to_vec(&report.proposal).ok()
                }) {
                    self.edit_approved = false;
                }
                self.explanation = None;
                self.report = Some(*report);
                self.last_poll = std::time::Instant::now();
                if mutation {
                    self.retry = None;
                }
            }
            Ok(Value::Response(ResponseBody::AssistantAttempts(reports))) => {
                self.attempts = reports;
            }
            Ok(_) => self.error = Some("Unexpected assistant response".into()),
            Err(error) => {
                self.error = Some(error);
                if mutation {
                    self.retry = Some(reply.work);
                }
            }
        }
    }
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, binding: Option<Binding>) {
        if binding != self.binding {
            self.binding = binding;
            self.profiles.clear();
            self.selected = None;
            self.prepared = None;
            self.report = None;
            self.explanation = None;
            self.review_focus.clear();
            self.target_focus.clear();
            self.edit_focus.clear();
            self.recipes.clear();
            self.edit_approved = false;
            self.edit_event = None;
            self.edit_path.clear();
            self.edit_prepared = None;
            self.edit_saved_approved = false;
            self.edit_result = None;
            self.targets.clear();
            self.attempts.clear();
            self.retry = None;
            self.approved = false;
            self.prompt.clear();
            self.sources.clear();
            self.image_source = None;
            self.generation_source = None;
            self.generation_prepared = None;
            self.generation_approved = false;
            self.generation_job = None;
            self.generation_event = None;
            self.generation_path.clear();

            self.error = None;
        }
        while let Ok(reply) = self.receiver.try_recv() {
            self.receive(reply);
        }
        if let Some(report) = self
            .report
            .as_ref()
            .filter(|r| r.state == AssistantState::Running)
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs(1));
            if !self.busy
                && self.error.is_none()
                && self.last_poll.elapsed() >= std::time::Duration::from_secs(1)
            {
                let attempt = report.attempt_id.clone();
                self.last_poll = std::time::Instant::now();
                self.send(Action::Status(attempt));
            }
        }
        ui.separator();
        ui.heading("Local assistant");
        ui.label("Ask for a proposal using a local model. Review the result before deciding what to do next.");
        let Some(binding) = self.binding.clone() else {
            ui.label("Open a project to use the assistant.");
            return;
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Refresh local models"))
                .clicked()
            {
                self.send(Action::Profiles);
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("List attempts"))
                .clicked()
            {
                self.send(Action::List);
            }
        });
        if self.profiles.is_empty() {
            ui.label("No models loaded in this view. Refresh to check the engine's approved local models.");
        }
        for profile in &self.profiles {
            ui.radio_value(
                &mut self.selected,
                Some(profile.profile_id.clone()),
                format!("{} · {} threads", profile.profile_id, profile.cpu_threads),
            );
        }
        let vision = self
            .profiles
            .iter()
            .find(|p| Some(&p.profile_id) == self.selected.as_ref())
            .is_some_and(|p| p.vision_projector.is_some());
        if vision {
            ui.label("Choose an imported single-frame image as a theme for synthetic motion. A still image does not supply observed timing or motion.");
            if ui
                .add_enabled(!self.busy, egui::Button::new("Refresh imported images"))
                .clicked()
            {
                self.send(Action::Sources);
            }
            egui::ComboBox::from_id_salt("assistant-image-source")
                .selected_text(
                    self.image_source
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "Select an imported image".into()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.image_source, None, "Select an imported image");
                    for source in &self.sources {
                        ui.selectable_value(
                            &mut self.image_source,
                            Some(source.source_version.clone()),
                            format!("{} · {}", source.label, source.source_version),
                        );
                    }
                });
            ui.small("Saving checks that the selected source contains exactly one frame. Enable synthetic motion and choose its channels and timing below.");
        }
        ui.add(
            egui::TextEdit::multiline(&mut self.prompt)
                .hint_text("Describe motion, generation settings, edits or review flags")
                .char_limit(8192),
        );
        ui.checkbox(
            &mut self.creative.enabled,
            "Create synthetic motion from this prompt",
        );
        if self.creative.enabled {
            ui.label("Choose the channels and timing allowed for this proposal. Clear target, edit and review selections before saving a synthetic request.");
            ui.horizontal_wrapped(|ui| {
                for axis in Axis::ALL {
                    let mut selected = self.creative.axes.contains(&axis);
                    if ui.checkbox(&mut selected, format!("{axis:?}")).changed() {
                        if selected {
                            self.creative.axes.push(axis);
                        } else {
                            self.creative.axes.retain(|a| *a != axis);
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Duration (seconds)");
                ui.add(
                    egui::DragValue::new(&mut self.creative.seconds)
                        .range(0.001..=86_400.0)
                        .speed(0.1),
                );
                ui.label("Samples per second");
                ui.add(egui::DragValue::new(&mut self.creative.sample_hz).range(1..=1000));
                ui.label("Maximum amplitude");
                ui.add(
                    egui::DragValue::new(&mut self.creative.max_amplitude)
                        .range(0.0..=0.5)
                        .speed(0.01),
                );
            });
        }
        let creative = self.creative.scope();
        if let Err(error) = &creative {
            ui.colored_label(egui::Color32::YELLOW, error.to_string());
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.review_focus)
                .hint_text(
                    "Optional review IDs from Review Status, separated by spaces (up to eight)",
                )
                .char_limit(520),
        );
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Refresh observed targets from accepted preview"),
            )
            .clicked()
        {
            self.send(Action::Targets);
        }
        for target in &self.targets {
            ui.monospace(&target.target_id);
            ui.small(format!(
                "Captured video view: {}",
                serde_json::to_string(&target.video_view).unwrap_or_else(|_| "unavailable".into())
            ));
            let seed = target.seed();
            ui.small(format!(
                "Source {} · frame {} · centre ({:.3}, {:.3}) · detector {}",
                target.context.source_version,
                seed.source_frame_index,
                seed.x,
                seed.y,
                target.model.sha256
            ));
        }
        ui.add(egui::TextEdit::singleline(&mut self.target_focus).hint_text("Optional observed target IDs from this preview, separated by spaces (up to eight)").char_limit(520));
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new("Refresh current edit recipes"),
            )
            .clicked()
        {
            self.send(Action::Recipes);
        }
        for recipe in &self.recipes {
            ui.monospace(&recipe.recipe_id);
            ui.small(format!(
                "{:?} {:?} at revision {} · program {}",
                recipe.operation, recipe.axis, recipe.revision, recipe.program_sha256
            ));
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.edit_focus)
                .hint_text("Optional edit recipe IDs, separated by spaces (up to eight)")
                .char_limit(520),
        );
        let spec = self
            .profiles
            .iter()
            .find(|p| Some(&p.profile_id) == self.selected.as_ref())
            .and_then(|profile| {
                Some(AssistantStart {
                    image: None,
                    creative_patterns: creative.as_ref().ok()?.clone(),
                    edit_focus: self
                        .edit_focus
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect(),
                    target_focus: self
                        .target_focus
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect(),
                    review_focus: self
                        .review_focus
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect(),
                    profile: profile.clone(),
                    user_request: self.prompt.clone(),
                })
            });
        let image_source = if vision {
            self.image_source.clone()
        } else {
            None
        };
        if ui
            .add_enabled(
                !self.busy
                    && self.retry.is_none()
                    && spec.as_ref().is_some_and(|s| {
                        assistant_client::validate_preparation(s, image_source.as_ref()).is_ok()
                    }),
                egui::Button::new("Save request for review"),
            )
            .clicked()
        {
            if let (Some(spec), Some(directory)) = (spec, &self.directory) {
                let request = crate::session::new_request_id();
                let path = directory.join(format!("assistant-intent-{request}.json"));
                self.intent_path = path.to_string_lossy().into_owned();
                self.send(Action::Prepare {
                    path,
                    request,
                    spec,
                    image_source,
                });
            }
        }
        ui.label("Saved request path (also usable after restarting Pulsar)");
        ui.text_edit_singleline(&mut self.intent_path);
        if ui
            .add_enabled(
                !self.busy && self.retry.is_none() && !self.intent_path.is_empty(),
                egui::Button::new("Restore saved request"),
            )
            .clicked()
        {
            self.send(Action::Restore(PathBuf::from(&self.intent_path)));
        }
        if let Some(prepared) = self.prepared.clone() {
            ui.group(|ui| {
                ui.label(format!(
                    "Prepared at project revision {}",
                    prepared.revision
                ));
                ui.monospace(format!("Request {}", prepared.request));
                ui.label(format!(
                    "Model: {}",
                    prepared.spec.profile.model.artifact_id
                ));
                ui.label(&prepared.spec.user_request);
                if let Some(image) = &prepared.spec.image {
                    ui.label(format!("Image theme: {} · {} × {} pixels", image.source_version, image.width, image.height));
                    ui.small(format!("Image content: {}", image.rgb.sha256));
                    ui.small("Image theme only; no observed temporal motion.");
                }
                if let Some(scope) = &prepared.spec.creative_patterns {
                    ui.label(format!("Synthetic channels {:?} · {:.9} seconds · {} samples/sec · amplitude at most {}", scope.axes, scope.duration.as_nanos() as f64 / 1_000_000_000.0, scope.sample_hz, scope.max_amplitude));
                }
                if !prepared.spec.edit_focus.is_empty() {
                    ui.label("Edit recipe IDs selected for this request:");
                    for id in &prepared.spec.edit_focus {
                        ui.monospace(id);
                    }
                }
                if !prepared.spec.target_focus.is_empty() {
                    ui.label("Observed target IDs selected for this request:");
                    for id in &prepared.spec.target_focus {
                        ui.monospace(id);
                    }
                }
                if !prepared.spec.review_focus.is_empty() {
                    ui.label("Recorded review IDs selected for this request:");
                    for id in &prepared.spec.review_focus {
                        ui.monospace(id);
                    }
                }

                ui.small(prepared.path.display().to_string());
                ui.checkbox(&mut self.approved, "Approve this local inference request");
                if ui
                    .add_enabled(
                        !self.busy
                            && self.approved
                            && self.retry.is_none()
                            && prepared.revision == binding.1,
                        egui::Button::new("Start or reconcile exact request"),
                    )
                    .clicked()
                {
                    self.send(Action::Start {
                        prepared: prepared.clone(),
                        approved: self.approved,
                    });
                }
                if ui
                    .add_enabled(
                        !self.busy
                            && !self
                                .retry
                                .as_ref()
                                .is_some_and(|w| matches!(w.action, Action::Cancel { .. })),
                        egui::Button::new("Resolve start; withdraw original ID if never accepted"),
                    )
                    .clicked()
                {
                    self.send(Action::Resolve(prepared.clone()));
                }
            });
        }
        if let Some(retry) = self.retry.clone().filter(|w| {
            matches!(
                w.action,
                Action::Start { .. }
                    | Action::Cancel { .. }
                    | Action::Resolve(_)
                    | Action::Generate { .. }
                    | Action::ApplyEdit { .. }
            )
        }) {
            if ui
                .add_enabled(
                    !self.busy
                        && match retry.action {
                            Action::Generate { .. } => self.generation_approved,
                            Action::ApplyEdit { .. } => self.edit_approved,
                            Action::Start { .. } => self.approved,
                            Action::Cancel { .. } | Action::Resolve(_) => true,
                            _ => false,
                        },
                    egui::Button::new("Retry original request after a lost acknowledgement"),
                )
                .clicked()
            {
                self.send(retry.action);
            }
        }
        let mut select = None;
        for attempt in &self.attempts {
            if ui
                .selectable_label(
                    self.report
                        .as_ref()
                        .is_some_and(|r| r.attempt_id == attempt.attempt_id),
                    format!(
                        "{} · request {} · {:?}",
                        attempt.attempt_id, attempt.request_id, attempt.state
                    ),
                )
                .clicked()
            {
                select = Some(attempt.clone());
            }
        }
        if let Some(report) = select {
            self.explanation = None;
            self.edit_approved = false;
            self.report = Some(report);
        }
        if let Some(report) = self.report.clone() {
            ui.label(format!(
                "Attempt {} · {:?} · base revision {}",
                report.attempt_id, report.state, report.base_revision
            ));
            if report.base_revision != binding.1 {
                ui.label("This result belongs to an earlier project revision.");
            }
            if let Some(proposal) = &report.proposal {
                if let AssistantProposal::SynthesizePatterns { patterns } = proposal {
                    self.pattern_reviews.show(ui, patterns);
                }
                ui.monospace(
                    serde_json::to_string_pretty(proposal)
                        .unwrap_or_else(|_| "Proposal unavailable".into()),
                );
            }
            if matches!(
                report.proposal,
                Some(AssistantProposal::ExplainReview { .. })
            ) && report.base_revision == binding.1
                && ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new("Show recorded review reason and confidence"),
                    )
                    .clicked()
            {
                self.send(Action::Explain(Box::new(report.clone())));
            }
            if let Some(text) = &self.explanation {
                ui.monospace(text);
            }
            if let Some(error) = &report.error {
                ui.label(&error.message);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Refresh attempt"))
                    .clicked()
                {
                    self.send(Action::Status(report.attempt_id.clone()));
                }
                if ui
                    .add_enabled(
                        !self.busy
                            && self.retry.is_none()
                            && matches!(
                                report.state,
                                AssistantState::Running | AssistantState::Interrupted
                            ),
                        egui::Button::new("Cancel this attempt"),
                    )
                    .clicked()
                {
                    self.send(Action::Cancel {
                        attempt: report.attempt_id,
                        request: crate::session::new_request_id(),
                    });
                }
            });
        }
        if let Some(report) = self
            .report
            .clone()
            .filter(|r| matches!(r.proposal, Some(AssistantProposal::ApplyEditRecipe { .. })))
        {
            if ui
                .add_enabled(
                    !self.busy
                        && self.retry.is_none()
                        && report.base_revision == binding.1
                        && self.directory.is_some(),
                    egui::Button::new("Save edit request for review and recovery"),
                )
                .clicked()
            {
                if let Some(directory) = &self.directory {
                    let request = crate::session::new_request_id();
                    let path = directory.join(format!("assistant-edit-{request}.json"));
                    self.edit_path = path.to_string_lossy().into_owned();
                    self.send(Action::PrepareEdit {
                        path,
                        request,
                        report: Box::new(report.clone()),
                    });
                }
            }
            ui.checkbox(
                &mut self.edit_approved,
                "Approve this edit recipe to create a candidate",
            );
            ui.label("Master motion stays unchanged until you review and commit the candidate. Protected segments and gaps are preserved.");
            if ui
                .add_enabled(
                    !self.busy
                        && self.retry.is_none()
                        && self.edit_approved
                        && report.base_revision == binding.1,
                    egui::Button::new("Create approved edit candidate"),
                )
                .clicked()
            {
                self.send(Action::ApplyEdit {
                    report: Box::new(report),
                    request: crate::session::new_request_id(),
                    approved: true,
                });
            }
        }
        ui.label("Saved edit request path");
        ui.text_edit_singleline(&mut self.edit_path);
        if ui
            .add_enabled(
                !self.busy && self.retry.is_none() && !self.edit_path.is_empty(),
                egui::Button::new("Restore saved edit request"),
            )
            .clicked()
        {
            self.send(Action::RestoreEdit(PathBuf::from(&self.edit_path)));
        }
        if let Some(prepared) = self.edit_prepared.clone() {
            ui.group(|ui| {
                ui.label(format!("Request {} · revision {} · {:?} {:?}", prepared.intent.request_id, prepared.intent.revision, prepared.intent.recipe.operation, prepared.intent.recipe.axis));
                ui.monospace(format!("Program {} · recipe {}", prepared.intent.recipe.program_sha256, prepared.intent.recipe.recipe_id));
                ui.label("Restoring does not apply the edit. An exact retry can recover an accepted candidate; fresh edits require the original revision and retained proposal. Commit remains separate.");
                if prepared.intent.revision != binding.1 { ui.label("Historical edit request: only accepted receipt recovery can succeed at a changed revision."); }
                ui.checkbox(&mut self.edit_saved_approved, "Approve this saved edit request");
                let compatible = self.retry.as_ref().is_none_or(|w| matches!(&w.action, Action::ApplySavedEdit { prepared: old, .. } if old.digest == prepared.digest));
                if ui.add_enabled(!self.busy && self.edit_saved_approved && compatible, egui::Button::new("Apply or retry exact saved edit")).clicked() {
                    self.send(Action::ApplySavedEdit { prepared: Box::new(prepared), approved: self.edit_saved_approved });
                }
            });
        }
        if let Some((candidate, revision)) = &self.edit_result {
            ui.label(format!(
                "Accepted edit candidate {candidate} · base revision {revision}"
            ));
            if *revision != binding.1 {
                ui.label("This recovered candidate belongs to an earlier revision. Ordinary rebase/review is required before commit.");
            }
        }
        self.show_generation(ui, &binding);
        if self.retry.is_some() && !self.busy {
            ui.label("The original request is unconfirmed. Reconcile it before preparing or starting another request.");
        }
        if self.busy {
            ui.spinner();
            ui.label("Waiting for the engine…");
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::YELLOW, error);
        }
    }
    pub(crate) fn take_edit_candidate(&mut self) -> Option<crate::motion::CandidateSnapshot> {
        self.edit_event.take()
    }
    pub(crate) fn take_generation_job(&mut self) -> Option<JobSnapshot> {
        self.generation_event.take()
    }
    fn show_generation(&mut self, ui: &mut egui::Ui, binding: &Binding) {
        ui.separator();
        ui.heading("Apply an approved proposal to generation");
        if let Some(report) = self.report.clone().filter(|r| {
            r.state == AssistantState::Completed
                && r.base_revision == binding.1
                && matches!(
                    r.proposal,
                    Some(AssistantProposal::SynthesizePatterns { .. })
                )
        }) {
            ui.label("Review the proposed synthetic patterns above, then save their exact generation request. Commit remains a separate candidate review action.");
            if ui
                .add_enabled(
                    !self.busy && self.retry.is_none() && self.directory.is_some(),
                    egui::Button::new("Save synthetic candidate request for review"),
                )
                .clicked()
            {
                if let Some(directory) = &self.directory {
                    let request = crate::session::new_request_id();
                    let path = directory.join(format!("assistant-generation-{request}.json"));
                    self.generation_path = path.to_string_lossy().into_owned();
                    self.send(Action::PrepareCreative {
                        path,
                        request,
                        report: Box::new(report),
                    });
                }
            }
        }
        if self.report.as_ref().is_none_or(|r| {
            !matches!(
                r.proposal,
                Some(AssistantProposal::SynthesizePatterns { .. })
            )
        }) {
            ui.label("Choose an imported media source and save the complete request before approving the job.");
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Refresh imported media sources"),
                )
                .clicked()
            {
                self.send(Action::Sources);
            }
            for source in &self.sources {
                ui.radio_value(
                    &mut self.generation_source,
                    Some(source.source_version.clone()),
                    &source.label,
                );
            }
            ui.label("Preset");
            ui.text_edit_singleline(&mut self.generation_preset);
            ui.label("Generation settings JSON (targets require a declared detector and a compatible sample stride)");
            ui.add(egui::TextEdit::multiline(&mut self.generation_settings).char_limit(8192));
            ui.label("Optional local model path");
            ui.add(egui::TextEdit::singleline(&mut self.generation_model).char_limit(4096));
            let settings: anyhow::Result<GenerationSettings> =
                if self.generation_settings.trim().is_empty() {
                    Ok(GenerationSettings::default())
                } else {
                    serde_json::from_str(&self.generation_settings).map_err(Into::into)
                };
            let report = self.report.clone().filter(|r| {
                r.state == AssistantState::Completed
                    && r.base_revision == binding.1
                    && matches!(
                        r.proposal,
                        Some(
                            AssistantProposal::SetSampleStride { .. }
                                | AssistantProposal::SelectTarget { .. }
                        )
                    )
            });
            if ui
                .add_enabled(
                    !self.busy
                        && self.retry.is_none()
                        && report.is_some()
                        && self.generation_source.is_some()
                        && settings.as_ref().is_ok_and(|s| s.validate().is_ok())
                        && self.directory.is_some(),
                    egui::Button::new("Save generation request for review"),
                )
                .clicked()
            {
                if let (Some(report), Some(source), Ok(settings), Some(directory)) = (
                    report,
                    self.generation_source.clone(),
                    settings,
                    self.directory.as_ref(),
                ) {
                    let request = crate::session::new_request_id();
                    let path = directory.join(format!("assistant-generation-{request}.json"));
                    self.generation_path = path.to_string_lossy().into_owned();
                    self.send(Action::PrepareGeneration(Box::new(GenerationDraft {
                        path,
                        request,
                        report,
                        source,
                        preset: self.generation_preset.clone(),
                        settings,
                        model: (!self.generation_model.trim().is_empty())
                            .then(|| PathBuf::from(&self.generation_model)),
                    })));
                }
            }
        }
        ui.label("Saved generation request path");
        ui.text_edit_singleline(&mut self.generation_path);
        if ui
            .add_enabled(
                !self.busy && self.retry.is_none() && !self.generation_path.is_empty(),
                egui::Button::new("Restore saved generation request"),
            )
            .clicked()
        {
            self.send(Action::RestoreGeneration(PathBuf::from(
                &self.generation_path,
            )));
        }
        if let Some(prepared) = self.generation_prepared.clone() {
            ui.group(|ui| {
                ui.label(format!("Request {} · revision {} · proposal {}", prepared.intent.request_id, prepared.intent.revision, prepared.intent.proposal_label()));
                ui.monospace(serde_json::to_string_pretty(&prepared.intent.command).unwrap_or_else(|_| "Request unavailable".into()));
                if let Some(patterns) = &prepared.intent.proposed_patterns {
                    ui.label("These exact synthetic patterns create a candidate after approval. Review it before committing; generation does not change the master script.");
                    self.pattern_reviews.show(ui, patterns);
                    ui.monospace(serde_json::to_string_pretty(patterns).unwrap_or_else(|_| "Patterns unavailable".into()));
                } else {
                    ui.label("The Engine applies the approved stride or derives the selected target seed and captured view. Source/view conflicts are refused. This creates a job and candidate; commit remains a separate reviewed action.");
                }
                if prepared.intent.revision != binding.1 { ui.label("Historical request: an exact retry may recover an accepted job; a fresh job requires the original revision."); }
                ui.checkbox(&mut self.generation_approved, "Approve this saved generation request");
                let compatible_retry = self.retry.as_ref().is_none_or(|w| matches!(&w.action, Action::Generate { prepared: pending, .. } if pending.digest == prepared.digest));
                if ui.add_enabled(!self.busy && self.generation_approved && compatible_retry, egui::Button::new("Start or retry exact saved generation")).clicked() {
                    self.send(Action::Generate { prepared: Box::new(prepared), approved: self.generation_approved });
                }
            });
        }
        if let Some(job) = self.generation_job.clone() {
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Refresh saved generation job"),
                )
                .clicked()
            {
                self.send(Action::GenerationStatus {
                    job_id: job.job_id.clone(),
                    base_revision: job.base_revision,
                });
            }
            ui.label(format!("Generation job {} · {}", job.job_id, job.state));
            if let Some(candidate) = &job.candidate_id {
                ui.label(format!("Candidate {candidate}"));
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[derive(Default)]
    struct Recording {
        calls: Vec<Vec<u8>>,
    }
    impl EngineApi for Recording {
        fn execute(
            &mut self,
            _: Command,
            _: Option<ProjectId>,
            _: Option<RevisionId>,
        ) -> anyhow::Result<ResponseBody> {
            anyhow::bail!("unexpected discovery or mutation")
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> anyhow::Result<ResponseBody> {
            self.calls
                .push(serde_json::to_vec(&(id, command, project, revision))?);
            anyhow::bail!("injected acknowledgement loss")
        }
    }
    fn spec() -> AssistantStart {
        let identity = || ArtifactIdentity {
            artifact_id: ArtifactId::new("fixture").unwrap(),
            sha256: "b".repeat(64),
            byte_len: 1,
        };
        AssistantStart {
            image: None,
            creative_patterns: None,
            edit_focus: vec![],
            target_focus: vec![],
            review_focus: vec![],
            profile: AssistantProfile {
                vision_projector: None,
                profile_id: AttemptId::new("fixture-profile").unwrap(),
                worker: identity(),
                runtime: identity(),
                model: identity(),
                cpu_threads: 1,
                wall_time_ms: 5000,
                worker_memory_bytes: 512 * 1024 * 1024,
            },
            user_request: "Suggest stride eight".into(),
        }
    }
    fn creative_report() -> AssistantReport {
        let draft = CreativeDraft {
            enabled: true,
            ..Default::default()
        };
        AssistantReport { image: None,
            attempt_id: AttemptId::new("creative").unwrap(), project_id:ProjectId::new("project").unwrap(),session_id:SessionId::new("owner").unwrap(),request_id:RequestId::new("start-creative").unwrap(),base_revision:RevisionId::new(7),engine_epoch:AttemptId::new("epoch").unwrap(),profile:spec().profile,context_sha256:"a".repeat(64),scope:AssistantProposalScope { creative_patterns:draft.scope().unwrap(), ..Default::default() },state:AssistantState::Completed,
            proposal:Some(AssistantProposal::SynthesizePatterns { patterns:vec![parse_pattern_prompt("pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8").unwrap()] }),error:None,worker_peak_memory_bytes:Some(1)
        }
    }
    struct CreativeClient {
        project: ProjectId,
        revision: RevisionId,
        reads: usize,
        recording: Recording,
    }
    impl EngineApi for CreativeClient {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> anyhow::Result<ResponseBody> {
            anyhow::ensure!(
                matches!(command, Command::GetSnapshot)
                    && project.as_ref() == Some(&self.project)
                    && revision.is_none(),
                "read-only preparation required"
            );
            self.reads += 1;
            Ok(ResponseBody::Project(Box::new(
                crate::motion::ProjectSnapshot {
                    project_id: self.project.clone(),
                    revision: self.revision,
                    name: "empty authored project".into(),
                    program: std::sync::Arc::new(MotionProgram::default()),
                    sources: vec![],
                    placements: vec![],
                    motion: MotionDescriptor {
                        program: ProgramDescriptor {
                            artifact_id: ArtifactId::new("fixture").unwrap(),
                            sha256: "a".repeat(64),
                            byte_len: 13,
                            codec: MotionCodec::MotionProgramJsonV1,
                            axes: vec![],
                        },
                        binding: MotionBinding::ProjectRevision {
                            project_id: self.project.clone(),
                            revision: self.revision,
                        },
                    },
                },
            )))
        }
        fn execute_identified(
            &mut self,
            id: RequestId,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> anyhow::Result<ResponseBody> {
            self.recording
                .execute_identified(id, command, project, revision)
        }
    }
    #[test]
    fn creative_desktop_scope_is_explicit_bounded_and_exclusive() {
        let mut draft = CreativeDraft {
            seconds: f64::NAN,
            ..Default::default()
        };
        assert!(draft.scope().unwrap().is_none());
        draft.enabled = true;
        assert!(draft.scope().is_err());
        draft.seconds = 2.0;
        draft.axes = Axis::ALL.to_vec();
        let scope = draft.scope().unwrap().unwrap();
        assert_eq!(scope.duration.as_nanos(), 2_000_000_000);
        assert_eq!(scope.axes.len(), 6);
        for seconds in [0.0, -1.0, f64::INFINITY, 86_400.1] {
            draft.seconds = seconds;
            assert!(draft.scope().is_err());
        }
        draft.seconds = 2.0;
        draft.axes.clear();
        assert!(draft.scope().is_err());
        draft.axes = vec![Axis::Stroke, Axis::Stroke];
        assert!(draft.scope().is_err());
        draft.axes = Axis::ALL.to_vec();
        draft.sample_hz = 1000;
        draft.seconds = 86_400.0;
        assert!(draft.scope().is_err());
        let mut start = spec();
        start.creative_patterns = Some(scope);
        start.validate().unwrap();
        start.review_focus = vec!["review".into()];
        assert!(start.validate().is_err());
    }
    #[test]
    fn creative_desktop_save_restore_and_reviewed_retries_are_exact_and_inert() {
        let dir = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let report = creative_report();
        let credential = "b".repeat(64);
        let ns = Namespace {
            endpoint: Path::new("/private/engine.sock"),
            session: &report.session_id,
            credential: &credential,
        };
        let binding = (report.project_id.clone(), report.base_revision);
        let mut client = CreativeClient {
            project: binding.0.clone(),
            revision: binding.1,
            reads: 0,
            recording: Recording::default(),
        };
        let work = Work {
            binding: binding.clone(),
            action: Action::PrepareCreative {
                path: dir.path().join("creative.json"),
                request: RequestId::new("exact-creative").unwrap(),
                report: Box::new(report.clone()),
            },
        };
        let Value::PreparedGeneration(prepared) = execute(&mut client, &ns, &work).unwrap() else {
            panic!("saved");
        };
        assert_eq!(client.reads, 1);
        assert!(client.recording.calls.is_empty());
        assert_eq!(
            std::fs::metadata(&prepared.path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(prepared.intent.proposed_patterns.as_ref().unwrap().len(), 1);
        let saved = std::fs::read(&prepared.path).unwrap();
        assert!(execute(&mut client, &ns, &work).is_err());
        assert_eq!(std::fs::read(&prepared.path).unwrap(), saved);
        let Value::PreparedGeneration(restored) = execute(
            &mut client,
            &ns,
            &Work {
                binding: binding.clone(),
                action: Action::RestoreGeneration(prepared.path.clone()),
            },
        )
        .unwrap() else {
            panic!("restore");
        };
        assert_eq!(restored.digest, prepared.digest);
        assert!(client.recording.calls.is_empty());
        let mut generation = Work {
            binding: (binding.0.clone(), RevisionId::new(9)),
            action: Action::Generate {
                prepared: restored.clone(),
                approved: false,
            },
        };
        assert!(execute(&mut client, &ns, &generation).is_err());
        assert!(client.recording.calls.is_empty());
        generation.action = Action::Generate {
            prepared: restored.clone(),
            approved: true,
        };
        for _ in 0..2 {
            assert!(execute(&mut client, &ns, &generation).is_err());
        }
        assert_eq!(client.recording.calls.len(), 2);
        assert_eq!(client.recording.calls[0], client.recording.calls[1]);
        let (id, command, project, revision): (
            RequestId,
            Command,
            Option<ProjectId>,
            Option<RevisionId>,
        ) = serde_json::from_slice(&client.recording.calls[0]).unwrap();
        assert_eq!(id.as_str(), "exact-creative");
        assert!(matches!(
            command,
            Command::GenerateCreativeWithAssistant { .. }
        ));
        assert_eq!(project, Some(binding.0));
        assert_eq!(revision, Some(binding.1));
        let mut changed = prepared.intent.clone();
        changed.request_id = RequestId::new("changed").unwrap();
        std::fs::write(&prepared.path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(execute(&mut client, &ns, &generation).is_err());
        assert_eq!(client.recording.calls.len(), 2);
    }
    #[test]
    fn creative_desktop_refuses_owner_report_and_current_revision_changes() {
        let dir = tempfile::tempdir().unwrap();
        let report = creative_report();
        let credential = "b".repeat(64);
        let ns = Namespace {
            endpoint: Path::new("/private/engine.sock"),
            session: &report.session_id,
            credential: &credential,
        };
        let binding = (report.project_id.clone(), report.base_revision);
        let mut client = CreativeClient {
            project: binding.0.clone(),
            revision: RevisionId::new(8),
            reads: 0,
            recording: Recording::default(),
        };
        let path = dir.path().join("refused.json");
        let make = |report| Work {
            binding: binding.clone(),
            action: Action::PrepareCreative {
                path: path.clone(),
                request: RequestId::new("refused").unwrap(),
                report: Box::new(report),
            },
        };
        assert!(execute(&mut client, &ns, &make(report.clone())).is_err());
        assert!(!path.exists());
        client.revision = binding.1;
        let mut wrong = report.clone();
        wrong.session_id = SessionId::new("other").unwrap();
        assert!(execute(&mut client, &ns, &make(wrong)).is_err());
        let mut wrong = report.clone();
        wrong.proposal = Some(AssistantProposal::NoAction {});
        assert!(execute(&mut client, &ns, &make(wrong)).is_err());
        assert!(!path.exists());
        assert!(client.recording.calls.is_empty());
    }
    #[test]
    #[ignore = "requires explicitly owned Engine, pinned learned runtime/model and confined worker"]
    fn native_creative_desktop_controller() -> anyhow::Result<()> {
        let state = PathBuf::from(std::env::var("PULSAR_DESKTOP_CREATIVE_STATE")?);
        anyhow::ensure!(state.is_absolute(), "absolute owned state required");
        let project = ProjectId::new(std::env::var("PULSAR_DESKTOP_CREATIVE_PROJECT")?)?;
        let endpoint = state.join("engine.sock");
        let mut client = SessionClient::connect(&endpoint, &state.join("pairing.token"))?;
        let session = client.session_id().clone();
        let credential = client.recovery_credential_binding();
        let ns = Namespace {
            endpoint: &endpoint,
            session: &session,
            credential: &credential,
        };
        let binding = (project.clone(), RevisionId::new(0));
        let ResponseBody::AssistantProfiles(profiles) =
            client.execute(Command::AssistantProfiles, Some(project.clone()), None)?
        else {
            anyhow::bail!("profiles required");
        };
        anyhow::ensure!(
            profiles.len() == 1,
            "one explicitly configured owner profile required"
        );
        let draft = CreativeDraft {
            enabled: true,
            ..Default::default()
        };
        let start = AssistantStart {
            image: None,
            creative_patterns: draft.scope()?,
            edit_focus: vec![],
            target_focus: vec![],
            review_focus: vec![],
            profile: profiles[0].clone(),
            user_request: "Create a triangle stroke at one hertz with amplitude 0.1 around 0.5."
                .into(),
        };
        let Value::Prepared(prepared) = execute(
            &mut client,
            &ns,
            &Work {
                binding: binding.clone(),
                action: Action::Prepare {
                    path: state.join("desktop-start.json"),
                    request: crate::session::new_request_id(),
                    spec: start,
                    image_source: None,
                },
            },
        )?
        else {
            anyhow::bail!("saved start required");
        };
        anyhow::ensure!(
            execute(
                &mut client,
                &ns,
                &Work {
                    binding: binding.clone(),
                    action: Action::Start {
                        prepared: (*prepared).clone(),
                        approved: false
                    }
                }
            )
            .is_err(),
            "missing inference approval accepted"
        );
        let Value::Prepared(restored) = execute(
            &mut client,
            &ns,
            &Work {
                binding: binding.clone(),
                action: Action::Restore(prepared.path.clone()),
            },
        )?
        else {
            anyhow::bail!("restored start required");
        };
        anyhow::ensure!(prepared.digest == restored.digest, "restored start changed");
        let Value::Response(ResponseBody::Assistant(accepted)) = execute(
            &mut client,
            &ns,
            &Work {
                binding: binding.clone(),
                action: Action::Start {
                    prepared: *restored,
                    approved: true,
                },
            },
        )?
        else {
            anyhow::bail!("accepted start required");
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let report = loop {
            let Value::Response(ResponseBody::Assistant(report)) = execute(
                &mut client,
                &ns,
                &Work {
                    binding: binding.clone(),
                    action: Action::Status(accepted.attempt_id.clone()),
                },
            )?
            else {
                anyhow::bail!("report required");
            };
            if report.state != AssistantState::Running {
                break report;
            }
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "learned proposal deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        let expected=AssistantProposal::SynthesizePatterns { patterns:vec![parse_pattern_prompt("pattern=triangle axis=stroke duration=2s frequency=1hz amplitude=0.1 offset=0.5 sample_hz=8")?] };
        anyhow::ensure!(
            report.state == AssistantState::Completed
                && serde_json::to_value(&report.proposal)? == serde_json::to_value(Some(expected))?,
            "learned prompt mismatch: {report:?}"
        );
        fn render_native_patterns(patterns: &[PatternSpec]) -> anyhow::Result<()> {
            let before = serde_json::to_vec(patterns)?;
            let mut reviews = crate::pattern_review::PatternReviews::default();
            let context = egui::Context::default();
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| reviews.show(ui, patterns));
                },
            );
            anyhow::ensure!(
                output
                    .shapes
                    .iter()
                    .any(|s| matches!(&s.shape, egui::Shape::Path(p) if p.points.len() == 17)),
                "actual native proposal graph missing"
            );
            anyhow::ensure!(
                serde_json::to_vec(patterns)? == before,
                "review changed proposed patterns"
            );
            Ok(())
        }
        let Some(AssistantProposal::SynthesizePatterns { patterns }) = &report.proposal else {
            anyhow::bail!("learned patterns required");
        };
        render_native_patterns(patterns)?;
        eprintln!("actual learned proposal: 17 exact synthetic samples constructed headlessly before generation approval; proposal unchanged PASS");
        let Value::PreparedGeneration(prepared) = execute(
            &mut client,
            &ns,
            &Work {
                binding: binding.clone(),
                action: Action::PrepareCreative {
                    path: state.join("desktop-generation.json"),
                    request: crate::session::new_request_id(),
                    report: report.clone(),
                },
            },
        )?
        else {
            anyhow::bail!("saved generation required");
        };
        render_native_patterns(
            prepared
                .intent
                .proposed_patterns
                .as_ref()
                .expect("saved synthetic patterns"),
        )?;
        eprintln!("actual saved generation request: exact approved-pattern review constructed headlessly; declaration unchanged PASS");
        let Value::PreparedGeneration(restored) = execute(
            &mut client,
            &ns,
            &Work {
                binding: binding.clone(),
                action: Action::RestoreGeneration(prepared.path.clone()),
            },
        )?
        else {
            anyhow::bail!("restored generation required");
        };
        anyhow::ensure!(
            prepared.digest == restored.digest,
            "restored generation changed"
        );
        anyhow::ensure!(
            execute(
                &mut client,
                &ns,
                &Work {
                    binding: binding.clone(),
                    action: Action::Generate {
                        prepared: restored.clone(),
                        approved: false
                    }
                }
            )
            .is_err(),
            "missing generation approval accepted"
        );
        let generation = Work {
            binding,
            action: Action::Generate {
                prepared: restored,
                approved: true,
            },
        };
        let Value::Response(ResponseBody::Job(job)) = execute(&mut client, &ns, &generation)?
        else {
            anyhow::bail!("job required");
        };
        let Value::Response(ResponseBody::Job(replayed)) = execute(&mut client, &ns, &generation)?
        else {
            anyhow::bail!("replayed job required");
        };
        anyhow::ensure!(job.job_id == replayed.job_id, "generation replay differs");
        println!(
            "PULSAR_DESKTOP_CREATIVE_RESULT={}",
            serde_json::json!({"qualified":false,"report":report,"job":job,"restored_exact_intents":true,"separate_approvals":true,"generation_replay":true})
        );
        Ok(())
    }
    fn prepare(
        client: &mut Recording,
        ns: &Namespace<'_>,
        path: PathBuf,
        binding: Binding,
    ) -> Prepared {
        let Value::Prepared(value) = execute(
            client,
            ns,
            &Work {
                binding,
                action: Action::Prepare {
                    path,
                    request: RequestId::new("original-start").unwrap(),
                    spec: spec(),
                    image_source: None,
                },
            },
        )
        .ok()
        .unwrap() else {
            panic!("prepared intent")
        };
        *value
    }
    #[test]
    fn edit_approval_exact_request_replay_and_owner_refusal() {
        let session = SessionId::new("owner").unwrap();
        let credential = "a".repeat(64);
        let ns = Namespace {
            endpoint: Path::new("/private/engine.sock"),
            session: &session,
            credential: &credential,
        };
        let binding = (ProjectId::new("project").unwrap(), RevisionId::new(7));
        let report = AssistantReport {
            image: None,
            attempt_id: AttemptId::new("edit-attempt").unwrap(),
            project_id: binding.0.clone(),
            session_id: session.clone(),
            request_id: RequestId::new("start").unwrap(),
            base_revision: binding.1,
            engine_epoch: AttemptId::new("epoch").unwrap(),
            profile: spec().profile,
            context_sha256: "b".repeat(64),
            scope: AssistantProposalScope {
                edit_recipe_ids: vec!["c".repeat(64)],
                ..Default::default()
            },
            state: AssistantState::Completed,
            proposal: Some(AssistantProposal::ApplyEditRecipe {
                recipe_id: "c".repeat(64),
            }),
            error: None,
            worker_peak_memory_bytes: Some(1),
        };
        let mut client = Recording::default();
        let mut work = Work {
            binding,
            action: Action::ApplyEdit {
                report: Box::new(report.clone()),
                request: RequestId::new("original-edit").unwrap(),
                approved: false,
            },
        };
        assert!(execute(&mut client, &ns, &work).is_err());
        assert!(client.calls.is_empty());
        work.action = Action::ApplyEdit {
            report: Box::new(report.clone()),
            request: RequestId::new("original-edit").unwrap(),
            approved: true,
        };
        for _ in 0..2 {
            assert!(execute(&mut client, &ns, &work).is_err());
        }
        assert_eq!(client.calls.len(), 2);
        assert_eq!(client.calls[0], client.calls[1]);
        let (id, command, project, revision): (
            RequestId,
            Command,
            Option<ProjectId>,
            Option<RevisionId>,
        ) = serde_json::from_slice(&client.calls[0]).unwrap();
        assert_eq!(id.as_str(), "original-edit");
        assert_eq!(project, Some(report.project_id.clone()));
        assert_eq!(revision, Some(report.base_revision));
        assert!(matches!(command, Command::ApplyAssistantEdit { .. }));
        let mut foreign = report;
        foreign.session_id = SessionId::new("foreign").unwrap();
        work.action = Action::ApplyEdit {
            report: Box::new(foreign),
            request: RequestId::new("foreign-edit").unwrap(),
            approved: true,
        };
        assert!(execute(&mut client, &ns, &work).is_err());
        assert_eq!(client.calls.len(), 2);
    }
    #[test]
    fn approval_exact_replay_and_replaced_intent_refusal() {
        let dir = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let session = SessionId::new("owner").unwrap();
        let credential = "a".repeat(64);
        let ns = Namespace {
            endpoint: Path::new("/private/engine.sock"),
            session: &session,
            credential: &credential,
        };
        let binding = (ProjectId::new("project").unwrap(), RevisionId::new(7));
        let mut client = Recording::default();
        let prepared = prepare(
            &mut client,
            &ns,
            dir.path().join("intent.json"),
            binding.clone(),
        );
        assert!(client.calls.is_empty());
        let mut work = Work {
            binding,
            action: Action::Start {
                prepared: prepared.clone(),
                approved: false,
            },
        };
        assert!(execute(&mut client, &ns, &work).is_err());
        assert!(client.calls.is_empty());
        work.action = Action::Start {
            prepared: prepared.clone(),
            approved: true,
        };
        for _ in 0..2 {
            assert!(execute(&mut client, &ns, &work).is_err());
        }
        assert_eq!(client.calls.len(), 2);
        assert_eq!(client.calls[0], client.calls[1]);
        let (id, command, project, revision): (
            RequestId,
            Command,
            Option<ProjectId>,
            Option<RevisionId>,
        ) = serde_json::from_slice(&client.calls[0]).unwrap();
        assert_eq!(id.as_str(), "original-start");
        assert_eq!(project, Some(work.binding.0.clone()));
        assert_eq!(revision, Some(work.binding.1));
        let Command::StartAssistant { spec } = command else {
            panic!("only inference start")
        };
        assert_eq!(spec.user_request, "Suggest stride eight");
        let mut replacement = assistant_client::load(&prepared.path).unwrap();
        replacement.request_id = RequestId::new("replacement-start").unwrap();
        std::fs::write(&prepared.path, serde_json::to_vec(&replacement).unwrap()).unwrap();
        assert!(execute(&mut client, &ns, &work).is_err());
        assert_eq!(client.calls.len(), 2);
    }
    #[test]
    fn restore_requires_current_namespace_and_revision_and_never_dispatches() {
        let dir = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let session = SessionId::new("owner").unwrap();
        let credential = "a".repeat(64);
        let ns = Namespace {
            endpoint: Path::new("/private/engine.sock"),
            session: &session,
            credential: &credential,
        };
        let binding = (ProjectId::new("project").unwrap(), RevisionId::new(7));
        let mut client = Recording::default();
        let prepared = prepare(
            &mut client,
            &ns,
            dir.path().join("intent.json"),
            binding.clone(),
        );
        let work = Work {
            binding: binding.clone(),
            action: Action::Restore(prepared.path.clone()),
        };
        assert!(matches!(
            execute(&mut client, &ns, &work),
            Ok(Value::Prepared(_))
        ));
        let other = "c".repeat(64);
        let changed = Namespace {
            credential: &other,
            ..ns
        };
        assert!(execute(&mut client, &changed, &work).is_err());
        let mut stale = work.clone();
        stale.binding.1 = RevisionId::new(8);
        assert!(matches!(
            execute(&mut client, &ns, &stale),
            Ok(Value::Prepared(_))
        ));
        let stale_start = Work {
            binding: stale.binding.clone(),
            action: Action::Start {
                prepared: prepared.clone(),
                approved: true,
            },
        };
        assert!(execute(&mut client, &ns, &stale_start).is_err());
        stale.binding = (ProjectId::new("another-project").unwrap(), binding.1);
        assert!(execute(&mut client, &ns, &stale).is_err());
        let original = std::fs::read(&prepared.path).unwrap();
        assert!(execute(
            &mut client,
            &ns,
            &Work {
                binding,
                action: Action::Prepare {
                    path: prepared.path.clone(),
                    request: RequestId::new("new-start").unwrap(),
                    spec: spec(),
                    image_source: None,
                }
            }
        )
        .is_err());
        assert_eq!(std::fs::read(&prepared.path).unwrap(), original);
        assert!(client.calls.is_empty());
        let resolution = Work {
            binding: stale_start.binding.clone(),
            action: Action::Resolve(prepared.clone()),
        };
        for _ in 0..2 {
            assert!(execute(&mut client, &ns, &resolution).is_err());
        }
        assert_eq!(client.calls.len(), 2);
        assert_eq!(client.calls[0], client.calls[1]);
        let (_, command, _, revision): (RequestId, Command, Option<ProjectId>, Option<RevisionId>) =
            serde_json::from_slice(&client.calls[0]).unwrap();
        let Command::ResolveAssistantStart { reference } = command else {
            panic!("resolution only")
        };
        assert_eq!(reference.original_request_id.as_str(), "original-start");
        assert_eq!(reference.base_revision, RevisionId::new(7));
        assert!(revision.is_none());
    }
    #[test]
    fn desktop_image_prepare_restore_and_approval_preserve_exact_bound_context() {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let session = SessionId::new("owner").unwrap();
        let binding = (ProjectId::new("project").unwrap(), RevisionId::new(7));
        let ns = Namespace {
            endpoint: Path::new("/engine.sock"),
            session: &session,
            credential: &"a".repeat(64),
        };
        let mut draft = spec();
        draft.profile.vision_projector = Some(draft.profile.model.clone());
        draft.creative_patterns = CreativeDraft {
            enabled: true,
            ..Default::default()
        }
        .scope()
        .unwrap();
        let mut rgb = draft.profile.model.clone();
        rgb.byte_len = 3;
        let image = AssistantImageContext {
            image_id: "c".repeat(64),
            project_id: binding.0.clone(),
            session_id: session.clone(),
            revision: binding.1,
            source_version: SourceVersionId::new("still").unwrap(),
            source: draft.profile.model.clone(),
            rgb,
            width: 1,
            height: 1,
            preprocessing_version: 1,
            ffmpeg: draft.profile.model.clone(),
            ffprobe: draft.profile.model.clone(),
        };
        struct Reader {
            image: AssistantImageContext,
            decodes: usize,
            recording: Recording,
        }
        impl EngineApi for Reader {
            fn execute(
                &mut self,
                command: Command,
                project: Option<ProjectId>,
                revision: Option<RevisionId>,
            ) -> anyhow::Result<ResponseBody> {
                assert!(
                    matches!(command, Command::PrepareAssistantImage { ref source_version } if source_version.as_str() == "still")
                );
                assert_eq!(project, Some(ProjectId::new("project").unwrap()));
                assert_eq!(revision, Some(RevisionId::new(7)));
                self.decodes += 1;
                Ok(ResponseBody::AssistantImage(Box::new(self.image.clone())))
            }
            fn execute_identified(
                &mut self,
                id: RequestId,
                command: Command,
                project: Option<ProjectId>,
                revision: Option<RevisionId>,
            ) -> anyhow::Result<ResponseBody> {
                self.recording
                    .execute_identified(id, command, project, revision)
            }
        }
        let mut reader = Reader {
            image: image.clone(),
            decodes: 0,
            recording: Recording::default(),
        };
        let path = root.path().join("image-intent.json");
        let work = |spec: AssistantStart| Work {
            binding: binding.clone(),
            action: Action::Prepare {
                path: path.clone(),
                request: RequestId::new("image-start").unwrap(),
                spec,
                image_source: Some(image.source_version.clone()),
            },
        };
        for kind in 0..4 {
            let mut bad = draft.clone();
            match kind {
                0 => bad.creative_patterns = None,
                1 => bad.edit_focus = vec!["a".repeat(64)],
                2 => bad.profile.vision_projector = None,
                _ => bad.user_request.clear(),
            }
            assert!(execute(&mut reader, &ns, &work(bad)).is_err());
            assert!(!path.exists());
        }
        assert_eq!(reader.decodes, 0);
        for kind in 0..4 {
            reader.image = image.clone();
            match kind {
                0 => reader.image.session_id = SessionId::new("foreign").unwrap(),
                1 => reader.image.project_id = ProjectId::new("foreign").unwrap(),
                2 => reader.image.revision = RevisionId::new(8),
                _ => reader.image.source_version = SourceVersionId::new("foreign").unwrap(),
            }
            assert!(execute(&mut reader, &ns, &work(draft.clone())).is_err());
            assert!(!path.exists());
        }
        reader.image = image.clone();
        let Value::Prepared(prepared) = execute(&mut reader, &ns, &work(draft)).unwrap() else {
            panic!("prepared required")
        };
        assert_eq!(prepared.spec.image.as_ref(), Some(&image));
        let original = std::fs::read(&path).unwrap();
        let Value::Prepared(restored) = execute(
            &mut reader,
            &ns,
            &Work {
                binding: binding.clone(),
                action: Action::Restore(path.clone()),
            },
        )
        .unwrap() else {
            panic!("restored required")
        };
        assert_eq!(prepared.digest, restored.digest);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(reader.recording.calls.is_empty());
        let start = |approved| Work {
            binding: binding.clone(),
            action: Action::Start {
                prepared: (*restored).clone(),
                approved,
            },
        };
        assert!(execute(&mut reader, &ns, &start(false)).is_err());
        assert!(reader.recording.calls.is_empty());
        assert!(execute(&mut reader, &ns, &start(true)).is_err());
        assert_eq!(reader.recording.calls.len(), 1);
        let call: (RequestId, Command, Option<ProjectId>, Option<RevisionId>) =
            serde_json::from_slice(&reader.recording.calls[0]).unwrap();
        assert_eq!(call.0.as_str(), "image-start");
        let Command::StartAssistant { spec } = call.1 else {
            panic!("start required")
        };
        assert_eq!(spec.image.as_ref(), Some(&image));
        let mut changed = assistant_client::load(&path).unwrap();
        changed.request_id = RequestId::new("tampered").unwrap();
        std::fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(execute(&mut reader, &ns, &start(true)).is_err());
        assert_eq!(reader.recording.calls.len(), 1);
    }
    #[test]
    fn stale_reply_does_not_replace_current_view_and_panel_builds_egui_shapes() {
        let (sender, _requests) = mpsc::sync_channel(4);
        let (_replies, receiver) = mpsc::sync_channel(4);
        let binding = (ProjectId::new("project").unwrap(), RevisionId::new(8));
        let mut panel = AssistantPanel {
            sender,
            receiver,
            binding: Some(binding.clone()),
            directory: None,
            busy: true,
            profiles: vec![],
            selected: None,
            prompt: String::new(),
            creative: CreativeDraft::default(),
            pattern_reviews: Default::default(),
            review_focus: String::new(),
            target_focus: String::new(),
            edit_focus: String::new(),
            recipes: vec![],
            edit_approved: false,
            edit_path: String::new(),
            edit_prepared: None,
            edit_saved_approved: false,
            edit_result: None,
            edit_event: None,
            targets: vec![],
            explanation: None,
            intent_path: String::new(),
            prepared: None,
            approved: false,
            report: None,
            attempts: vec![],
            retry: None,
            error: None,
            sources: vec![],
            image_source: None,
            generation_source: None,
            generation_settings: String::new(),
            generation_model: String::new(),
            generation_preset: "fast".into(),
            generation_path: String::new(),
            generation_prepared: None,
            generation_approved: false,
            generation_job: None,
            generation_event: None,
            last_poll: std::time::Instant::now(),
        };
        panel.receive(Reply {
            work: Work {
                binding: (binding.0.clone(), RevisionId::new(7)),
                action: Action::Profiles,
            },
            result: Ok(Value::Response(ResponseBody::AssistantProfiles(vec![
                spec().profile,
            ]))),
        });
        assert!(!panel.busy);
        assert!(panel.profiles.is_empty());
        assert!(panel.retry.is_none());
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| panel.show(ui, Some(binding.clone())));
        });
        assert!(!output.shapes.is_empty());
        assert!(panel.prepared.is_none());
        assert!(!panel.approved);
        panel.creative.enabled = true;
        panel.creative.axes = Axis::ALL.to_vec();
        panel.prompt = "Create gentle synthetic motion".into();
        let mut vision_profile = spec().profile;
        vision_profile.vision_projector = Some(vision_profile.model.clone());
        panel.receive(Reply {
            work: Work {
                binding: binding.clone(),
                action: Action::Profiles,
            },
            result: Ok(Value::Response(ResponseBody::AssistantProfiles(vec![
                vision_profile,
            ]))),
        });
        assert_eq!(panel.profiles.len(), 1);
        assert!(panel.profiles[0].vision_projector.is_some());
        panel.selected = Some(panel.profiles[0].profile_id.clone());
        panel.sources = vec![SourceSummary {
            source_version: SourceVersionId::new("still").unwrap(),
            label: "Authored still".into(),
            kind: SourceKind::Media,
        }];
        panel.image_source = Some(panel.sources[0].source_version.clone());
        let mut synthetic = creative_report();
        synthetic.base_revision = binding.1;
        panel.report = Some(synthetic);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| panel.show(ui, Some(binding.clone())));
        });
        assert!(!output.shapes.is_empty());
        assert!(panel.prepared.is_none());
        assert!(!panel.approved && !panel.generation_approved);
        let original = Work {
            binding: binding.clone(),
            action: Action::Cancel {
                attempt: AttemptId::new("pending-attempt").unwrap(),
                request: RequestId::new("original-cancel").unwrap(),
            },
        };
        panel.retry = Some(original);
        panel.send(Action::List);
        assert!(
            matches!(&panel.retry.as_ref().unwrap().action, Action::Cancel {request, ..} if request.as_str() == "original-cancel")
        );
        panel.receive(Reply {
            work: Work {
                binding: binding.clone(),
                action: Action::List,
            },
            result: Ok(Value::Response(ResponseBody::AssistantAttempts(vec![]))),
        });
        assert!(
            matches!(&panel.retry.as_ref().unwrap().action, Action::Cancel {request, ..} if request.as_str() == "original-cancel")
        );
        let report = AssistantReport {
            image: None,
            attempt_id: AttemptId::new("selected").unwrap(),
            project_id: binding.0.clone(),
            session_id: SessionId::new("session").unwrap(),
            request_id: RequestId::new("request").unwrap(),
            base_revision: binding.1,
            engine_epoch: AttemptId::new("epoch").unwrap(),
            profile: spec().profile,
            context_sha256: "a".repeat(64),
            scope: AssistantProposalScope::default(),
            state: AssistantState::Completed,
            proposal: Some(AssistantProposal::NoAction {}),
            error: None,
            worker_peak_memory_bytes: Some(1),
        };
        panel.report = Some(report.clone());
        panel.explanation = Some("old".into());
        panel.send(Action::Explain(Box::new(report.clone())));
        assert!(panel.explanation.is_none());
        panel.receive(Reply {
            work: Work {
                binding: binding.clone(),
                action: Action::Explain(Box::new(report.clone())),
            },
            result: Ok(Value::Explanation("record".into())),
        });
        assert_eq!(panel.explanation.as_deref(), Some("record"));
        assert!(panel.retry.is_some());
        let mut other = report;
        other.attempt_id = AttemptId::new("unselected").unwrap();
        panel.receive(Reply {
            work: Work {
                binding: binding.clone(),
                action: Action::Explain(Box::new(other)),
            },
            result: Ok(Value::Explanation("stale".into())),
        });
        assert_eq!(panel.explanation.as_deref(), Some("record"));
        while _requests.try_recv().is_ok() {}
        let mut report = panel.report.clone().unwrap();
        report.proposal = Some(AssistantProposal::SetSampleStride { stride: 8 });
        report.scope.allowed_sample_strides = vec![8];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("generation.json");
        let credential = "b".repeat(64);
        let ns = Namespace {
            endpoint: Path::new("/private/engine.sock"),
            session: &report.session_id,
            credential: &credential,
        };
        let intent = crate::assistant_generation_intent::GenerationIntent {
            proposed_patterns: None,
            schema_version: 1,
            endpoint: ns.endpoint.into(),
            session: ns.session.clone(),
            credential_binding: credential.clone(),
            request_id: RequestId::new("original-generation").unwrap(),
            project: binding.0.clone(),
            revision: binding.1,
            proposed_stride: Some(8),
            proposed_target: None,
            command: assistant_client::approved_generation_command(
                &report,
                SourceVersionId::new("media-source").unwrap(),
                "fast".into(),
                GenerationSettings::default(),
                None,
                true,
            )
            .unwrap(),
        };
        crate::assistant_generation_intent::save(&path, &intent).unwrap();
        let mut client = Recording::default();
        let Value::PreparedGeneration(prepared) = execute(
            &mut client,
            &ns,
            &Work {
                binding: binding.clone(),
                action: Action::RestoreGeneration(path),
            },
        )
        .unwrap() else {
            panic!("restore")
        };
        assert!(client.calls.is_empty());
        let mut work = Work {
            binding: (binding.0.clone(), RevisionId::new(9)),
            action: Action::Generate {
                prepared: prepared.clone(),
                approved: false,
            },
        };
        assert!(execute(&mut client, &ns, &work).is_err());
        assert!(client.calls.is_empty());
        work.action = Action::Generate {
            prepared: prepared.clone(),
            approved: true,
        };
        for _ in 0..2 {
            assert!(execute(&mut client, &ns, &work).is_err());
        }
        assert_eq!(client.calls.len(), 2);
        assert_eq!(client.calls[0], client.calls[1]);
        let (_, _, _, revision): (RequestId, Command, Option<ProjectId>, Option<RevisionId>) =
            serde_json::from_slice(&client.calls[0]).unwrap();
        assert_eq!(revision, Some(binding.1)); // Historical retry keeps its original revision.
        panel.retry = None;
        panel.generation_prepared = Some(*prepared.clone());
        panel.send(Action::Generate {
            prepared: prepared.clone(),
            approved: true,
        });
        let original = panel.retry.clone().unwrap();
        panel.receive(Reply {
            work: original.clone(),
            result: Err("acknowledgement lost".into()),
        });
        panel.send(Action::Cancel {
            attempt: report.attempt_id.clone(),
            request: RequestId::new("different-mutation").unwrap(),
        });
        assert!(
            matches!(&panel.retry.as_ref().unwrap().action, Action::Generate { prepared, .. } if prepared.intent.request_id.as_str() == "original-generation")
        );
        panel.send(Action::List);
        panel.receive(Reply {
            work: Work {
                binding: binding.clone(),
                action: Action::List,
            },
            result: Ok(Value::Response(ResponseBody::AssistantAttempts(vec![]))),
        });
        assert!(matches!(
            panel.retry.as_ref().unwrap().action,
            Action::Generate { .. }
        ));
        let job = JobSnapshot {
            job_id: JobId::new("accepted-job").unwrap(),
            attempt_id: AttemptId::new("job-attempt").unwrap(),
            project_id: binding.0.clone(),
            base_revision: binding.1,
            state: "queued".into(),
            candidate_id: None,
            error: None,
        };
        panel.receive(Reply {
            work: original,
            result: Ok(Value::Response(ResponseBody::Job(job.clone()))),
        });
        assert!(panel.retry.is_none());
        assert_eq!(panel.take_generation_job().unwrap().job_id, job.job_id);
        assert!(panel.take_generation_job().is_none());
        panel.retry = Some(Work {
            binding: binding.clone(),
            action: Action::Generate {
                prepared,
                approved: true,
            },
        });
        let status_work = Work {
            binding,
            action: Action::GenerationStatus {
                job_id: job.job_id.clone(),
                base_revision: job.base_revision,
            },
        };
        let mut completed = job.clone();
        completed.state = "completed".into();
        completed.candidate_id = Some(CandidateId::new("candidate").unwrap());
        panel.receive(Reply {
            work: status_work.clone(),
            result: Ok(Value::Response(ResponseBody::Job(completed))),
        });
        assert!(panel.retry.is_some());
        assert!(panel.take_generation_job().unwrap().candidate_id.is_some());
        let mut wrong = job;
        wrong.job_id = JobId::new("different-job").unwrap();
        panel.receive(Reply {
            work: status_work,
            result: Ok(Value::Response(ResponseBody::Job(wrong))),
        });
        assert!(panel.take_generation_job().is_none());
        assert!(panel.retry.is_some());
        while _requests.try_recv().is_ok() {}
        panel.retry = None;
        panel.busy = false;
        let binding = panel.binding.clone().unwrap();
        let mut edit_report = panel.report.clone().unwrap();
        edit_report.scope.edit_recipe_ids = vec!["c".repeat(64)];
        edit_report.proposal = Some(AssistantProposal::ApplyEditRecipe {
            recipe_id: "c".repeat(64),
        });
        let original = Work {
            binding: binding.clone(),
            action: Action::ApplyEdit {
                report: Box::new(edit_report.clone()),
                request: RequestId::new("original-edit").unwrap(),
                approved: true,
            },
        };
        panel.retry = Some(original.clone());
        let mut changed = edit_report;
        changed.proposal = Some(AssistantProposal::NoAction {});
        panel.send(Action::ApplyEdit {
            report: Box::new(changed),
            request: RequestId::new("original-edit").unwrap(),
            approved: true,
        });
        assert!(_requests.try_recv().is_err());
        panel.send(Action::Recipes);
        assert!(matches!(
            _requests.try_recv().unwrap().action,
            Action::Recipes
        ));
        panel.receive(Reply {
            work: Work {
                binding: binding.clone(),
                action: Action::Recipes,
            },
            result: Ok(Value::Response(ResponseBody::AssistantEditRecipes(vec![]))),
        });
        assert!(matches!(
            panel.retry.as_ref().unwrap().action,
            Action::ApplyEdit { .. }
        ));
        panel.send(original.action.clone());
        assert!(matches!(
            _requests.try_recv().unwrap().action,
            Action::ApplyEdit { .. }
        ));
        let id = CandidateId::new("accepted-edit").unwrap();
        let candidate = crate::motion::CandidateSnapshot {
            project_interval: None,
            candidate_id: id.clone(),
            project_id: binding.0.clone(),
            base_revision: binding.1,
            motion: MotionDescriptor {
                program: ProgramDescriptor {
                    artifact_id: ArtifactId::new("fixture").unwrap(),
                    sha256: "a".repeat(64),
                    byte_len: 13,
                    codec: MotionCodec::MotionProgramJsonV1,
                    axes: vec![],
                },
                binding: MotionBinding::Candidate {
                    project_id: binding.0,
                    candidate_id: id.clone(),
                    base_revision: binding.1,
                },
            },
            program: std::sync::Arc::new(MotionProgram::default()),
            job_id: None,
            review: vec![],
        };
        let mut wrong = candidate.clone();
        wrong.base_revision = RevisionId::new(6);
        panel.receive(Reply {
            work: original.clone(),
            result: Ok(Value::Response(ResponseBody::Candidate(Box::new(wrong)))),
        });
        assert!(panel.take_edit_candidate().is_none());
        assert!(panel.retry.is_some());
        panel.edit_approved = true;
        panel.receive(Reply {
            work: original,
            result: Ok(Value::Response(ResponseBody::Candidate(Box::new(
                candidate.clone(),
            )))),
        });
        assert!(panel.retry.is_none());
        assert!(!panel.edit_approved);
        assert_eq!(panel.take_edit_candidate().unwrap().candidate_id, id);
        assert!(panel.take_edit_candidate().is_none());
        while _requests.try_recv().is_ok() {}
        panel.busy = false;
        let binding = panel.binding.clone().unwrap();
        let mut recipe = AssistantEditRecipe {
            recipe_id: String::new(),
            project_id: binding.0.clone(),
            revision: RevisionId::new(7),
            program_sha256: "a".repeat(64),
            axis: Axis::Stroke,
            operation: ValueEditOperation::Invert,
        };
        recipe.recipe_id = recipe.identity().unwrap();
        let saved = crate::assistant_edit_intent::EditIntent {
            schema_version: 1,
            endpoint: ns.endpoint.into(),
            session: ns.session.clone(),
            credential_binding: credential.clone(),
            request_id: RequestId::new("saved-edit").unwrap(),
            project: binding.0.clone(),
            revision: recipe.revision,
            approval: AssistantEditApproval {
                attempt_id: AttemptId::new("saved-attempt").unwrap(),
                context_sha256: "d".repeat(64),
                proposal_sha256: format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&Some(AssistantProposal::ApplyEditRecipe {
                            recipe_id: recipe.recipe_id.clone()
                        }))
                        .unwrap()
                    )
                ),
            },
            recipe,
        };
        let path = dir.path().join("edit.json");
        crate::assistant_edit_intent::save(&path, &saved).unwrap();
        let restore = Work {
            binding: binding.clone(),
            action: Action::RestoreEdit(path.clone()),
        };
        let mut client = Recording::default();
        let value = execute(&mut client, &ns, &restore).unwrap();
        assert!(client.calls.is_empty());
        panel.edit_saved_approved = true;
        panel.receive(Reply {
            work: restore,
            result: Ok(value),
        });
        assert!(!panel.edit_saved_approved);
        let prepared = Box::new(panel.edit_prepared.clone().unwrap());
        assert_eq!(prepared.intent.revision, RevisionId::new(7));
        let original = Work {
            binding: binding.clone(),
            action: Action::ApplySavedEdit {
                prepared: prepared.clone(),
                approved: true,
            },
        };
        panel.retry = Some(original.clone());
        panel.send(Action::RestoreEdit(path));
        assert!(_requests.try_recv().is_err());
        panel.send(Action::Recipes);
        assert!(matches!(
            _requests.try_recv().unwrap().action,
            Action::Recipes
        ));
        panel.receive(Reply {
            work: Work {
                binding: binding.clone(),
                action: Action::Recipes,
            },
            result: Ok(Value::Response(ResponseBody::AssistantEditRecipes(vec![]))),
        });
        assert!(matches!(
            panel.retry.as_ref().unwrap().action,
            Action::ApplySavedEdit { .. }
        ));
        let mut changed = *prepared;
        changed.intent.request_id = RequestId::new("different-edit").unwrap();
        changed.digest = changed.intent.digest().unwrap();
        panel.send(Action::ApplySavedEdit {
            prepared: Box::new(changed),
            approved: true,
        });
        assert!(_requests.try_recv().is_err());
        panel.send(original.action.clone());
        assert!(matches!(
            _requests.try_recv().unwrap().action,
            Action::ApplySavedEdit { .. }
        ));
        let mut recovered = candidate;
        recovered.base_revision = RevisionId::new(7);
        recovered.motion.binding = MotionBinding::Candidate {
            project_id: recovered.project_id.clone(),
            candidate_id: recovered.candidate_id.clone(),
            base_revision: recovered.base_revision,
        };
        panel.receive(Reply {
            work: original,
            result: Ok(Value::Response(ResponseBody::Candidate(Box::new(
                recovered,
            )))),
        });
        assert!(panel.retry.is_none());
        assert!(!panel.edit_saved_approved);
        assert_eq!(panel.edit_result.as_ref().unwrap().1, RevisionId::new(7));
        assert!(panel.take_edit_candidate().is_some());
        assert!(panel.take_edit_candidate().is_none());
    }
}
