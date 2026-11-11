//! Local conversation display and a dedicated inference RPC lane.
use crate::SessionClient;
use eframe::egui;
use pulsar_protocol::{
    AssistantProfile, AttemptId, ProjectId, RoleplayMessage, RoleplayReply, RoleplaySpeaker,
    RoleplayToolReceipt, RoleplayTurnInput, MAX_ROLEPLAY_REPLY_BYTES,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
};

enum Request {
    Profiles(ProjectId),
    Turn {
        project: ProjectId,
        input: RoleplayTurnInput,
        deadline_ms: u64,
        media: Option<PathBuf>,
    },
}

enum Reply {
    Profiles(Vec<AssistantProfile>),
    Turn {
        text: String,
        receipts: Vec<RoleplayToolReceipt>,
    },
}

type Work = (u64, Request);
type Completion = (u64, Result<Reply, String>);

#[derive(PartialEq, Eq)]
struct Context {
    project: ProjectId,
    consent: PathBuf,
    media: Option<PathBuf>,
}

pub(crate) struct RoleplayPanel {
    requests: SyncSender<Work>,
    replies: Receiver<Completion>,
    context: Option<Context>,
    epoch: Arc<AtomicU64>,
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
        let epoch = Arc::new(AtomicU64::new(0));
        let active_epoch = epoch.clone();
        let started = std::thread::Builder::new()
            .name("pulsar-roleplay".into())
            .spawn(move || {
                let mut client = None;
                let mut work_calls = 0;
                let mut work_epoch = 0;
                while let Ok((epoch, work)) = incoming.recv() {
                    if work_epoch != epoch {
                        work_epoch = epoch;
                        work_calls = 0;
                    }
                    let result = execute(
                        &mut client,
                        &endpoint,
                        &bootstrap,
                        work,
                        &mut work_calls,
                        || active_epoch.load(Ordering::Acquire) == epoch,
                    );
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
            epoch,
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
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.messages.clear();
        self.pending_user = None;
        self.draft.clear();
        self.status.clear();
    }

    pub(crate) fn update(
        &mut self,
        project: Option<&ProjectId>,
        consent: &Path,
        media: Option<PathBuf>,
    ) {
        let context = project
            .filter(|_| consent.is_absolute())
            .map(|project| Context {
                project: project.clone(),
                consent: consent.to_owned(),
                media,
            });
        if self.context != context {
            self.clear();
            self.context = context;
        }
        while let Ok((epoch, reply)) = self.replies.try_recv() {
            self.busy = false;
            if epoch != self.epoch.load(Ordering::Acquire) {
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
                Ok(Reply::Turn { text, receipts }) => {
                    let Some(user_text) = self.pending_user.take() else {
                        continue;
                    };
                    self.messages.push(RoleplayMessage {
                        speaker: RoleplaySpeaker::User,
                        text: user_text,
                        tool_receipts: vec![],
                    });
                    self.messages.push(RoleplayMessage {
                        speaker: RoleplaySpeaker::Assistant,
                        text,
                        tool_receipts: receipts,
                    });
                    self.status.clear();
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
        if self
            .requests
            .try_send((self.epoch.load(Ordering::Acquire), work))
            .is_ok()
        {
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
            tool_receipts: vec![],
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
            media: context.media.clone(),
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
                .add_enabled(
                    !self.busy && self.context.is_some(),
                    egui::Button::new("Load model profiles"),
                )
                .clicked()
            {
                if let Some(context) = &self.context {
                    self.send(Request::Profiles(context.project.clone()));
                }
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
                    for receipt in &message.tool_receipts {
                        ui.small(format!(
                            "{} · {}",
                            receipt.name,
                            if receipt.accepted {
                                "accepted"
                            } else {
                                "refused"
                            }
                        ));
                    }
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
    work_calls: &mut u32,
    is_current: impl Fn() -> bool,
) -> anyhow::Result<Reply> {
    if client.is_none() {
        *client = Some(SessionClient::connect(endpoint, bootstrap)?);
    }
    let client = client.as_mut().expect("connected roleplay client");
    match work {
        Request::Profiles(project) => {
            crate::roleplay_conversation::profiles(client, &project).map(Reply::Profiles)
        }
        Request::Turn {
            project,
            input,
            deadline_ms,
            media,
        } => {
            let consent = input.consent.clone();
            let RoleplayReply { text, calls } = *crate::roleplay_conversation::infer(
                client,
                bootstrap,
                &project,
                input,
                deadline_ms,
            )?;
            let receipts = crate::mcp_stdio::dispatch_roleplay_calls(
                endpoint,
                bootstrap,
                project,
                &consent,
                media.as_deref(),
                calls,
                work_calls,
                is_current,
            );
            Ok(Reply::Turn { text, receipts })
        }
    }
}
