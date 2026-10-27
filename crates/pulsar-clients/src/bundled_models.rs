//! Host-supplied discovery presentation. No filesystem intake or model authority.
use anyhow::{ensure, Result};
use eframe::egui;
use pulsar_protocol::{validate_sha256, ModelPackManifest};
use serde::Deserialize;
use std::{
    path::{Component, PathBuf},
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
};

pub type BundledModelDiscovery = Arc<dyn Fn() -> Result<String> + Send + Sync>;
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    schema_version: u16,
    runtime_manifest_sha256: String,
    directory: PathBuf,
    packs: Vec<BundledPack>,
    integrity_verified: bool,
    native_execution_approved: bool,
    rights_verified: bool,
    qualified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> String {
        let runtime = "a".repeat(64);
        let pack = "b".repeat(64);
        let assets=[("model.onnx","model"),("runtime.bin","runtime"),("profile.json","profile"),("LICENSE","license")].iter().map(|(path,role)|serde_json::json!({"path":path,"role":role,"byte_len":1,"sha256":"c".repeat(64)})).collect::<Vec<_>>();
        serde_json::json!({"schema_version":1,"runtime_manifest_sha256":runtime,
            "directory":crate::test_host_path(&format!("/fixture/cache/{runtime}")),"packs":[{"manifest_sha256":pack,
            "source":crate::test_host_path(&format!("/fixture/cache/{runtime}/generation/{pack}")),"manifest":{
                "schema_version":1,"pack_id":"authored-display-fixture","target_os":"linux","target_arch":"x86_64",
                "ort_api_version":24,"labels":["fixture"],"assets":assets,
                "license_path":"LICENSE","source_description":"Authored presentation fixture; no execution evidence"}}],
            "integrity_verified":true,"native_execution_approved":false,"rights_verified":false,"qualified":false}).to_string()
    }
    #[test]
    fn untrusted_discovery_bytes_cannot_promote_authority_or_change_namespace() {
        assert!(parse(&fixture()).is_ok());
        for mutate in [
            "native_execution_approved",
            "rights_verified",
            "qualified",
            "source",
            "directory_parent",
            "duplicate",
            "unknown",
        ] {
            let mut value: serde_json::Value = serde_json::from_str(&fixture()).unwrap();
            match mutate {
                "source" => {
                    value["packs"][0]["source"] =
                        serde_json::json!(crate::test_host_path("/outside"))
                }
                "directory_parent" => {
                    let runtime = "a".repeat(64);
                    value["directory"] = serde_json::json!(crate::test_host_path(&format!(
                        "/fixture/cache/../{runtime}"
                    )));
                }
                "duplicate" => {
                    let pack = value["packs"][0].clone();
                    value["packs"].as_array_mut().unwrap().push(pack);
                }
                "unknown" => value["extra"] = serde_json::json!(true),
                field => value[field] = serde_json::json!(true),
            }
            assert!(parse(&value.to_string()).is_err(), "{mutate}");
        }
        assert!(parse(&" ".repeat(256 * 1024 + 1)).is_err());
    }
    #[test]
    fn discovery_runs_off_thread_bounds_failures_and_can_retry() {
        let main = std::thread::current().id();
        let bytes = fixture();
        let mut panel = BundledModelPanel::default();
        panel.configure(Arc::new(move || {
            assert_ne!(std::thread::current().id(), main);
            Ok(bytes.clone())
        }));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while panel.receiver.is_some() {
            assert!(std::time::Instant::now() < deadline);
            panel.poll();
            std::thread::yield_now();
        }
        assert_eq!(panel.inventory.as_ref().unwrap().packs.len(), 1);
        panel.configure(Arc::new(|| Err(anyhow::anyhow!("x".repeat(8192)))));
        while panel.receiver.is_some() {
            assert!(std::time::Instant::now() < deadline);
            panel.poll();
            std::thread::yield_now();
        }
        assert_eq!(panel.error.as_ref().unwrap().len(), 4096);
        assert!(panel.inventory.is_none());
        panel.configure(Arc::new(|| Ok(fixture())));
        while panel.receiver.is_some() {
            assert!(std::time::Instant::now() < deadline);
            panel.poll();
            std::thread::yield_now();
        }
        assert!(panel.inventory.is_some());
    }
    fn draw(
        ctx: &egui::Context,
        panel: &mut BundledModelPanel,
        busy: bool,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<BundledPack>) {
        let mut choice = None;
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    choice = panel.ui(ui, busy);
                });
            },
        );
        (output, choice)
    }
    fn text(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(s) if s.galley.job.text == label => {
                Some(s.pos + s.galley.rect.center().to_vec2())
            }
            egui::epaint::Shape::Vec(v) => v.iter().find_map(|s| text(s, label)),
            _ => None,
        }
    }
    fn click(
        ctx: &egui::Context,
        panel: &mut BundledModelPanel,
        busy: bool,
    ) -> Option<BundledPack> {
        draw(ctx, panel, busy, vec![]);
        let output = draw(ctx, panel, busy, vec![]).0;
        let at = output
            .shapes
            .iter()
            .find_map(|s| text(&s.shape, "Prepare pack import"))
            .expect("drawn import preparation control");
        draw(
            ctx,
            panel,
            busy,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        draw(
            ctx,
            panel,
            busy,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        )
        .1
    }
    #[test]
    fn drawn_choice_prepares_only_metadata_and_is_disabled_during_other_work() {
        let ctx = egui::Context::default();
        let mut panel = BundledModelPanel {
            inventory: Some(parse(&fixture()).unwrap()),
            ..Default::default()
        };
        assert!(click(&ctx, &mut panel, true).is_none());
        let choice = click(&ctx, &mut panel, false).expect("explicit selection");
        assert_eq!(choice.manifest_sha256, "b".repeat(64));
        assert!(panel.inventory.is_some());
        assert!(panel.receiver.is_none());
    }
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BundledPack {
    pub manifest_sha256: String,
    pub source: PathBuf,
    pub manifest: ModelPackManifest,
}
fn parse(bytes: &str) -> Result<Inventory> {
    ensure!(
        bytes.len() <= 256 * 1024,
        "Bundled inventory exceeds presentation limit"
    );
    let inventory: Inventory = serde_json::from_str(bytes)?;
    validate_sha256(&inventory.runtime_manifest_sha256)?;
    ensure!(
        inventory.schema_version == 1
            && inventory.integrity_verified
            && !inventory.native_execution_approved
            && !inventory.rights_verified
            && !inventory.qualified
            && inventory.packs.len() <= 8,
        "Invalid bundled inventory authority"
    );
    ensure!(
        inventory.directory.is_absolute()
            && inventory.directory.components().all(|p| matches!(
                p,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            ))
            && inventory.directory.file_name().and_then(|p| p.to_str())
                == Some(inventory.runtime_manifest_sha256.as_str()),
        "Invalid bundled runtime path"
    );
    let mut seen = std::collections::BTreeSet::new();
    for pack in &inventory.packs {
        validate_sha256(&pack.manifest_sha256)?;
        pack.manifest.validate()?;
        ensure!(
            seen.insert(&pack.manifest_sha256)
                && pack.source
                    == inventory
                        .directory
                        .join("generation")
                        .join(&pack.manifest_sha256),
            "Bundled pack path or identity differs"
        );
    }
    Ok(inventory)
}
#[derive(Default)]
pub(crate) struct BundledModelPanel {
    discover: Option<BundledModelDiscovery>,
    receiver: Option<Receiver<Result<Inventory, String>>>,
    inventory: Option<Inventory>,
    error: Option<String>,
}
impl BundledModelPanel {
    pub fn configure(&mut self, discover: BundledModelDiscovery) {
        self.discover = Some(discover);
        self.start();
    }
    fn start(&mut self) {
        if self.receiver.is_some() {
            return;
        }
        let Some(discover) = self.discover.clone() else {
            return;
        };
        self.inventory = None;
        self.error = None;
        let (sender, receiver) = mpsc::sync_channel(1);
        self.receiver = Some(receiver);
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                discover().and_then(|s| parse(&s))
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Bundled discovery stopped unexpectedly")))
            .map_err(|e| e.to_string().chars().take(4096).collect());
            let _ = sender.send(result);
        });
    }
    fn poll(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(inventory)) => {
                self.inventory = Some(inventory);
                self.receiver = None;
            }
            Ok(Err(error)) => {
                self.error = Some(error);
                self.receiver = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = Some("Bundled discovery stopped before returning a report".into());
                self.receiver = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, busy: bool) -> Option<BundledPack> {
        self.poll();
        let mut choice = None;
        egui::CollapsingHeader::new("Bundled models")
            .default_open(true)
            .show(ui, |ui| {
                if self.receiver.is_some() {
                    ui.spinner();
                    ui.label("Checking bundled files…");
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
                if let Some(error) = &self.error {
                    ui.label(error);
                }
                if let Some(inventory) = &self.inventory {
                    if inventory.packs.is_empty() {
                        ui.label("This runtime has no bundled generation models.");
                    }
                    for pack in &inventory.packs {
                        ui.group(|ui| {
                            ui.label(pack.manifest.pack_id.as_str());
                            ui.label(&pack.manifest.source_description);
                            ui.small(format!(
                                "{} declared labels. Import and execution approval are separate.",
                                pack.manifest.labels.len()
                            ));
                            if ui
                                .add_enabled(!busy, egui::Button::new("Prepare pack import"))
                                .clicked()
                            {
                                choice = Some(pack.clone());
                            }
                        });
                    }
                }
                if self.discover.is_none() {
                    ui.label("Bundled discovery is unavailable in this launcher.");
                } else if ui
                    .add_enabled(
                        self.receiver.is_none(),
                        egui::Button::new("Refresh bundled models"),
                    )
                    .clicked()
                {
                    self.start();
                }
            });
        choice
    }
}
