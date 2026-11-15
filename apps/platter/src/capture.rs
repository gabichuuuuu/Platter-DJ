//! The capture mode: `platter --capture <state> <file.png>` draws the interface from
//! invented data, with no engine and no audio device, and saves one frame as an image.
//!
//! It exists for the visual comparison with the reference screens of `docs/design/`. The
//! window is drawn at one pixel per point, 1440 by 900, whatever the scale of the screen.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use platter_ui_egui::command::SettingsPage;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

use crate::png;

/// Frames to let the window settle at its size before the picture is taken, and frames to
/// wait for a size that never comes before taking it anyway.
const SETTLE_FRAMES: u32 = 4;
const GIVE_UP_FRAMES: u32 = 120;

/// The states that can be captured: one per reference screen of M2, and the Mixer page of
/// the preferences.
pub const STATES: [&str; 5] = [
    "main",
    "startup",
    "preferences-audio",
    "preferences-general",
    "preferences-mixer",
];

fn scene_of(state: &str) -> Option<(Scene, Option<SettingsPage>)> {
    match state {
        "main" => Some((sample::playing(), None)),
        "startup" => Some((sample::startup(), None)),
        "preferences-audio" => Some((sample::playing(), Some(SettingsPage::Audio))),
        "preferences-general" => Some((sample::playing(), Some(SettingsPage::General))),
        "preferences-mixer" => Some((sample::playing(), Some(SettingsPage::Mixer))),
        _ => None,
    }
}

struct CaptureApp {
    shell: Shell,
    scene: Scene,
    file: PathBuf,
    frames: u32,
    settled: u32,
    requested: bool,
    /// Set by the screenshot callback once the file is written, or could not be.
    done: Arc<AtomicBool>,
}

fn save(image: &egui::ColorImage, file: &PathBuf) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let rgb: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|pixel| [pixel.r(), pixel.g(), pixel.b()])
        .collect();
    let mut out = std::io::BufWriter::new(std::fs::File::create(file)?);
    png::write_rgb(&mut out, image.size[0], image.size[1], &rgb)
}

impl eframe::App for CaptureApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        if self.done.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // One pixel per point, so the picture has the size of the reference screens.
        if let Some(native) = ctx.native_pixels_per_point()
            && (ctx.pixels_per_point() - 1.0).abs() > 0.001
        {
            ctx.set_zoom_factor(1.0 / native);
        }
        let _ = self.shell.show(ui, &self.scene.view());

        let size = ui.max_rect().size();
        let wanted = egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT);
        let right_size =
            (size - wanted).abs().max_elem() < 0.5 && (ctx.pixels_per_point() - 1.0).abs() < 0.001;
        if right_size {
            self.settled += 1;
        } else {
            self.settled = 0;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(wanted));
        }
        if !self.requested && (self.settled >= SETTLE_FRAMES || self.frames >= GIVE_UP_FRAMES) {
            self.requested = true;
            let (file, done) = (self.file.clone(), Arc::clone(&self.done));
            let callback = egui::ScreenshotCallback::new(move |image| {
                match save(&image, &file) {
                    Ok(()) => println!(
                        "{} x {} saved to {}",
                        image.size[0],
                        image.size[1],
                        file.display()
                    ),
                    Err(error) => eprintln!("cannot save {}: {error}", file.display()),
                }
                done.store(true, Ordering::Relaxed);
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(callback));
        }
        ctx.request_repaint();
    }
}

/// Opens the window in `state`, saves one frame to `file` and closes it.
pub fn run(state: &str, file: PathBuf) -> Result<(), String> {
    let (scene, page) = scene_of(state).ok_or_else(|| {
        format!(
            "unknown state `{state}`; the states are: {}",
            STATES.join(", ")
        )
    })?;
    let mut shell = Shell::new(scene.settings.language).map_err(|error| error.to_string())?;
    shell.state.settings_page = page;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Platter DJ")
            .with_inner_size([DESIGN_WIDTH, DESIGN_HEIGHT])
            .with_resizable(false),
        ..eframe::NativeOptions::default()
    };
    let app = CaptureApp {
        shell,
        scene,
        file,
        frames: 0,
        settled: 0,
        requested: false,
        done: Arc::new(AtomicBool::new(false)),
    };
    eframe::run_native(
        "Platter DJ",
        options,
        Box::new(move |creation| {
            app.shell.install(&creation.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| error.to_string())
}
