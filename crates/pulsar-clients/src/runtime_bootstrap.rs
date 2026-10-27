//! Local installation presentation. Filesystem authority stays in the executable
//! host callback; no Engine, device, model or policy operation originates here.
use anyhow::{bail, Result};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

type Prepare = Arc<dyn Fn() -> Result<PathBuf> + Send + Sync>;
#[derive(Debug, PartialEq, Eq)]
enum Phase {
    Running,
    Failed(String),
    Ready(PathBuf),
    Cancelled,
}
struct Preparation {
    callback: Prepare,
    pending: Option<mpsc::Receiver<std::result::Result<PathBuf, String>>>,
    phase: Phase,
}
impl Preparation {
    fn new(callback: Prepare) -> Self {
        let mut value = Self {
            callback,
            pending: None,
            phase: Phase::Running,
        };
        value.start();
        value
    }
    fn start(&mut self) {
        if self.pending.is_some() || matches!(self.phase, Phase::Ready(_) | Phase::Cancelled) {
            return;
        }
        let callback = self.callback.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("pulsar-runtime-preparation".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback()));
                let result = match outcome {
                    Ok(Ok(path)) if path.is_absolute() => Ok(path),
                    Ok(Ok(_)) => Err("Setup produced an invalid installation location.".into()),
                    Ok(Err(error)) => Err(format!("{error:#}").chars().take(4096).collect()),
                    Err(_) => Err("Setup stopped unexpectedly. Reopen Pulsar to retry.".into()),
                };
                // Closing the window drops its receiver; no late result can launch.
                let _ = sender.send(result);
            }) {
            Ok(_) => {
                self.pending = Some(receiver);
                self.phase = Phase::Running;
            }
            Err(error) => self.phase = Phase::Failed(format!("Cannot start setup: {error}")),
        }
    }
    fn poll(&mut self) {
        let Some(receiver) = &self.pending else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(path)) => {
                self.pending = None;
                self.phase = Phase::Ready(path);
            }
            Ok(Err(message)) => {
                self.pending = None;
                self.phase = Phase::Failed(message);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                self.phase = Phase::Failed("Setup stopped before completion.".into());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    fn cancel(&mut self) {
        self.pending = None;
        self.phase = Phase::Cancelled;
    }
}

struct Bootstrap {
    preparation: Preparation,
    ready: Arc<Mutex<Option<PathBuf>>>,
}
impl eframe::App for Bootstrap {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        if context.input(|input| input.viewport().close_requested())
            && !matches!(self.preparation.phase, Phase::Ready(_))
        {
            self.preparation.cancel();
            return;
        }
        self.preparation.poll();
        if let Phase::Ready(path) = &self.preparation.phase {
            match self.ready.lock() {
                Ok(mut ready) => *ready = Some(path.clone()),
                Err(_) => {
                    self.preparation.phase =
                        Phase::Failed("Setup result could not be retained.".into())
                }
            }
            if matches!(self.preparation.phase, Phase::Ready(_)) {
                context.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        show(context, &mut self.preparation);
        if matches!(self.preparation.phase, Phase::Cancelled) {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if matches!(self.preparation.phase, Phase::Running) {
            context.request_repaint_after(Duration::from_millis(50));
        }
    }
}
fn show(context: &egui::Context, preparation: &mut Preparation) {
    egui::CentralPanel::default().show(context, |ui| {
        ui.add_space(16.0);
        ui.heading("Preparing Pulsar");
        ui.add_space(12.0);
        match &preparation.phase {
            Phase::Running => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking the offline installation.");
                });
                ui.label("First use may take longer while the bundled files are installed.");
                ui.add_space(12.0);
                if ui.button("Cancel").clicked() {
                    preparation.cancel();
                }
            }
            Phase::Failed(message) => {
                ui.colored_label(egui::Color32::YELLOW, "Setup could not finish");
                ui.label("Your existing installations have been preserved.");
                egui::CollapsingHeader::new("Details")
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                ui.label(message);
                            });
                    });
                ui.horizontal(|ui| {
                    if ui.button("Try again").clicked() {
                        preparation.start();
                    }
                    if ui.button("Close").clicked() {
                        preparation.cancel();
                    }
                });
            }
            Phase::Ready(_) | Phase::Cancelled => {}
        }
    });
}

/// Show setup before graphical launch. No callback starts if native window
/// creation fails. A close returns an error; the composition root must end the
/// bootstrap process so its preparation thread cannot outlive cancellation.
/// Interrupted private stages retain the installer's existing recovery rules.
pub fn run_runtime_bootstrap<F>(prepare: F) -> Result<PathBuf>
where
    F: Fn() -> Result<PathBuf> + Send + Sync + 'static,
{
    let ready = Arc::new(Mutex::new(None));
    let output = ready.clone();
    let callback: Prepare = Arc::new(prepare);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([540.0, 320.0])
            .with_min_inner_size([420.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Pulsar Setup",
        options,
        Box::new(move |_| {
            Ok(Box::new(Bootstrap {
                preparation: Preparation::new(callback),
                ready,
            }))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Setup window could not open: {error}"))?;
    let mut result = output
        .lock()
        .map_err(|_| anyhow::anyhow!("Setup result unavailable"))?;
    match result.take() {
        Some(path) => Ok(path),
        None => bail!("Pulsar setup was closed before completion; reopen Pulsar to retry"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };
    fn settle(value: &mut Preparation) {
        let until = Instant::now() + Duration::from_secs(2);
        while matches!(value.phase, Phase::Running) {
            value.poll();
            assert!(Instant::now() < until, "preparation test deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn busy_work_is_off_thread_and_cancellation_ignores_late_success() {
        let (release, wait) = mpsc::sync_channel(1);
        let wait = Mutex::new(wait);
        let (finished, done) = mpsc::sync_channel(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let mut value = Preparation::new(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            wait.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            finished.send(()).unwrap();
            Ok(PathBuf::from(crate::test_host_path("/verified")))
        }));
        value.start();
        value.poll();
        assert_eq!(value.phase, Phase::Running);
        value.cancel();
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(2)).unwrap();
        value.poll();
        assert_eq!(value.phase, Phase::Cancelled);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn failure_is_bounded_and_explicit_retry_rechecks_before_success() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let mut value = Preparation::new(Arc::new(move || {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                bail!("{}", "x".repeat(8000));
            }
            Ok(PathBuf::from(crate::test_host_path("/verified")))
        }));
        settle(&mut value);
        assert!(matches!(&value.phase,Phase::Failed(message) if message.chars().count()==4096));
        value.start();
        settle(&mut value);
        assert_eq!(
            value.phase,
            Phase::Ready(PathBuf::from(crate::test_host_path("/verified")))
        );
        value.start();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn invalid_host_result_and_panics_never_become_ready() {
        for callback in [
            Arc::new(|| Ok(PathBuf::from("relative"))) as Prepare,
            Arc::new(|| -> Result<PathBuf> { panic!("authored preparation panic") }) as Prepare,
        ] {
            let mut value = Preparation::new(callback);
            settle(&mut value);
            assert!(matches!(value.phase, Phase::Failed(_)));
        }
    }
    #[test]
    fn failure_panel_has_error_retry_and_close_without_engine() {
        let context = egui::Context::default();
        let mut value = Preparation {
            callback: Arc::new(|| panic!("render must not prepare")),
            pending: None,
            phase: Phase::Failed("cache is busy".into()),
        };
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(540.0, 320.0),
                )),
                ..Default::default()
            },
            |ctx| show(ctx, &mut value),
        );
        let text = output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        for expected in [
            "Setup could not finish",
            "cache is busy",
            "Try again",
            "Close",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(matches!(value.phase, Phase::Failed(_)));
    }
}
