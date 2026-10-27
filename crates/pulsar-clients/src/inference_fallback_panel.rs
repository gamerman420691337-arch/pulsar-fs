//! Disposable, one-shot generation intent. Policy file I/O and dispatch belong
//! to the Desktop RPC worker; a UI reply can never initiate another generation.
use crate::{
    motion::ResponseBody,
    session::{EngineApi, SessionClient},
};
use anyhow::{bail, ensure, Context, Result};
use eframe::egui;
use pulsar_protocol::*;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SessionBinding {
    session: SessionId,
    credential: String,
}
impl SessionBinding {
    pub(super) fn current(client: &SessionClient) -> Self {
        Self {
            session: client.session_id().clone(),
            credential: client.recovery_credential_binding(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    pub(super) session: SessionBinding,
    pub(super) project: ProjectId,
    pub(super) revision: RevisionId,
    pub(super) source: SourceVersionId,
    /// Typed serialization of the selected model/pack and its exact profile.
    /// It is transient GUI correspondence, not evidence or Engine permission.
    pub(super) model_key: Vec<u8>,
}

#[derive(Default)]
pub(super) struct FallbackPanel {
    enabled: bool,
    selection_path: String,
    binding: Option<Binding>,
}
impl FallbackPanel {
    pub(super) fn sync(&mut self, binding: Option<Binding>) {
        if self.binding != binding {
            self.enabled = false;
            self.binding = binding;
        }
    }
    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(super) fn submitted(&mut self) {
        self.enabled = false;
    }
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, busy: bool, supported: bool) {
        ui.separator();
        ui.label("Equivalent backend fallback");
        ui.label("Selection policy JSON (absolute path)");
        if ui
            .add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut self.selection_path).char_limit(4096),
            )
            .changed()
        {
            self.enabled = false;
        }
        let valid_path = crate::cli::parse_fallback_selection_path(&self.selection_path).is_ok();
        let eligible = supported && self.binding.is_some() && valid_path;
        if !eligible {
            self.enabled = false;
        }
        ui.add_enabled(
            !busy && eligible,
            egui::Checkbox::new(
                &mut self.enabled,
                "Allow accepted equivalent backends for this generation",
            ),
        );
        if !supported {
            ui.small("The connected Engine does not advertise fallback generation.");
        } else if self.binding.is_none() {
            ui.small("Select a media source and an explicit model profile or current model pack.");
        } else if !valid_path {
            ui.small("Enter an absolute selection-file path without traversal.");
        }
        ui.small("The Engine checks accepted evidence and current scope. Consent applies to one submission; candidates still require review.");
    }
    /// Pure intent capture. Reading the file here would block the UI thread.
    pub(super) fn prepare(&self, command: Command) -> Result<Intent> {
        ensure!(self.enabled, "Fallback is not enabled for this submission");
        let binding = self
            .binding
            .clone()
            .context("Fallback selection is no longer current")?;
        let selection_path = crate::cli::parse_fallback_selection_path(&self.selection_path)
            .map_err(anyhow::Error::msg)?;
        let (source, preset, settings, path, choice) = generation(&command)?;
        ensure!(
            *source == binding.source,
            "Fallback source changed before submission"
        );
        settings.validate_execution_preset(preset, true)?;
        validate_model(settings, path, choice)?;
        Ok(Intent {
            binding,
            selection_path,
            command,
        })
    }
}

pub(super) struct Intent {
    binding: Binding,
    selection_path: PathBuf,
    command: Command,
}
impl Intent {
    pub(super) fn binding(&self) -> &Binding {
        &self.binding
    }

    /// Called only on the RPC worker after establishing the exact captured session.
    pub(super) fn execute(
        &self,
        client: &mut impl EngineApi,
        session: &SessionBinding,
        project: Option<&ProjectId>,
        revision: Option<RevisionId>,
    ) -> Result<ResponseBody> {
        ensure!(*session == self.binding.session
            && project == Some(&self.binding.project) && revision == Some(self.binding.revision),
            "Fallback work differs from its captured session/project/revision; submit again explicitly");
        let (source, preset, settings, path, choice) = generation(&self.command)?;
        ensure!(
            *source == self.binding.source,
            "Fallback work source differs from its captured selection"
        );
        validate_model(settings, path, choice)?;
        let selection = crate::read_fallback_selection(&self.selection_path)?;
        let model = crate::prepared_fallback_model(settings, path, choice, Some(&selection))?
            .context("Fallback model selection is absent")?;
        client.execute(
            Command::GenerateWithInferenceFallback {
                source_version: source.clone(),
                preset: preset.to_owned(),
                settings: Box::new(settings.clone()),
                model,
                selection: Box::new(selection),
            },
            Some(self.binding.project.clone()),
            Some(self.binding.revision),
        )
    }
}

type GenerationParts<'a> = (
    &'a SourceVersionId,
    &'a str,
    &'a GenerationSettings,
    Option<&'a std::path::Path>,
    Option<&'a ModelPackChoice>,
);
fn generation(command: &Command) -> Result<GenerationParts<'_>> {
    match command {
        Command::Generate {
            source_version,
            preset,
            settings: Some(settings),
            model_path: Some(path),
        } => Ok((source_version, preset, settings, Some(path.as_path()), None)),
        Command::GenerateFromModelPack {
            source_version,
            preset,
            settings: Some(settings),
            choice,
        } => Ok((source_version, preset, settings, None, Some(choice))),
        _ => bail!("Fallback requires an explicit model/profile or selected pack on decoded media"),
    }
}
fn validate_model(
    settings: &GenerationSettings,
    path: Option<&std::path::Path>,
    choice: Option<&ModelPackChoice>,
) -> Result<()> {
    ensure!(
        settings.raw_frame_pair.is_none(),
        "Fallback requires decoded media, not raw frame pairs"
    );
    let profile = settings
        .inference_profile
        .as_ref()
        .context("Fallback requires an explicit inference profile")?;
    profile.validate()?;
    ensure!(
        settings
            .model_input
            .as_ref()
            .is_some_and(|input| serde_json::to_vec(input).ok()
                == serde_json::to_vec(&profile.preprocessing.input).ok()),
        "Fallback model input differs from its explicit profile"
    );
    match (path, choice) {
        (Some(path), None) => InferenceFallbackModel::Path {
            path: path.to_owned(),
        }
        .validate()?,
        (None, Some(choice)) => choice.validate()?,
        _ => bail!("Fallback requires exactly one selected model or pack"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> Binding {
        Binding {
            session: SessionBinding {
                session: SessionId::new("session").unwrap(),
                credential: "credential digest".into(),
            },
            project: ProjectId::new("project").unwrap(),
            revision: RevisionId::new(4),
            source: SourceVersionId::new("source").unwrap(),
            model_key: b"original model/profile".to_vec(),
        }
    }
    fn selection() -> InferenceFallbackSelection {
        InferenceFallbackSelection {
            schema_version: 1,
            equivalents: vec![InferenceFallbackReference {
                dossier_id: ArtifactId::new("dossier").unwrap(),
                expected_revision: 2,
                expected_report_sha256: "a".repeat(64),
            }],
            maximum_fallbacks: 1,
            source_policy: InferenceFallbackSourcePolicy {
                schema_version: 1,
                kind: InferenceFallbackSourceKind::DecodedMedia,
                sources: None,
                independent_audio: InferenceFallbackIndependentAudio::Forbidden,
            },
            view_policy: InferenceFallbackViewPolicy {
                schema_version: 1,
                applicability: InferenceFallbackViewApplicability::PreserveRequestedSemantics,
            },
        }
    }
    fn command(model: PathBuf) -> Command {
        let artifact = ArtifactIdentity {
            artifact_id: ArtifactId::new("a".repeat(64)).unwrap(),
            sha256: "a".repeat(64),
            byte_len: 1,
        };
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 3,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::YoloChannelMajor,
        };
        Command::Generate {
            source_version: binding().source,
            preset: "default".into(),
            model_path: Some(model),
            settings: Some(GenerationSettings {
                model_input: Some(input.clone()),
                inference_profile: Some(DeclaredInferenceProfile {
                    schema_version: 1,
                    model: artifact.clone(),
                    runtime: artifact,
                    preprocessing: InferencePreprocessing {
                        input,
                        resize: InferenceResize::Stretch,
                        color: InferenceColor::Rgb,
                        scale: vec![1.0 / 255.0; 3],
                        bias: vec![0.0; 3],
                        padding_byte: 0,
                    },
                    provider: InferenceProvider::Cpu,
                    minimum_memory_bytes: 1,
                    minimum_output_bytes: 1,
                    cpu_threads: 1,
                    cuda_resources: None,
                }),
                project_interval: Some(
                    TimeRange::new(ProjectTime::from_nanos(10), ProjectTime::from_nanos(20))
                        .unwrap(),
                ),
                target_seed: Some(AnalysisTargetSeed {
                    source_frame_index: 0,
                    x: 0.4,
                    y: 0.5,
                    class_id: Some(0),
                }),
                ..GenerationSettings::default()
            }),
        }
    }
    fn panel(path: &std::path::Path) -> FallbackPanel {
        let mut panel = FallbackPanel::default();
        panel.sync(Some(binding()));
        panel.selection_path = path.to_str().unwrap().into();
        panel.enabled = true;
        panel
    }
    #[derive(Default)]
    struct Api {
        calls: Vec<(Command, Option<ProjectId>, Option<RevisionId>)>,
    }
    impl EngineApi for Api {
        fn execute(
            &mut self,
            command: Command,
            project: Option<ProjectId>,
            revision: Option<RevisionId>,
        ) -> Result<ResponseBody> {
            self.calls.push((command, project, revision));
            Ok(ResponseBody::Ack)
        }
    }
    #[test]
    fn consent_is_off_by_default_one_shot_and_cleared_by_every_scope_change() {
        let path = std::env::temp_dir().join("selection.json");
        assert!(!FallbackPanel::default().enabled());
        let mut same = panel(&path);
        same.sync(Some(binding()));
        assert!(same.enabled());
        same.submitted();
        assert!(!same.enabled());
        for change in [
            |b: &mut Binding| b.session.session = SessionId::new("another-session").unwrap(),
            |b: &mut Binding| b.session.credential = "new credential digest".into(),
            |b: &mut Binding| b.project = ProjectId::new("another-project").unwrap(),
            |b: &mut Binding| b.revision = RevisionId::new(5),
            |b: &mut Binding| b.source = SourceVersionId::new("another-source").unwrap(),
            |b: &mut Binding| b.model_key = b"new profile or pack selection".to_vec(),
        ] {
            let mut panel = panel(&path);
            let mut changed = binding();
            change(&mut changed);
            panel.sync(Some(changed));
            assert!(!panel.enabled());
        }
        let mut missing = panel(&path);
        missing.sync(None);
        assert!(!missing.enabled());
    }
    #[test]
    fn intent_capture_is_io_free_and_rejects_implicit_models_and_rebound_source() {
        let root = tempfile::tempdir().unwrap();
        let panel = panel(&root.path().join("not-created.json"));
        let model = root.path().join("not-created.onnx");
        panel.prepare(command(model.clone())).unwrap();
        let mut implicit = command(model.clone());
        if let Command::Generate {
            settings: Some(settings),
            ..
        } = &mut implicit
        {
            settings.inference_profile = None;
        }
        assert!(panel.prepare(implicit).is_err());
        let mut other = command(model);
        if let Command::Generate { source_version, .. } = &mut other {
            *source_version = SourceVersionId::new("other-source").unwrap();
        }
        assert!(panel.prepare(other).is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn headless_controls_do_not_read_policy_files_or_infer_consent() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("absent-policy.json");
        let mut panel = FallbackPanel::default();
        panel.sync(Some(binding()));
        panel.selection_path = path.to_str().unwrap().into();
        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| panel.ui(ui, false, true));
        });
        assert!(!panel.enabled());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        panel.enabled = true;
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| panel.ui(ui, false, false));
        });
        assert!(!panel.enabled());
    }

    #[test]
    fn raw_frame_pair_intent_is_refused_without_policy_io() {
        let root = tempfile::tempdir().unwrap();
        let mut command = command(root.path().join("model"));
        let Command::Generate {
            settings: Some(settings),
            ..
        } = &mut command
        else {
            unreachable!()
        };
        settings.target_seed = None;
        settings.project_interval = None;
        settings.analysis_width = 64;
        settings.analysis_height = 64;
        let input = ModelInputContract {
            width: 64,
            height: 64,
            channels: 6,
            layout: TensorLayout::Nchw,
            class_count: 1,
            decoder: DetectorDecoder::TemporalScriptPosition,
        };
        settings.model_input = Some(input.clone());
        let profile = settings.inference_profile.as_mut().unwrap();
        profile.preprocessing.input = input;
        profile.preprocessing.resize = InferenceResize::NearestStretch;
        profile.preprocessing.color = InferenceColor::RgbFramePair;
        profile.preprocessing.scale = vec![1.0 / 255.0; 6];
        profile.preprocessing.bias = vec![0.0; 6];
        settings.raw_frame_pair = Some(RawFramePair {
            source_epoch: AttemptId::new("raw-source").unwrap(),
            previous_frame: FrameId::new(0),
            current_frame: FrameId::new(1),
            previous_time: SourceTimestamp::new(0, 30).unwrap(),
            current_time: SourceTimestamp::new(1, 30).unwrap(),
            presentation_clock: None,
        });
        settings.validate_execution_preset("default", true).unwrap();
        assert!(panel(&root.path().join("absent-policy.json"))
            .prepare(command)
            .is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[test]
    fn worker_reads_policy_and_dispatches_one_captured_custom_model_request() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("selection.json");
        std::fs::write(&path, serde_json::to_vec(&selection()).unwrap()).unwrap();
        let original = command(root.path().join("model.onnx"));
        let expected_settings = match &original {
            Command::Generate {
                settings: Some(settings),
                ..
            } => serde_json::to_vec(settings).unwrap(),
            _ => unreachable!(),
        };
        let intent = panel(&path).prepare(original).unwrap();
        let parent = std::thread::current().id();
        let api = std::thread::spawn(move || {
            assert_ne!(std::thread::current().id(), parent);
            let mut api = Api::default();
            intent
                .execute(
                    &mut api,
                    &binding().session,
                    Some(&binding().project),
                    Some(binding().revision),
                )
                .unwrap();
            api
        })
        .join()
        .unwrap();
        assert_eq!(api.calls.len(), 1);
        let (command, project, revision) = &api.calls[0];
        assert_eq!(project.as_ref(), Some(&binding().project));
        assert_eq!(*revision, Some(binding().revision));
        match command {
            Command::GenerateWithInferenceFallback {
                source_version,
                preset,
                settings,
                model,
                selection: actual,
            } => {
                assert_eq!(*source_version, binding().source);
                assert_eq!(preset, "default");
                assert_eq!(serde_json::to_vec(settings).unwrap(), expected_settings);
                assert!(matches!(model, InferenceFallbackModel::Path { .. }));
                assert_eq!(actual.identity().unwrap(), selection().identity().unwrap());
            }
            _ => panic!("wrong command"),
        }
    }
    #[test]
    fn worker_keeps_exact_pack_choice_without_acceptance_or_extra_rpc() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("selection.json");
        std::fs::write(&path, serde_json::to_vec(&selection()).unwrap()).unwrap();
        let Command::Generate {
            source_version,
            preset,
            settings,
            ..
        } = command(root.path().join("model"))
        else {
            unreachable!()
        };
        let choice = ModelPackChoice {
            manifest_sha256: "b".repeat(64),
            selection_revision: 7,
        };
        let intent = panel(&path)
            .prepare(Command::GenerateFromModelPack {
                source_version,
                preset,
                settings,
                choice: choice.clone(),
            })
            .unwrap();
        let mut api = Api::default();
        intent
            .execute(
                &mut api,
                &binding().session,
                Some(&binding().project),
                Some(binding().revision),
            )
            .unwrap();
        assert_eq!(api.calls.len(), 1);
        assert!(
            matches!(&api.calls[0].0,Command::GenerateWithInferenceFallback {model:InferenceFallbackModel::ModelPack {choice:actual},..} if actual==&choice)
        );
    }
    #[test]
    fn worker_refuses_changed_session_or_project_before_file_reads_and_never_dispatches_bad_json() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("absent.json");
        let intent = panel(&path)
            .prepare(command(root.path().join("model")))
            .unwrap();
        let mut api = Api::default();
        let mut session = binding().session;
        session.credential = "changed".into();
        let e = intent
            .execute(
                &mut api,
                &session,
                Some(&binding().project),
                Some(binding().revision),
            )
            .unwrap_err();
        assert!(e.to_string().contains("captured session/project/revision"));
        assert!(intent
            .execute(
                &mut api,
                &binding().session,
                Some(&ProjectId::new("other").unwrap()),
                Some(binding().revision)
            )
            .is_err());
        assert!(intent
            .execute(
                &mut api,
                &binding().session,
                Some(&binding().project),
                Some(RevisionId::new(5))
            )
            .is_err());
        std::fs::write(&path, b"{}").unwrap();
        assert!(intent
            .execute(
                &mut api,
                &binding().session,
                Some(&binding().project),
                Some(binding().revision)
            )
            .is_err());
        assert!(api.calls.is_empty());
    }
}
