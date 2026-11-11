//! Local conversation display and a dedicated inference RPC lane.
use crate::{motion::ResponseBody, EngineApi, SessionClient};
use eframe::egui;
use pulsar_protocol::{
    AssistantProfile, AttemptId, Command, ProjectId, RoleplayMessage, RoleplayReply,
    RoleplaySpeaker, RoleplayTurnInput, MAX_ROLEPLAY_REPLY_BYTES,
};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

enum Request {
    Profiles,
    Turn {
        project: ProjectId,
        input: RoleplayTurnInput,
        deadline_ms: u64,
    },
}

enum Reply {
    Profiles(Vec<AssistantProfile>),
    Turn(Box<RoleplayReply>),
}

type Work = (u64, Request);
type Completion = (u64, Result<Reply, String>);

#[derive(PartialEq, Eq)]
struct Context {
    project: ProjectId,
    consent: PathBuf,
}

pub(crate) struct RoleplayPanel {
    requests: SyncSender<Work>,
    replies: Receiver<Completion>,
    context: Option<Context>,
    epoch: u64,
    busy: bool,
    profiles: Vec<AssistantProfile>,
    selected: Option<AttemptId>,
    messages: Vec<RoleplayMessage>,
    pending_user: Option<String>,
    draft: String,
    status: String,
}

impl RoleplayPanel {
    pub(crate) fn new(endpoint: PathBuf, bootstrap: PathBuf, repaint: egui::Context) -> Self {
        let (requests, incoming) = mpsc::sync_channel(1);
        let (outgoing, replies) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("pulsar-roleplay".into())
            .spawn(move || {
                let mut client = None;
                while let Ok((epoch, work)) = incoming.recv() {
                    let result = execute(&mut client, &endpoint, &bootstrap, work);
                    let result =
                        result.map_err(|error| error.to_string().chars().take(500).collect());
                    if outgoing.send((epoch, result)).is_err() {
                        break;
                    }
                    repaint.request_repaint();
                }
            });
        Self {
            requests,
            replies,
            context: None,
            epoch: 0,
            busy: false,
            profiles: Vec::new(),
            selected: None,
            messages: Vec::new(),
            pending_user: None,
            draft: String::new(),
            status: if started.is_ok() {
                String::new()
            } else {
                "Roleplay worker could not start".into()
            },
        }
    }

    fn clear(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.messages.clear();
        self.pending_user = None;
        self.draft.clear();
        self.status.clear();
    }

    pub(crate) fn update(&mut self, project: Option<&ProjectId>, consent: &Path) {
        let context = project
            .filter(|_| consent.is_absolute())
            .map(|project| Context {
                project: project.clone(),
                consent: consent.to_owned(),
            });
        if self.context != context {
            self.clear();
            self.context = context;
        }
        while let Ok((epoch, reply)) = self.replies.try_recv() {
            self.busy = false;
            if epoch != self.epoch {
                continue;
            }
            match reply {
                Ok(Reply::Profiles(profiles)) => {
                    if !profiles
                        .iter()
                        .any(|profile| Some(&profile.profile_id) == self.selected.as_ref())
                    {
                        self.selected = profiles.first().map(|profile| profile.profile_id.clone());
                    }
                    self.profiles = profiles;
                    self.status = if self.profiles.is_empty() {
                        "No local model profile is registered".into()
                    } else {
                        String::new()
                    };
                }
                Ok(Reply::Turn(reply)) => {
                    let Some(text) = self.pending_user.take() else {
                        continue;
                    };
                    self.messages.push(RoleplayMessage {
                        speaker: RoleplaySpeaker::User,
                        text,
                    });
                    self.messages.push(RoleplayMessage {
                        speaker: RoleplaySpeaker::Assistant,
                        text: reply.text,
                    });
                    self.status = if reply.calls.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "The model requested {} tools. Desktop tool routing is unavailable.",
                            reply.calls.len()
                        )
                    };
                }
                Err(error) => {
                    if let Some(text) = self.pending_user.take() {
                        self.draft = text;
                    }
                    self.status = error;
                }
            }
        }
    }

    fn send(&mut self, work: Request) -> bool {
        if self.requests.try_send((self.epoch, work)).is_ok() {
            self.busy = true;
            self.status.clear();
            true
        } else {
            self.status = "Roleplay worker is unavailable".into();
            false
        }
    }

    fn send_turn(&mut self) {
        let Some(context) = &self.context else { return };
        let Some(profile) = self
            .profiles
            .iter()
            .find(|profile| Some(&profile.profile_id) == self.selected.as_ref())
        else {
            return;
        };
        let submitted = self.draft.trim().to_owned();
        let mut messages = self.messages.clone();
        messages.push(RoleplayMessage {
            speaker: RoleplaySpeaker::User,
            text: submitted.clone(),
        });
        let input = RoleplayTurnInput {
            profile: profile.profile_id.clone(),
            consent: context.consent.clone(),
            messages,
        };
        if let Err(error) = input.encode_messages() {
            self.status = error.to_string();
            return;
        }
        if self.send(Request::Turn {
            project: context.project.clone(),
            input,
            deadline_ms: profile.wall_time_ms,
        }) {
            self.draft.clear();
            self.pending_user = Some(submitted);
        }
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Local roleplay");
        if self.context.is_none() {
            ui.label("Open a project and select a consent document in Settings.");
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Load model profiles"))
                .clicked()
            {
                self.send(Request::Profiles);
            }
            egui::ComboBox::from_id_salt("roleplay-profile")
                .selected_text(
                    self.selected
                        .as_ref()
                        .map_or("Select model", AttemptId::as_str),
                )
                .show_ui(ui, |ui| {
                    for profile in &self.profiles {
                        ui.selectable_value(
                            &mut self.selected,
                            Some(profile.profile_id.clone()),
                            profile.profile_id.as_str(),
                        );
                    }
                });
            if ui
                .add_enabled(
                    !self.messages.is_empty() || self.busy,
                    egui::Button::new("Clear conversation"),
                )
                .clicked()
            {
                self.clear();
            }
        });
        egui::ScrollArea::vertical()
            .max_height(250.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for message in &self.messages {
                    ui.strong(if message.speaker == RoleplaySpeaker::User {
                        "You"
                    } else {
                        "Pulsar"
                    });
                    ui.label(&message.text);
                }
                if let Some(message) = &self.pending_user {
                    ui.strong("You");
                    ui.label(message);
                }
            });
        ui.add_enabled(
            !self.busy && self.context.is_some(),
            egui::TextEdit::multiline(&mut self.draft)
                .hint_text("Message")
                .char_limit(MAX_ROLEPLAY_REPLY_BYTES),
        );
        if ui
            .add_enabled(
                !self.busy
                    && self.context.is_some()
                    && self.selected.is_some()
                    && !self.draft.trim().is_empty(),
                egui::Button::new("Send"),
            )
            .clicked()
        {
            self.send_turn();
        }
        if self.busy {
            ui.spinner();
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
    }
}

fn execute(
    client: &mut Option<SessionClient>,
    endpoint: &Path,
    bootstrap: &Path,
    work: Request,
) -> anyhow::Result<Reply> {
    if client.is_none() {
        *client = Some(SessionClient::connect(endpoint, bootstrap)?);
    }
    let client = client.as_mut().expect("connected roleplay client");
    match work {
        Request::Profiles => {
            client.set_operation_deadline(Some(Instant::now() + Duration::from_secs(5)));
            match client.execute(Command::AssistantProfiles, None, None)? {
                ResponseBody::AssistantProfiles(profiles) => Ok(Reply::Profiles(profiles)),
                _ => anyhow::bail!("Engine returned an unexpected model catalog"),
            }
        }
        Request::Turn {
            project,
            input,
            deadline_ms,
        } => {
            let directory = bootstrap
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Engine state directory is missing"))?;
            let owner_proof =
                crate::project_package::read_local_owner_proof(directory, "handy-owner.token")?;
            client.set_operation_deadline(Some(
                Instant::now() + Duration::from_millis(deadline_ms.saturating_add(5000)),
            ));
            match client.execute(
                Command::RunRoleplayTurn { input, owner_proof },
                Some(project),
                None,
            )? {
                ResponseBody::RoleplayReply(reply) => Ok(Reply::Turn(reply)),
                _ => anyhow::bail!("Engine returned an unexpected roleplay reply"),
            }
        }
    }
}
