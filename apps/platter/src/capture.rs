//! The capture mode: `platter --capture <state> <file.png>` draws the interface from
//! invented data, with no engine and no audio device, and saves one frame as an image.
//!
//! It exists for the visual comparison with the reference screens of `docs/design/`. The
//! window is drawn at one pixel per point, 1440 by 900 or the size asked for, whatever the scale
//! of the screen.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui;
use platter_ui_egui::command::{DeckSize, SettingsPage};
use platter_ui_egui::layout::BuiltIn;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

use crate::png;

/// Frames to let the window settle at its size before the picture is taken, and frames to
/// wait for a size that never comes before taking it anyway.
const SETTLE_FRAMES: u32 = 4;
const GIVE_UP_FRAMES: u32 = 120;

/// The states that can be captured: one per reference screen of M2, the main window with compact
/// decks and with four decks, in the layouts Preparation and Browser, with four effect units, the
/// Decks, Mixer, Recording, Layouts and Library pages of the preferences with the maintenance of
/// the collection, a playlist and a history shown in the browser, and the details of one track
/// and of two.
pub const STATES: [&str; 21] = [
    "main",
    "compact",
    "four-decks",
    "stems",
    "stems-lanes",
    "layout-preparation",
    "layout-browser",
    "four-units",
    "playlist",
    "history",
    "startup",
    "preferences-audio",
    "preferences-general",
    "preferences-decks",
    "preferences-mixer",
    "preferences-recording",
    "preferences-layouts",
    "preferences-library",
    "maintenance",
    "details",
    "details-several",
];

fn scene_of(state: &str) -> Option<(Scene, Option<SettingsPage>)> {
    match state {
        "main" => Some((sample::playing(), None)),
        "compact" => {
            let mut scene = sample::playing();
            scene.settings.deck_size = DeckSize::Compact;
            Some((scene, None))
        }
        "four-decks" => Some((sample::four_decks(), None)),
        "stems" | "stems-lanes" => Some((sample::two_deck_stems(), None)),
        "layout-preparation" => Some((sample::in_layout(BuiltIn::Preparation), None)),
        "layout-browser" => Some((sample::in_layout(BuiltIn::Browser), None)),
        "four-units" => Some((sample::four_units(), None)),
        "playlist" | "details" => Some((sample::playlist(), None)),
        "details-several" => {
            let mut scene = sample::playlist();
            scene.browser.marked = vec![1, 2];
            scene.browser.selected = Some(1);
            Some((scene, None))
        }
        "history" => Some((sample::history(), None)),
        "startup" => Some((sample::startup(), None)),
        "preferences-audio" => Some((sample::playing(), Some(SettingsPage::Audio))),
        "preferences-general" => Some((sample::playing(), Some(SettingsPage::General))),
        "preferences-decks" => Some((sample::playing(), Some(SettingsPage::Decks))),
        "preferences-mixer" => Some((sample::playing(), Some(SettingsPage::Mixer))),
        "preferences-recording" => Some((sample::playing(), Some(SettingsPage::Recording))),
        "preferences-layouts" => Some((sample::playing(), Some(SettingsPage::Layouts))),
        "preferences-library" | "maintenance" => {
            Some((sample::playing(), Some(SettingsPage::Library)))
        }
        _ => None,
    }
}

struct CaptureApp {
    shell: Shell,
    /// The size of the picture, in points.
    size: egui::Vec2,
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
        let wanted = self.size;
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

/// The largest picture a capture takes, a side of a 4K screen.
const MAX_SIDE: f32 = 3_840.0;

/// Reads a size written `<width>x<height>`, from the designed 1440 by 900 up to a 4K screen.
pub fn size_of(text: &str) -> Result<egui::Vec2, String> {
    let wrong = || format!("the size `{text}` is not <width>x<height>, from 1440x900 to 3840x2160");
    let (width, height) = text.split_once(['x', 'X']).ok_or_else(wrong)?;
    let width: f32 = width.trim().parse().map_err(|_| wrong())?;
    let height: f32 = height.trim().parse().map_err(|_| wrong())?;
    let fits = |side: f32, least: f32| side.is_finite() && (least..=MAX_SIDE).contains(&side);
    if fits(width, DESIGN_WIDTH) && fits(height, DESIGN_HEIGHT) {
        Ok(egui::vec2(width.round(), height.round()))
    } else {
        Err(wrong())
    }
}

/// Opens the window in `state` at `size`, 1440 by 900 when none is given, saves one frame to
/// `file` and closes it.
pub fn run(state: &str, file: PathBuf, size: Option<egui::Vec2>) -> Result<(), String> {
    let size = size.unwrap_or(egui::vec2(DESIGN_WIDTH, DESIGN_HEIGHT));
    let (scene, page) = scene_of(state).ok_or_else(|| {
        format!(
            "unknown state `{state}`; the states are: {}",
            STATES.join(", ")
        )
    })?;
    let mut shell = Shell::new(scene.settings.language).map_err(|error| error.to_string())?;
    shell.state.settings_page = page;
    shell.state.maintenance = state == "maintenance";
    shell.state.stem_lanes[1] = state == "stems-lanes";
    shell.state.details_wanted = state.starts_with("details");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Platter DJ")
            .with_inner_size(size)
            .with_resizable(false),
        ..eframe::NativeOptions::default()
    };
    let app = CaptureApp {
        shell,
        size,
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

#[cfg(test)]
mod tests {
    use super::size_of;

    #[test]
    fn a_capture_size_is_read_from_the_design_size_to_a_4k_screen() {
        assert_eq!(size_of("1920x1080"), Ok(eframe::egui::vec2(1920.0, 1080.0)));
        assert_eq!(size_of("1440X900"), Ok(eframe::egui::vec2(1440.0, 900.0)));
        for wrong in [
            "1280x800",
            "8000x900",
            "1920",
            "widexhigh",
            "NaNx900",
            "1920x",
        ] {
            assert!(size_of(wrong).is_err(), "{wrong}");
        }
    }
}
