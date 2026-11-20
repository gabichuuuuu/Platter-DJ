//! The window: where each zone sits, what the interface remembers between frames, and the
//! one function the application calls to draw a frame.
//!
//! The layout follows `docs/design/DESIGN.md`, "Layout of the main window": status header,
//! two waveform lanes, deck A, mixer and deck B, and the file selector to the bottom; or, in
//! the four-deck layout, decks A and C, mixer, decks B and D, and the file selector.

use egui::emath::GuiRounding as _;
use egui::{CornerRadius, Key, Rect, Sense};
use platter_core::DeckId;
use platter_engine::Command;
use platter_library::{MAX_RULES, NodeId, parse_rules};

use crate::command::{
    Action, AppCommand, DeckLayout, DeckPage, DeckSize, DetailField, DetailFields, DetailsThen,
    MeterId, RuleField, SettingsPage, TrackDetails,
};
use crate::cx::{Control, Cx, FrameOutput, id};
use crate::i18n::{Language, TextError, Texts};
use crate::keymap::KeymapState;
use crate::model::{AudioState, CollectionView, FileRow, RowKind, View};
use crate::panels::browser::BrowserMemory;
use crate::panels::details::{DetailsEdit, DetailsForm};
use crate::panels::rules::{RuleRow, RulesForm};
use crate::panels::settings::{DIALOG_SIZE, SettingsMemory};
use crate::panels::waveform::{DEFAULT_ZOOM, ZOOM_SECONDS};
use crate::panels::{
    browser, deck, details, effects, header, maintenance, mixer, rules, settings, waveform,
};
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
/// The band of compact decks: the height the mixer needs, whose crossfader row ends 343 points
/// down, with the padding of a panel under it.
const COMPACT_DECKS_HEIGHT: f32 = 356.0;
const DECK_WIDTH: f32 = 520.0;
/// The band of the four-deck layout: two decks of 289 points, one over the other.
const FOUR_DECKS_HEIGHT: f32 = 580.0;
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
    /// The meters of the decks, deck A first, and the master: [`MeterId::index`].
    pub meters: [MeterMemory; MeterId::ALL.len()],
    pub browser: BrowserMemory,
    pub settings: SettingsMemory,
    /// The dialog of the maintenance of the collection is open over the preferences.
    pub maintenance: bool,
    /// *Remove missing* waits for its answer.
    pub remove_asked: bool,
    /// The dialog of the details of the selected tracks is asked for, and what it holds once it
    /// is open.
    pub details_wanted: bool,
    pub details: Option<DetailsForm>,
    /// The position the dialog shows next: 0 for all the selected tracks, 1 and up for one.
    pub details_position: usize,
    /// What the dialog saved since it opened, shown at once while the library answers.
    pub details_edits: Vec<DetailsEdit>,
    /// The smart playlist whose dialog of rules is asked for, and what it holds once open.
    pub rules_wanted: Option<NodeId>,
    pub rules: Option<RulesForm>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            settings_page: None,
            zoom: [DEFAULT_ZOOM; DeckId::COUNT],
            remaining_large: [true; DeckId::COUNT],
            deck_page: [DeckPage::default(); DeckId::COUNT],
            cue_map: [false; DeckId::COUNT],
            meters: [MeterMemory::default(); MeterId::ALL.len()],
            browser: BrowserMemory::default(),
            settings: SettingsMemory::default(),
            maintenance: false,
            remove_asked: false,
            details_wanted: false,
            details: None,
            details_position: 0,
            details_edits: Vec::new(),
            rules_wanted: None,
            rules: None,
        }
    }
}

impl ViewState {
    fn close_rules(&mut self) {
        self.rules_wanted = None;
        self.rules = None;
    }

    /// The commands of the dialog of the rules of a smart playlist.
    fn edit_rules(&mut self, command: &AppCommand) {
        match command {
            AppCommand::BrowserSmartRulesOpen { smart } => {
                self.rules_wanted = Some(*smart);
                self.rules = None;
            }
            AppCommand::BrowserSmartRulesClose | AppCommand::BrowserSmartRulesSet { .. } => {
                self.close_rules();
            }
            _ => {}
        }
        let Some(form) = &mut self.rules else {
            return;
        };
        match command {
            AppCommand::BrowserSmartRuleAdd if form.rows.len() < MAX_RULES => {
                form.rows.push(RuleRow::new(RuleField::Genre));
            }
            AppCommand::BrowserSmartRuleRemove { row } if *row < form.rows.len() => {
                form.rows.remove(*row);
            }
            AppCommand::BrowserSmartRuleField { row, field } => {
                if let Some(row) = form.rows.get_mut(*row) {
                    *row = RuleRow::new(*field);
                }
            }
            AppCommand::BrowserSmartRuleChoice { row, value } => {
                if let Some(row) = form.rows.get_mut(*row) {
                    row.choice = *value;
                }
            }
            _ => {}
        }
    }

    fn close_details(&mut self) {
        self.details_wanted = false;
        self.details = None;
        self.details_edits.clear();
    }

    /// Keeps what a save of the dialog wrote, then closes the dialog or shows the position
    /// before or after. `docs/behavior/browser.md`, *Track details*.
    fn saved_details(
        &mut self,
        details: &TrackDetails,
        fields: DetailFields,
        position: usize,
        then: DetailsThen,
    ) {
        let Some(form) = &self.details else {
            self.close_details();
            return;
        };
        let count = form.count;
        if position > count || (count == 1 && position > 0) {
            self.close_details();
            return;
        }
        if !fields.is_empty() {
            self.details_edits.push(DetailsEdit {
                tracks: form.tracks_at(position),
                details: details.clone(),
                fields,
            });
        }
        let to = match then {
            DetailsThen::Previous if count > 1 => position.saturating_sub(1),
            DetailsThen::Next if count > 1 => (position + 1).min(count),
            _ => {
                self.close_details();
                return;
            }
        };
        self.details = None;
        self.details_wanted = true;
        self.details_position = to;
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
            AppCommand::SettingsClose => {
                state.settings_page = None;
                state.maintenance = false;
            }
            AppCommand::LibraryMaintenanceOpen => {
                state.maintenance = state.settings_page.is_some();
                state.remove_asked = false;
            }
            AppCommand::LibraryMaintenanceClose => {
                state.maintenance = false;
                state.remove_asked = false;
            }
            AppCommand::LibraryMissingAsk => state.remove_asked = true,
            AppCommand::BrowserTrackDetailsOpen => {
                state.details_wanted = true;
                state.details_position = 0;
                state.details_edits.clear();
            }
            AppCommand::BrowserTrackDetailsClose => state.close_details(),
            AppCommand::BrowserSmartRulesOpen { .. }
            | AppCommand::BrowserSmartRulesClose
            | AppCommand::BrowserSmartRulesSet { .. }
            | AppCommand::BrowserSmartRuleAdd
            | AppCommand::BrowserSmartRuleRemove { .. }
            | AppCommand::BrowserSmartRuleField { .. }
            | AppCommand::BrowserSmartRuleChoice { .. } => state.edit_rules(command),
            AppCommand::BrowserTrackDetailsSave {
                details,
                fields,
                position,
                then,
            } => state.saved_details(details, *fields, *position, *then),
            AppCommand::BrowserTrackDetailsRating { rating } => {
                if let Some(form) = &mut state.details {
                    form.details.rating = (*rating).min(5);
                    form.ticked.insert(DetailField::Rating);
                }
            }
            AppCommand::BrowserTrackDetailsTick { field } => {
                if let Some(form) = &mut state.details {
                    form.ticked.toggle(*field);
                }
            }
            AppCommand::LibraryMissingKeep | AppCommand::LibraryMissingRemove => {
                state.remove_asked = false;
            }
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
            AppCommand::BrowserSearchFocus => state.browser.focus_search = true,
            _ => {}
        }
    }

    /// Draws a frame of the whole window and returns what the user asked for in it.
    pub fn show(&mut self, ui: &mut egui::Ui, view: &View<'_>) -> Vec<Action> {
        let now = ui.input(|input| input.time);
        self.theme.wave_colour = view.settings.wave_colour;
        let engine = view.engine;
        let levels = MeterId::ALL.map(|meter| match meter {
            MeterId::Deck(deck) => engine.decks[deck.index()].levels,
            MeterId::Master => engine.master_levels,
        });
        for (memory, levels) in self.state.meters.iter_mut().zip(levels) {
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
        // The dialog of the details opens on the selected tracks of the library, and closes when
        // the selection is no longer the one it opened on.
        if self.state.details_wanted && self.state.details.is_none() {
            self.state.details =
                details_of(view, self.state.details_position, &self.state.details_edits);
            self.state.details_wanted = self.state.details.is_some();
        }
        if self
            .state
            .details
            .as_ref()
            .is_some_and(|form| form.tracks != track_paths(&selected_tracks(view)))
        {
            self.state.close_details();
        }
        // The dialog of the rules opens on the smart playlist of the tree, and closes when it
        // is gone.
        if let Some(smart) = self.state.rules_wanted {
            let row = view.browser.collection.as_ref().and_then(|collection| {
                collection
                    .playlists
                    .iter()
                    .find(|row| row.id == smart && row.smart)
            });
            match (row, &self.state.rules) {
                (None, _) => self.state.close_rules(),
                (Some(row), None) => {
                    self.state.rules = Some(RulesForm {
                        smart,
                        name: row.name.clone(),
                        rows: parse_rules(&row.rules)
                            .unwrap_or_default()
                            .iter()
                            .map(RuleRow::of)
                            .collect(),
                    });
                }
                (Some(_), Some(_)) => {}
            }
        }
        let dialog = self.state.settings_page;
        // The dialog of the details goes first; the one of the rules waits behind it.
        let details_open = self.state.details.is_some() && dialog.is_none();
        let rules_open = self.state.rules.is_some() && dialog.is_none() && !details_open;
        let mut state = self.state.clone();
        let zones = Zones::of(screen, view, self.theme.tokens.space.s1);
        {
            let mut cx = Cx {
                ui,
                theme: &self.theme,
                texts: &self.texts,
                out: &mut out,
                interactive: dialog.is_none() && !details_open && !rules_open,
                help: view.settings.tooltips.then_some(view.keymap),
            };
            header::header(&mut cx, zones.header, view, &state);
            if let (Some(rect), AudioState::Stopped(problem)) = (zones.notice, &view.audio.state) {
                header::audio_notice(&mut cx, rect, *problem);
            }
            if let Some(lanes) = zones.lanes {
                waveform::lanes(&mut cx, lanes, &engine.decks, state.zoom);
            }
            for (deck_id, rect) in zones.decks.iter().copied() {
                match view.settings.deck_layout {
                    DeckLayout::Two => deck::deck(&mut cx, rect, deck_id, view, &state),
                    DeckLayout::Four => deck::four_deck(&mut cx, rect, deck_id, view, &state),
                }
            }
            effects::row(&mut cx, zones.effects, view);
            mixer::mixer(&mut cx, zones.mixer, view, &state);
            browser::browser(&mut cx, zones.browser, view, &mut state.browser);
            if cx.interactive {
                dropped_files(&mut cx, &zones, view);
            }
        }
        if let Some(form) = state.details.as_mut().filter(|_| details_open) {
            let veil = self.theme.tokens.color.bg_app.gamma_multiply(DIALOG_VEIL);
            ui.painter().rect_filled(screen, CornerRadius::ZERO, veil);
            ui.interact(screen, id("details-backdrop"), Sense::click_and_drag());
            let rect = Rect::from_center_size(screen.center(), details::DETAILS_SIZE).round_ui();
            let mut cx = Cx {
                ui,
                theme: &self.theme,
                texts: &self.texts,
                out: &mut out,
                interactive: true,
                help: view.settings.tooltips.then_some(view.keymap),
            };
            details::details(&mut cx, rect, form);
            let escape = ui.input(|input| input.key_pressed(Key::Escape));
            if escape && !out.is_claimed(Key::Escape, &egui::Modifiers::NONE) {
                out.emit(AppCommand::BrowserTrackDetailsClose);
            }
        }
        if let Some(form) = state.rules.as_mut().filter(|_| rules_open) {
            let veil = self.theme.tokens.color.bg_app.gamma_multiply(DIALOG_VEIL);
            ui.painter().rect_filled(screen, CornerRadius::ZERO, veil);
            ui.interact(screen, id("rules-backdrop"), Sense::click_and_drag());
            let rect = Rect::from_center_size(screen.center(), rules::rules_size(form.rows.len()))
                .round_ui();
            let mut cx = Cx {
                ui,
                theme: &self.theme,
                texts: &self.texts,
                out: &mut out,
                interactive: true,
                help: view.settings.tooltips.then_some(view.keymap),
            };
            rules::rules(&mut cx, rect, form, view.settings.key_notation);
            let escape = ui.input(|input| input.key_pressed(Key::Escape));
            if escape && !out.is_claimed(Key::Escape, &egui::Modifiers::NONE) {
                out.emit(AppCommand::BrowserSmartRulesClose);
            }
        }
        if let Some(page) = dialog {
            // Whatever lies behind the dialog is dimmed and takes no input.
            let veil = self.theme.tokens.color.bg_app.gamma_multiply(DIALOG_VEIL);
            ui.painter().rect_filled(screen, CornerRadius::ZERO, veil);
            ui.interact(screen, id("dialog-backdrop"), Sense::click_and_drag());
            let rect = Rect::from_center_size(screen.center(), DIALOG_SIZE).round_ui();
            let maintenance = state.maintenance;
            let mut cx = Cx {
                ui,
                theme: &self.theme,
                texts: &self.texts,
                out: &mut out,
                interactive: !maintenance,
                help: view.settings.tooltips.then_some(view.keymap),
            };
            settings::settings(&mut cx, rect, page, view, &mut state.settings);
            // The maintenance of the collection opens over the preferences, which then rest.
            if maintenance {
                ui.painter().rect_filled(screen, CornerRadius::ZERO, veil);
                ui.interact(screen, id("maintenance-backdrop"), Sense::click_and_drag());
                let rect = Rect::from_center_size(screen.center(), maintenance::MAINTENANCE_SIZE)
                    .round_ui();
                let mut cx = Cx {
                    ui,
                    theme: &self.theme,
                    texts: &self.texts,
                    out: &mut out,
                    interactive: true,
                    help: view.settings.tooltips.then_some(view.keymap),
                };
                maintenance::maintenance(&mut cx, rect, view, state.remove_asked);
            }
            let escape = ui.input(|input| input.key_pressed(Key::Escape));
            if escape && !out.is_claimed(Key::Escape, &egui::Modifiers::NONE) {
                out.emit(if maintenance {
                    AppCommand::LibraryMaintenanceClose
                } else {
                    AppCommand::SettingsClose
                });
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
        // With a dialog open, the shortcuts of the decks and the mixer rest.
        if dialog.is_some() || details_open || rules_open {
            actions.retain(|action| matches!(action, Action::App(_)));
        }
        // Control and A belongs to a text field while one has the keyboard, and the list
        // behind a dialog keeps its selection.
        if dialog.is_some() || details_open || rules_open || out.takes_text() {
            actions.retain(|action| !matches!(action, Action::App(AppCommand::BrowserSelectAll)));
        }
        out.actions.extend(actions);
        self.controls = out.controls;
        out.actions
    }
}

/// The selected rows that are tracks of the library, in the order of their files, a track that
/// is in two rows once.
fn selected_tracks<'a>(view: &View<'a>) -> Vec<&'a FileRow> {
    let browser = view.browser;
    if !browser
        .collection
        .as_ref()
        .is_some_and(CollectionView::shown)
    {
        return Vec::new();
    }
    let mut rows: Vec<&FileRow> = Vec::new();
    for row in browser
        .marked
        .iter()
        .filter_map(|index| browser.rows.get(*index))
        .filter(|row| matches!(row.kind, RowKind::Track(_)))
    {
        if !rows.iter().any(|seen| seen.path == row.path) {
            rows.push(row);
        }
    }
    // By file, not by place in the list: a save that sorts the list again keeps the order.
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    rows
}

fn track_paths(rows: &[&FileRow]) -> Vec<std::path::PathBuf> {
    rows.iter().map(|row| row.path.clone()).collect()
}

/// What the library has of a track, with what the dialog saved since it opened.
fn values_of(row: &FileRow, edits: &[DetailsEdit]) -> Option<TrackDetails> {
    let RowKind::Track(track) = &row.kind else {
        return None;
    };
    let text = |value: &Option<String>| value.clone().unwrap_or_default();
    let mut values = TrackDetails {
        title: text(&track.title),
        artist: text(&track.artist),
        album: text(&track.album),
        genre: text(&track.genre),
        comment: text(&track.comment),
        rating: track.rating.unwrap_or(0),
    };
    for edit in edits.iter().filter(|edit| edit.tracks.contains(&row.path)) {
        for field in DetailField::ALL
            .into_iter()
            .filter(|field| edit.fields.contains(*field))
        {
            details::copy_field(&mut values, &edit.details, field);
        }
    }
    Some(values)
}

/// The form of the dialog of the details at `position` of the selected tracks: all of them at
/// 0, one of them from 1.
fn details_of(view: &View<'_>, position: usize, edits: &[DetailsEdit]) -> Option<DetailsForm> {
    let rows = selected_tracks(view);
    let count = rows.len();
    let position = if count > 1 { position.min(count) } else { 0 };
    let tracks = track_paths(&rows);
    if position == 0 && count > 1 {
        let all: Vec<TrackDetails> = rows
            .iter()
            .filter_map(|row| values_of(row, edits))
            .collect();
        let first = all.first()?;
        let mut shared = DetailFields::NONE;
        let mut values = TrackDetails::default();
        for field in DetailField::ALL {
            if all
                .iter()
                .all(|other| details::same_field(other, first, field))
            {
                shared.insert(field);
                details::copy_field(&mut values, first, field);
            }
        }
        return Some(DetailsForm {
            file: String::new(),
            folder: String::new(),
            opened: values.clone(),
            details: values,
            count,
            position,
            shared,
            ticked: shared,
            tracks,
        });
    }
    let row = rows.get(position.saturating_sub(1))?;
    let values = values_of(row, edits)?;
    Some(DetailsForm {
        file: row.name.clone(),
        folder: row
            .path
            .parent()
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default(),
        opened: values.clone(),
        details: values,
        count,
        position,
        shared: DetailFields::ALL,
        ticked: DetailFields::NONE,
        tracks,
    })
}

/// The rectangles of the main window.
struct Zones {
    header: Rect,
    notice: Option<Rect>,
    /// The waveform lanes, which only the two-deck layout has.
    lanes: Option<Rect>,
    /// The effect row: unit 1, the panel over the mixer, unit 2.
    effects: [Rect; 3],
    /// The decks the window shows and where.
    decks: Vec<(DeckId, Rect)>,
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
        // The effect row, in both layouts: a unit over each column of decks, a panel over
        // the mixer.
        let column = deck_column(screen, seam);
        let effects_band = |left: f32, right: f32| {
            Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(right, top + effects::ROW_HEIGHT),
            )
        };
        let effects = [
            effects_band(screen.left(), screen.left() + column),
            effects_band(
                screen.left() + column + seam,
                screen.right() - column - seam,
            ),
            effects_band(screen.right() - column, screen.right()),
        ];
        top += effects::ROW_HEIGHT + seam;
        if view.settings.deck_layout == DeckLayout::Four {
            return Self::four(screen, (header, notice, effects), top, seam);
        }
        let lanes = band(top, LANES_HEIGHT);
        top = lanes.bottom() + seam;
        let decks_height = match view.settings.deck_size {
            DeckSize::Full => DECKS_HEIGHT,
            DeckSize::Compact => COMPACT_DECKS_HEIGHT,
        };
        let deck_a = Rect::from_min_size(
            egui::pos2(screen.left(), top),
            egui::vec2(column, decks_height),
        );
        let deck_b = Rect::from_min_size(
            egui::pos2(screen.right() - column, top),
            egui::vec2(column, decks_height),
        );
        let mixer = Rect::from_min_max(
            egui::pos2(deck_a.right() + seam, top),
            egui::pos2(deck_b.left() - seam, top + decks_height),
        );
        top += decks_height + seam;
        let browser = Rect::from_min_max(
            egui::pos2(screen.left(), top),
            screen.max.max(egui::pos2(screen.left(), top)),
        );
        Self {
            header,
            notice,
            lanes: Some(lanes),
            effects,
            decks: vec![(DeckId::A, deck_a), (DeckId::B, deck_b)],
            mixer,
            browser,
        }
    }

    /// The four-deck layout: A over C on the left, the mixer, B over D on the right, and the
    /// browser under them. The mixer keeps its width; the two columns of decks share the rest
    /// of the window, and the browser takes the rest of its height.
    fn four(
        screen: Rect,
        (header, notice, effects): (Rect, Option<Rect>, [Rect; 3]),
        top: f32,
        seam: f32,
    ) -> Self {
        let column = deck_column(screen, seam);
        let deck_height = ((FOUR_DECKS_HEIGHT - seam) / 2.0).floor();
        let left = screen.left();
        let right = screen.right() - column;
        let place = |x: f32, row: f32| {
            Rect::from_min_size(
                egui::pos2(x, top + row * (deck_height + seam)),
                egui::vec2(column, deck_height),
            )
        };
        let mixer = Rect::from_min_max(
            egui::pos2(left + column + seam, top),
            egui::pos2(right - seam, top + FOUR_DECKS_HEIGHT),
        );
        let browser_top = top + FOUR_DECKS_HEIGHT + seam;
        Self {
            header,
            notice,
            lanes: None,
            effects,
            decks: vec![
                (DeckId::A, place(left, 0.0)),
                (DeckId::B, place(right, 0.0)),
                (DeckId::C, place(left, 1.0)),
                (DeckId::D, place(right, 1.0)),
            ],
            mixer,
            browser: Rect::from_min_max(
                egui::pos2(screen.left(), browser_top),
                screen.max.max(egui::pos2(screen.left(), browser_top)),
            ),
        }
    }
}

/// The width of each column of decks: the mixer keeps its width and the two columns share the
/// rest of the window, 520 points each at the designed width. `docs/behavior/interface.md`,
/// *Window size*.
fn deck_column(screen: Rect, seam: f32) -> f32 {
    let mixer_width = DESIGN_WIDTH - 2.0 * (DECK_WIDTH + seam);
    ((screen.width() - mixer_width) / 2.0 - seam)
        .max(DECK_WIDTH)
        .floor()
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
        zones
            .decks
            .iter()
            .find(|(_, rect)| rect.contains(pos))
            .map(|(deck, _)| *deck)
    });
    let deck = under_pointer.unwrap_or_else(|| {
        view.settings
            .deck_layout
            .decks()
            .iter()
            .copied()
            .find(|deck| {
                let snapshot = &view.engine.decks[deck.index()];
                snapshot.track.is_none() && !snapshot.loading
            })
            .unwrap_or(DeckId::A)
    });
    cx.out.emit(Command::DeckLoad { deck, path });
}

#[cfg(test)]
mod tests {
    use egui::{Rect, pos2, vec2};
    use platter_core::DeckId;

    use super::{DECK_WIDTH, DESIGN_WIDTH, Zones};
    use crate::command::DeckLayout;
    use crate::sample;

    const SEAM: f32 = 2.0;
    const MIXER_WIDTH: f32 = DESIGN_WIDTH - 2.0 * (DECK_WIDTH + SEAM);

    /// `docs/behavior/interface.md`, *Window size*: in a window wider than 1440 points the
    /// mixer keeps its width, the decks share the rest and reach the edges of the window.
    #[test]
    fn a_wide_window_widens_the_decks_and_keeps_the_mixer() {
        for layout in DeckLayout::ALL {
            let mut scene = sample::playing();
            scene.settings.deck_layout = layout;
            let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 1080.0));
            let zones = Zones::of(screen, &scene.view(), SEAM);
            let deck = |id: DeckId| {
                zones
                    .decks
                    .iter()
                    .find(|(of, _)| *of == id)
                    .map(|(_, rect)| *rect)
                    .unwrap()
            };
            let (a, b) = (deck(DeckId::A), deck(DeckId::B));
            assert_eq!(a.left(), 0.0, "{layout:?}");
            assert_eq!(b.right(), 1920.0, "{layout:?}");
            assert_eq!(a.width(), b.width(), "{layout:?}");
            assert_eq!(zones.mixer.width(), MIXER_WIDTH, "{layout:?}");
            assert_eq!(zones.mixer.left(), a.right() + SEAM, "{layout:?}");
            assert_eq!(zones.browser.bottom(), 1080.0, "{layout:?}");
            // The effect row follows the columns: a unit over each, the mixer width between.
            let [left, middle, right] = zones.effects;
            assert_eq!((left.left(), left.width()), (0.0, a.width()), "{layout:?}");
            assert_eq!(middle.x_range(), zones.mixer.x_range(), "{layout:?}");
            assert_eq!(
                (right.right(), right.width()),
                (1920.0, b.width()),
                "{layout:?}"
            );
            assert!(left.bottom() < a.top(), "{layout:?}");
            if let Some(lanes) = zones.lanes {
                assert_eq!(lanes.width(), 1920.0);
            }
        }
    }

    /// At the designed size nothing moves: decks of 520 points and the mixer between them.
    #[test]
    fn the_designed_window_keeps_decks_of_520_points() {
        for layout in DeckLayout::ALL {
            let mut scene = sample::playing();
            scene.settings.deck_layout = layout;
            let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(DESIGN_WIDTH, 900.0));
            let zones = Zones::of(screen, &scene.view(), SEAM);
            for (_, rect) in &zones.decks {
                assert_eq!(rect.width(), DECK_WIDTH, "{layout:?}");
            }
            assert_eq!(zones.mixer.width(), MIXER_WIDTH, "{layout:?}");
        }
    }
}
