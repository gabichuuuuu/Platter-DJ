//! The window: where each zone sits, what the interface remembers between frames, and the
//! one function the application calls to draw a frame.
//!
//! The layout follows `docs/design/DESIGN.md`, "Layout of the main window": status header,
//! two waveform lanes, deck A, mixer and deck B, and the file selector to the bottom.

use egui::emath::GuiRounding as _;
use egui::{CornerRadius, Key, Rect, Sense};
use platter_core::DeckId;
use platter_engine::Command;

use crate::command::{Action, AppCommand, DeckPage, SettingsPage};
use crate::cx::{Control, Cx, FrameOutput, id};
use crate::i18n::{Language, TextError, Texts};
use crate::keymap::KeymapState;
use crate::model::{AudioState, View};
use crate::panels::browser::BrowserMemory;
use crate::panels::settings::{DIALOG_SIZE, SettingsMemory};
use crate::panels::waveform::{DEFAULT_ZOOM, ZOOM_SECONDS};
use crate::panels::{browser, deck, header, mixer, settings, waveform};
use crate::theme::{Theme, ThemeError};
use crate::widgets::meter::MeterMemory;

/// Width the layout is designed for. A narrower window is not designed yet; the
/// application scales the whole interface down to fit instead.
pub const DESIGN_WIDTH: f32 = 1_440.0;
pub const DESIGN_HEIGHT: f32 = 900.0;
const HEADER_HEIGHT: f32 = 40.0;
const NOTICE_HEIGHT: f32 = 44.0;
const LANES_HEIGHT: f32 = 178.0;
const DECKS_HEIGHT: f32 = 388.0;
const DECK_WIDTH: f32 = 520.0;
/// How much of the window shows through the veil behind the preferences.
const DIALOG_VEIL: f32 = 0.55;

/// What the interface remembers between frames. None of it is state of the engine: it is
/// how the engine is shown.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewState {
    /// The page of the preferences that is open, if the dialog is.
    pub settings_page: Option<SettingsPage>,
    /// Zoom step of each scrolling waveform.
    pub zoom: [usize; DeckId::COUNT],
    /// The large time of each deck is the time that remains.
    pub remaining_large: [bool; DeckId::COUNT],
    /// The page of the advanced panel each deck shows.
    pub deck_page: [DeckPage; DeckId::COUNT],
    /// The hotcues of each deck are in the mapping mode: the next one pressed takes the
    /// selected cue.
    pub cue_map: [bool; DeckId::COUNT],
    /// The meters of deck A, deck B and the master.
    pub meters: [MeterMemory; 3],
    pub browser: BrowserMemory,
    pub settings: SettingsMemory,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            settings_page: None,
            zoom: [DEFAULT_ZOOM; DeckId::COUNT],
            remaining_large: [true; DeckId::COUNT],
            deck_page: [DeckPage::default(); DeckId::COUNT],
            cue_map: [false; DeckId::COUNT],
            meters: [MeterMemory::default(); 3],
            browser: BrowserMemory::default(),
            settings: SettingsMemory::default(),
        }
    }
}

#[derive(Debug)]
pub enum ShellError {
    Theme(ThemeError),
    Texts(TextError),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Theme(error) => error.fmt(f),
            Self::Texts(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ShellError {}

pub struct Shell {
    theme: Theme,
    texts: Texts,
    pub state: ViewState,
    keys: KeymapState,
    /// The controls of the last frame, for the tests.
    controls: Vec<Control>,
}

impl Shell {
    pub fn new(language: Language) -> Result<Self, ShellError> {
        Ok(Self {
            theme: Theme::load().map_err(ShellError::Theme)?,
            texts: Texts::load(language).map_err(ShellError::Texts)?,
            state: ViewState::default(),
            keys: KeymapState::default(),
            controls: Vec::new(),
        })
    }

    /// Gives egui the fonts and the colours. Call it once, before the first frame.
    pub fn install(&self, ctx: &egui::Context) {
        self.theme.install(ctx);
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn texts(&self) -> &Texts {
        &self.texts
    }

    /// Replaces the texts of the interface.
    pub fn set_texts(&mut self, texts: Texts) {
        self.texts = texts;
    }

    pub fn set_language(&mut self, language: Language) -> Result<(), ShellError> {
        self.texts = Texts::load(language).map_err(ShellError::Texts)?;
        Ok(())
    }

    /// The controls drawn in the last frame.
    pub fn controls(&self) -> &[Control] {
        &self.controls
    }

    /// Applies a command that only changes what the window shows. The application passes
    /// every command of its own through here; the others are left alone.
    pub fn apply(&mut self, command: &AppCommand) {
        let state = &mut self.state;
        match command {
            AppCommand::SettingsOpen { page } => state.settings_page = Some(*page),
            AppCommand::SettingsClose => state.settings_page = None,
            AppCommand::WaveformZoomIn { deck } => {
                state.zoom[deck.index()] = state.zoom[deck.index()].saturating_sub(1);
            }
            AppCommand::WaveformZoomOut { deck } => {
                let step = &mut state.zoom[deck.index()];
                *step = (*step + 1).min(ZOOM_SECONDS.len() - 1);
            }
            AppCommand::DeckTimeDisplayToggle { deck } => {
                state.remaining_large[deck.index()] ^= true;
            }
            AppCommand::DeckPageSet { deck, page } => state.deck_page[deck.index()] = *page,
            AppCommand::DeckCueMapToggle { deck } => state.cue_map[deck.index()] ^= true,
            AppCommand::MixerClipReset { meter } => state.meters[meter.index()].reset_clip(),
            _ => {}
        }
    }

    /// Draws a frame of the whole window and returns what the user asked for in it.
    pub fn show(&mut self, ui: &mut egui::Ui, view: &View<'_>) -> Vec<Action> {
        let now = ui.input(|input| input.time);
        let engine = view.engine;
        for (memory, levels) in self.state.meters.iter_mut().zip([
            engine.decks[0].levels,
            engine.decks[1].levels,
            engine.master_levels,
        ]) {
            memory.update(levels, now);
        }
        // The mapping mode of the hotcues ends with the cue it was about: deleted, or gone
        // with its track.
        for (mapping, deck) in self.state.cue_map.iter_mut().zip(&engine.decks) {
            *mapping &= deck.cues.selected().is_some();
        }

        let mut out = FrameOutput::default();
        let screen = ui.max_rect();
        ui.painter()
            .rect_filled(screen, CornerRadius::ZERO, self.theme.tokens.color.bg_app);
        let dialog = self.state.settings_page;
        let mut state = self.state.clone();
        let zones = Zones::of(screen, view, self.theme.tokens.space.s1);
        {
            let mut cx = Cx {
                ui,
                theme: &self.theme,
                texts: &self.texts,
                out: &mut out,
                interactive: dialog.is_none(),
                help: view.settings.tooltips.then_some(view.keymap),
            };
            header::header(&mut cx, zones.header, view, &state);
            if let (Some(rect), AudioState::Stopped(problem)) = (zones.notice, &view.audio.state) {
                header::audio_notice(&mut cx, rect, *problem);
            }
            waveform::lanes(&mut cx, zones.lanes, &engine.decks, state.zoom);
            for (deck_id, rect) in DeckId::ALL.into_iter().zip(zones.decks) {
                deck::deck(&mut cx, rect, deck_id, view, &state);
            }
            mixer::mixer(&mut cx, zones.mixer, view, &state);
            browser::browser(&mut cx, zones.browser, view, &mut state.browser);
            if cx.interactive {
                dropped_files(&mut cx, &zones, view);
            }
        }
        if let Some(page) = dialog {
            // Whatever lies behind the dialog is dimmed and takes no input.
            let veil = self.theme.tokens.color.bg_app.gamma_multiply(DIALOG_VEIL);
            ui.painter().rect_filled(screen, CornerRadius::ZERO, veil);
            ui.interact(screen, id("dialog-backdrop"), Sense::click_and_drag());
            let rect = Rect::from_center_size(screen.center(), DIALOG_SIZE).round_ui();
            let mut cx = Cx {
                ui,
                theme: &self.theme,
                texts: &self.texts,
                out: &mut out,
                interactive: true,
                help: view.settings.tooltips.then_some(view.keymap),
            };
            settings::settings(&mut cx, rect, page, view, &mut state.settings);
            let escape = ui.input(|input| input.key_pressed(Key::Escape));
            if escape && !out.is_claimed(Key::Escape, &egui::Modifiers::NONE) {
                out.emit(AppCommand::SettingsClose);
            }
        }
        self.state = state;

        // Shortcuts last: by now the focused control has said which keys it uses itself.
        let focused = ui.input(|input| input.focused);
        let mut actions = if focused {
            let events = ui.input(|input| input.events.clone());
            self.keys.actions(view.keymap, &events, |key, modifiers| {
                out.is_claimed(key, modifiers)
            })
        } else {
            self.keys.release_all(view.keymap)
        };
        // With the preferences open, the shortcuts of the decks and the mixer rest.
        if dialog.is_some() {
            actions.retain(|action| matches!(action, Action::App(_)));
        }
        out.actions.extend(actions);
        self.controls = out.controls;
        out.actions
    }
}

/// The rectangles of the main window.
struct Zones {
    header: Rect,
    notice: Option<Rect>,
    lanes: Rect,
    decks: [Rect; DeckId::COUNT],
    mixer: Rect,
    browser: Rect,
}

impl Zones {
    fn of(screen: Rect, view: &View<'_>, seam: f32) -> Self {
        let band = |top: f32, height: f32| {
            Rect::from_min_size(
                egui::pos2(screen.left(), top),
                egui::vec2(screen.width(), height),
            )
        };
        let header = band(screen.top(), HEADER_HEIGHT);
        let mut top = header.bottom() + seam;
        let notice = matches!(view.audio.state, AudioState::Stopped(_)).then(|| {
            let rect = band(top, NOTICE_HEIGHT);
            top = rect.bottom() + seam;
            rect
        });
        let lanes = band(top, LANES_HEIGHT);
        top = lanes.bottom() + seam;
        // Decks and mixer keep their width; in a wider window they stay in the middle.
        let left = (screen.center().x - DESIGN_WIDTH / 2.0)
            .max(screen.left())
            .round();
        let deck_a =
            Rect::from_min_size(egui::pos2(left, top), egui::vec2(DECK_WIDTH, DECKS_HEIGHT));
        let deck_b = Rect::from_min_size(
            egui::pos2(left + DESIGN_WIDTH - DECK_WIDTH, top),
            egui::vec2(DECK_WIDTH, DECKS_HEIGHT),
        );
        let mixer = Rect::from_min_max(
            egui::pos2(deck_a.right() + seam, top),
            egui::pos2(deck_b.left() - seam, top + DECKS_HEIGHT),
        );
        top += DECKS_HEIGHT + seam;
        let browser = Rect::from_min_max(
            egui::pos2(screen.left(), top),
            screen.max.max(egui::pos2(screen.left(), top)),
        );
        Self {
            header,
            notice,
            lanes,
            decks: [deck_a, deck_b],
            mixer,
            browser,
        }
    }
}

/// Files dropped on the window from outside are loaded into the deck under the pointer;
/// when the system does not say where they fell, into the first empty deck, or deck A.
fn dropped_files(cx: &mut Cx<'_>, zones: &Zones, view: &View<'_>) {
    let (files, pointer) = cx.ui.input(|input| {
        let files: Vec<_> = input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .collect();
        (files, input.pointer.latest_pos())
    });
    let Some(path) = files.into_iter().next() else {
        return;
    };
    let under_pointer = pointer.and_then(|pos| {
        DeckId::ALL
            .into_iter()
            .zip(zones.decks)
            .find(|(_, rect)| rect.contains(pos))
            .map(|(deck, _)| deck)
    });
    let deck = under_pointer.unwrap_or_else(|| {
        DeckId::ALL
            .into_iter()
            .find(|deck| {
                let snapshot = &view.engine.decks[deck.index()];
                snapshot.track.is_none() && !snapshot.loading
            })
            .unwrap_or(DeckId::A)
    });
    cx.out.emit(Command::DeckLoad { deck, path });
}
