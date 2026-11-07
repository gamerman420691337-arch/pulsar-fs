//! Desktop presentation of authenticated preparation operations. No native runtime lives here.
use crate::motion::ResponseBody;
use crate::session::SessionClient;
use eframe::egui;
use pulsar_protocol::*;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
};
type Binding = (ProjectId, RevisionId);
#[derive(Clone)]
struct Work {
    binding: Binding,
    request_id: RequestId,
    command: Command,
    dismiss: Option<RequestId>,
}
struct Reply {
    work: Work,
    result: Result<Option<ResponseBody>, String>,
    saved: Result<Vec<crate::native_preparation_recovery::Intent>, String>,
}
pub(crate) struct NativePreparationPanel {
    sender: SyncSender<Work>,
    receiver: Receiver<Reply>,
    binding: Option<Binding>,
    busy: bool,
    limits: Option<NativeImagePreparationLimits>,
    status: Option<NativeImagePreparationStatus>,
    preparation_ms: u64,
    retention_ms: u64,
    original_request: String,
    error: Option<String>,
    retry: Option<Work>,
    saved: Vec<crate::native_preparation_recovery::Intent>,
}
impl NativePreparationPanel {
    pub(crate) fn new(endpoint: PathBuf, bootstrap: PathBuf, repaint: egui::Context) -> Self {
        let (sender, requests) = mpsc::sync_channel::<Work>(4);
        let (replies, receiver) = mpsc::sync_channel(4);
        std::thread::spawn(move || {
            let mut client = None;
            while let Ok(work) = requests.recv() {
                let result = (|| -> anyhow::Result<Option<ResponseBody>> {
                    if client.is_none() {
                        client = Some(SessionClient::connect(&endpoint, &bootstrap)?);
                    }
                    let directory = bootstrap
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("Missing private engine directory"))?;
                    if let Some(request) = &work.dismiss {
                        let connected = client.as_ref().expect("connected");
                        crate::native_preparation_recovery::dismiss(
                            directory,
                            &endpoint,
                            connected.session_id(),
                            &connected.recovery_credential_binding(),
                            &work.binding.0,
                            request,
                        )?;
                        return Ok(None);
                    }
                    crate::native_preparation_recovery::execute(
                        client.as_mut().expect("connected"),
                        directory,
                        &endpoint,
                        work.request_id.clone(),
                        work.command.clone(),
                        work.binding.0.clone(),
                        work.binding.1,
                    )
                    .map(Some)
                })()
                .map_err(|e| e.to_string());
                let saved = (|| -> anyhow::Result<_> {
                    let client = client
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("No connected session"))?;
                    let directory = bootstrap
                        .parent()
                        .ok_or_else(|| anyhow::anyhow!("Missing private engine directory"))?;
                    crate::native_preparation_recovery::load(
                        directory,
                        &endpoint,
                        client.session_id(),
                        &client.recovery_credential_binding(),
                    )
                })()
                .map_err(|e| e.to_string());
                if replies
                    .send(Reply {
                        work,
                        result,
                        saved,
                    })
                    .is_err()
                {
                    break;
                }
                repaint.request_repaint();
            }
        });
        Self {
            sender,
            receiver,
            binding: None,
            busy: false,
            limits: None,
            status: None,
            preparation_ms: 180_000,
            retention_ms: 300_000,
            original_request: String::new(),
            error: None,
            retry: None,
            saved: vec![],
        }
    }
    fn send(&mut self, work: Work) {
        match self.sender.try_send(work.clone()) {
            Ok(()) => {
                self.busy = true;
                self.retry = Some(work);
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    fn query(&mut self, command: Command) {
        if let Some(binding) = self.binding.clone() {
            self.send(Work {
                binding,
                request_id: crate::session::new_request_id(),
                command,
                dismiss: None,
            });
        }
    }
    pub(crate) fn show(&mut self, ui: &mut egui::Ui, binding: Option<Binding>) {
        if binding != self.binding {
            self.binding = binding;
            self.limits = None;
            self.status = None;
            self.original_request.clear();
            self.error = None;
            self.retry = None;
            self.saved.clear();
            if self.binding.is_some() && !self.busy {
                self.query(Command::NativeImagePreparationLimits);
            }
            // In-flight work remains bound to the old project. Its reply never updates this view.
        }
        while let Ok(reply) = self.receiver.try_recv() {
            self.busy = false;
            if Some(&reply.work.binding) != self.binding.as_ref() {
                continue;
            }
            match reply.saved {
                Ok(entries) => {
                    self.saved = entries
                        .into_iter()
                        .filter(|e| Some(&e.project) == self.binding.as_ref().map(|b| &b.0))
                        .collect()
                }
                Err(error) => self.error = Some(error),
            }
            match reply.result {
                Ok(Some(ResponseBody::NativeImagePreparationLimits(value))) => {
                    self.limits = Some(*value);
                    self.retry = None;
                }
                Ok(Some(ResponseBody::NativeImagePreparation(value))) => {
                    self.original_request = value.report.intent.original_request_id.to_string();
                    self.status = Some(*value);
                    self.retry = None;
                }
                Ok(None) => {
                    if reply
                        .work
                        .dismiss
                        .as_ref()
                        .is_some_and(|id| id.as_str() == self.original_request)
                    {
                        self.original_request.clear();
                        self.status = None;
                    }
                    self.retry = None;
                }
                Ok(_) => self.error = Some("Unexpected preparation response".into()),
                Err(error) => self.error = Some(error),
            }
        }
        ui.collapsing("CUDA image preparation",|ui| {
            ui.label("Prepare verified native bytes before live capture. Capture keeps its own deadline.");
            if self.binding.is_none(){ui.label("Open a project first.");return;}
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy,egui::Button::new("Inspect limits")).clicked(){self.query(Command::NativeImagePreparationLimits);}
                if self.busy {ui.spinner();}
            });
            if let Some(limits)=&self.limits {
                ui.label(if limits.supported {"CUDA preparation supported"}else{"CUDA preparation unavailable in this engine configuration"});
                ui.small(format!("Preparation maximum {} ms; retention maximum {} ms",limits.preparation_max_ms,limits.retention_max_ms));
            }
            ui.horizontal(|ui|{ui.label("Preparation ms");ui.add(egui::DragValue::new(&mut self.preparation_ms).range(1..=3_600_000));ui.label("Retention ms");ui.add(egui::DragValue::new(&mut self.retention_ms).range(1..=3_600_000));});
            let can_start=!self.busy && self.original_request.is_empty() && self.limits.as_ref().is_some_and(|l|l.supported&&self.preparation_ms<=l.preparation_max_ms&&self.retention_ms<=l.retention_max_ms);
            if ui.add_enabled(can_start,egui::Button::new("Start preparation")).clicked(){
                let request_id=crate::session::new_request_id();self.original_request=request_id.to_string();
                let limits=self.limits.as_ref().expect("inspected limits");
                self.send(Work {binding:self.binding.clone().expect("project"),request_id,dismiss:None,command:Command::StartNativeImagePreparation {engine_epoch:limits.engine_epoch.clone(),options:NativeImagePreparationOptions {preparation_ms:self.preparation_ms,retention_ms:self.retention_ms}}});
            }
            for intent in self.saved.clone() {
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.busy,egui::Button::new(format!("Recover {} (revision {}, epoch {})",intent.request_id,intent.revision.value(),intent.engine_epoch))).clicked() {
                        self.original_request=intent.request_id.to_string();
                        self.query(Command::ResolveNativeImagePreparation {original_request_id:intent.request_id.clone()});
                    }
                    if ui.add_enabled(!self.busy,egui::Button::new("Forget saved identity")).clicked() {
                        self.send(Work {binding:self.binding.clone().expect("project"),request_id:crate::session::new_request_id(),command:Command::NativeImagePreparationLimits,dismiss:Some(intent.request_id)});
                    }
                });
            }
            ui.small("Forgetting an identity does not cancel or release the engine operation.");
            ui.small("Saved identities survive reopening. Recover queries the original operation using this project's current revision.");
            ui.label("Original Start request ID");ui.add_enabled(!self.busy,egui::TextEdit::singleline(&mut self.original_request));
            if ui.add_enabled(!self.busy&&!self.original_request.is_empty(),egui::Button::new("Resolve Start")).clicked(){
                match RequestId::new(self.original_request.clone()) {Ok(original_request_id)=>self.query(Command::ResolveNativeImagePreparation {original_request_id}),Err(error)=>self.error=Some(error.to_string())}
            }
            if let Some(value)=self.status.clone() {
                ui.monospace(value.report.intent.operation_id.as_str());ui.label(format!("{:?}; native bytes available: {}; producer active: {}",value.report.state,value.available,value.producer_active));
                ui.label(format!("Physical storage cleanup confirmed: {}",value.report.cleanup_confirmed));
                if let Some(error)=value.report.error {ui.colored_label(egui::Color32::YELLOW,error);}
                let operation_id=value.report.intent.operation_id;
                ui.horizontal(|ui|{
                    if ui.add_enabled(!self.busy,egui::Button::new("Refresh status")).clicked(){self.query(Command::NativeImagePreparationStatus {operation_id:operation_id.clone()});}
                    if ui.add_enabled(!self.busy&&matches!(value.report.state,NativeImagePreparationState::Queued|NativeImagePreparationState::Preparing),egui::Button::new("Cancel preparation")).clicked(){self.query(Command::CancelNativeImagePreparation {operation_id:operation_id.clone()});}
                    if ui.add_enabled(!self.busy&&value.report.state==NativeImagePreparationState::Retained,egui::Button::new("Release retention")).clicked(){self.query(Command::ReleaseNativeImagePreparation {operation_id:operation_id.clone()});}
                    if ui.add_enabled(!self.busy&&value.report.state.is_terminal(),egui::Button::new("New preparation")).clicked(){self.status=None;self.original_request.clear();self.retry=None;}
                });
                ui.small("Availability describes retained bytes. GPU readiness, launch permission and physical playback require separate checks.");
            }
            if let Some(error)=&self.error {ui.colored_label(egui::Color32::YELLOW,error);}
            if let Some(work)=self.retry.clone(){
                ui.small(format!("Pending request {}",work.request_id));
                if ui.add_enabled(!self.busy,egui::Button::new("Retry same request")).clicked(){self.send(work);}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reopening_lists_saved_identities_without_issuing_start_or_resolve() {
        let (sender, requests) = mpsc::sync_channel(4);
        let (replies, receiver) = mpsc::sync_channel(4);
        let mut panel = NativePreparationPanel {
            sender,
            receiver,
            binding: None,
            busy: false,
            limits: None,
            status: None,
            preparation_ms: 1000,
            retention_ms: 2000,
            original_request: String::new(),
            error: None,
            retry: None,
            saved: vec![],
        };
        let binding = (ProjectId::new("project").unwrap(), RevisionId::new(1));
        let context = egui::Context::default();
        let _ = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| panel.show(ui, Some(binding.clone())));
        });
        let work = requests.try_recv().unwrap();
        assert!(matches!(
            work.command,
            Command::NativeImagePreparationLimits
        ));
        assert!(requests.try_recv().is_err());
        let intent = crate::native_preparation_recovery::Intent {
            project: binding.0.clone(),
            revision: RevisionId::new(0),
            request_id: RequestId::new("original").unwrap(),
            engine_epoch: AttemptId::new("old-epoch").unwrap(),
            options: NativeImagePreparationOptions {
                preparation_ms: 1000,
                retention_ms: 2000,
            },
        };
        replies
            .send(Reply {
                work,
                result: Err("engine unavailable".into()),
                saved: Ok(vec![intent.clone()]),
            })
            .unwrap();
        let _ = context.run(Default::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| panel.show(ui, Some(binding.clone())));
        });
        assert_eq!(panel.saved, vec![intent]);
        assert!(panel.original_request.is_empty());
        assert!(requests.try_recv().is_err());
        assert!(panel.status.is_none());
    }
}
