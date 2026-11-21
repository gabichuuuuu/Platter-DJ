//! The file selector along the bottom of the window: a tree of folders on the left and the
//! files of the chosen folder on the right.
//!
//! Component: `TrackList` in `docs/design/DESIGN.md`; placement from
//! `docs/design/screens/M2-main.html`. Behaviour note: `docs/behavior/browser.md`.

use std::path::PathBuf;

use egui::{Align2, CornerRadius, EventFilter, Key, Pos2, Rect, Role, Sense, WidgetInfo};
use platter_core::DeckId;
use platter_engine::Command;
use platter_library::{NodeId, SessionId};

use crate::collection::TagWriting;
use crate::command::{
    Action, AppCommand, BrowserNode, FAVOURITE_PLACES, ListColumn, NewItem, SortColumn,
};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::icons::Glyph;
use crate::keys::{self, KeyNotation};
use crate::model::{
    BrowserView, CollectionView, CoverImage, FileRow, ImportOutcome, PlaylistFileOutcome,
    PlaylistRow, RowKind, SessionRow, TreeRow, View,
};
use crate::paint;
use crate::panels::waveform::deck_color;
use crate::widgets::field::{self, Finish};
use crate::widgets::key::{self, Face, KeySpec, Press};
use crate::widgets::menu::{self, Item};

pub const TREE_WIDTH: f32 = 260.0;
const TREE_ROW: f32 = 26.0;
const TREE_INDENT: f32 = 16.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
/// The status bar under the list.
const STATUS_HEIGHT: f32 = 24.0;
/// The preview player in the toolbar, as the design system sizes it.
const PREVIEW_STRIP: egui::Vec2 = egui::vec2(200.0, 20.0);
const PREVIEW_KEY: f32 = 28.0;
const PREVIEW_TIME_WIDTH: f32 = 40.0;
const SEARCH_WIDTH: f32 = 240.0;
const SEARCH_HEIGHT: f32 = 28.0;
pub const ROW_HEIGHT: f32 = 27.0;
const COLUMN_GAP: f32 = 12.0;

/// The columns of the file list after the title, which takes the width that is left.
const ARTIST: f32 = 200.0;
const ALBUM: f32 = 180.0;
const BPM: f32 = 64.0;
const KEY: f32 = 64.0;
const TIME: f32 = 56.0;
/// Five stars of 12 pixels with 2 between them.
const RATING: f32 = 68.0;
const FILE: f32 = 110.0;
const BADGE: f32 = 22.0;
/// The cell of the cover of a track, after its badge.
const COVER: f32 = 18.0;

/// The square of the colour tag, with room for the chevron of its header.
const COLOUR: f32 = 26.0;
const GENRE: f32 = 120.0;
const COMMENT: f32 = 180.0;

fn width_of(column: ListColumn) -> f32 {
    match column {
        ListColumn::Colour => COLOUR,
        ListColumn::Artist => ARTIST,
        ListColumn::Album => ALBUM,
        ListColumn::Genre => GENRE,
        ListColumn::Bpm => BPM,
        ListColumn::Key => KEY,
        ListColumn::Time => TIME,
        ListColumn::Rating => RATING,
        ListColumn::Comment => COMMENT,
        ListColumn::File => FILE,
    }
}

/// The name of a column in its header and in the menu of the header.
fn label_of(column: ListColumn) -> &'static str {
    match column {
        ListColumn::Colour => "browser.colour",
        ListColumn::Artist => "browser.artist",
        ListColumn::Album => "browser.album",
        ListColumn::Genre => "browser.genre",
        ListColumn::Bpm => "browser.bpm",
        ListColumn::Key => "browser.key",
        ListColumn::Time => "browser.time",
        ListColumn::Rating => "browser.rating",
        ListColumn::Comment => "browser.comment",
        ListColumn::File => "browser.file",
    }
}

/// Where each column of the file list starts and ends; a hidden column has no place.
struct Columns {
    badge: Rect,
    cover: Rect,
    colour: Option<Rect>,
    title: Rect,
    artist: Option<Rect>,
    album: Option<Rect>,
    genre: Option<Rect>,
    bpm: Option<Rect>,
    key: Option<Rect>,
    time: Option<Rect>,
    rating: Option<Rect>,
    comment: Option<Rect>,
    file: Option<Rect>,
}

impl Columns {
    /// The title takes the width the columns `shown` leave.
    fn of(row: Rect, shown: &[ListColumn]) -> Self {
        let left = row.left() + 10.0;
        let right = row.right() - 10.0;
        let others: f32 = shown
            .iter()
            .map(|column| width_of(*column) + COLUMN_GAP)
            .sum();
        let title = (right - left - BADGE - COVER - 2.0 * COLUMN_GAP - others).max(80.0);
        let cell = |x: &mut f32, width: f32| {
            let rect = Rect::from_min_max(
                egui::pos2(*x, row.top()),
                egui::pos2(*x + width, row.bottom()),
            );
            *x += width + COLUMN_GAP;
            rect
        };
        let optional = |x: &mut f32, column: ListColumn| {
            shown.contains(&column).then(|| cell(x, width_of(column)))
        };
        let mut x = left;
        Self {
            badge: cell(&mut x, BADGE),
            cover: cell(&mut x, COVER),
            colour: optional(&mut x, ListColumn::Colour),
            title: cell(&mut x, title),
            artist: optional(&mut x, ListColumn::Artist),
            album: optional(&mut x, ListColumn::Album),
            genre: optional(&mut x, ListColumn::Genre),
            bpm: optional(&mut x, ListColumn::Bpm),
            key: optional(&mut x, ListColumn::Key),
            time: optional(&mut x, ListColumn::Time),
            rating: optional(&mut x, ListColumn::Rating),
            comment: optional(&mut x, ListColumn::Comment),
            file: optional(&mut x, ListColumn::File),
        }
    }

    fn get(&self, column: ListColumn) -> Option<Rect> {
        match column {
            ListColumn::Colour => self.colour,
            ListColumn::Artist => self.artist,
            ListColumn::Album => self.album,
            ListColumn::Genre => self.genre,
            ListColumn::Bpm => self.bpm,
            ListColumn::Key => self.key,
            ListColumn::Time => self.time,
            ListColumn::Rating => self.rating,
            ListColumn::Comment => self.comment,
            ListColumn::File => self.file,
        }
    }

    /// Where the state of a file that cannot be played goes, and what is left of the title:
    /// the format column, or the end of the title while that column is hidden.
    fn flag(&self) -> (Rect, Rect) {
        match self.file {
            Some(file) => (self.title, file),
            None => {
                let split = (self.title.right() - FILE).max(self.title.left());
                (
                    Rect::from_min_max(
                        self.title.min,
                        egui::pos2(split - COLUMN_GAP, self.title.bottom()),
                    ),
                    Rect::from_min_max(egui::pos2(split, self.title.top()), self.title.max),
                )
            }
        }
    }
}

/// What the file list keeps between frames: how far each side is scrolled.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BrowserMemory {
    pub list_scroll: f32,
    pub tree_scroll: f32,
    /// The selection the list was last scrolled to, so it follows the selection when it
    /// moves and stays put when the user scrolls away.
    seen_selection: Option<usize>,
    /// The search field takes the keyboard in the next frame.
    pub(crate) focus_search: bool,
    /// The context menu that is open, if one is.
    pub menu: Option<BrowserMenu>,
    /// The field that names a playlist or a folder, while one is open in the tree.
    pub naming: Option<Naming>,
}

/// What the name field of the tree names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Naming {
    /// A new playlist or folder in a folder of the tree, or at its top.
    New {
        folder: Option<NodeId>,
        item: NewItem,
    },
    /// Another name for a playlist or a folder.
    Rename(NodeId),
}

/// An open context menu: what it acts on and where it was asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrowserMenu {
    pub target: MenuTarget,
    pub at: Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTarget {
    /// The heading *Playlists*.
    Playlists,
    /// A folder of playlists.
    Folder(NodeId),
    /// A playlist of the tree.
    Playlist(NodeId),
    /// The selected row of the playlist the list shows.
    Entry,
    /// The folders a playlist or a folder can go to.
    MoveTo(NodeId),
    /// A favourite place that holds a playlist.
    Favourite(usize),
    /// The question before the selected track is deleted from the collection.
    ConfirmDelete,
    /// The colours the selected track can be tagged with.
    Colour,
    /// The ratings the selected track can be given.
    Rating,
    /// The pictures of the folder of the explorer, one of which becomes the cover of the
    /// selected tracks.
    Pictures,
    /// The columns of the list, from its header.
    Columns,
    /// The history of a session.
    Session(SessionId),
    /// The question before the history of a session is deleted.
    ConfirmSessionDelete(SessionId),
}

/// Whether Shift+F10, the key of the context menu, was pressed in this frame.
fn menu_key(cx: &Cx<'_>) -> bool {
    cx.ui.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: Key::F10,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.shift
            )
        })
    })
}

/// Keeps a scroll offset inside what there is to scroll.
fn clamp_scroll(offset: f32, content: f32, visible: f32) -> f32 {
    offset.clamp(0.0, (content - visible).max(0.0))
}

/// A thin thumb at the right edge of a scrolled area.
fn scroll_thumb(cx: &Cx<'_>, area: Rect, offset: f32, content: f32) {
    if content <= area.height() {
        return;
    }
    let tokens = &cx.theme.tokens;
    let share = area.height() / content;
    let thumb = Rect::from_min_size(
        egui::pos2(area.right() - 4.0, area.top() + offset * share),
        egui::vec2(3.0, (area.height() * share).max(12.0)),
    );
    cx.painter().rect_filled(
        thumb,
        paint::radius(tokens.radius.sm),
        tokens.color.line_key,
    );
}

pub fn browser(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>, memory: &mut BrowserMemory) {
    let seam = cx.theme.tokens.space.s1;
    let tree_rect = Rect::from_min_size(rect.min, egui::vec2(TREE_WIDTH, rect.height()));
    let list_rect = Rect::from_min_max(egui::pos2(tree_rect.right() + seam, rect.top()), rect.max);
    tree(cx, tree_rect, view.browser, memory);
    toolbar(
        cx,
        Rect::from_min_size(list_rect.min, egui::vec2(list_rect.width(), TOOLBAR_HEIGHT)),
        view,
        std::mem::take(&mut memory.focus_search),
    );
    let rows = Rect::from_min_max(
        egui::pos2(list_rect.left(), list_rect.top() + TOOLBAR_HEIGHT),
        egui::pos2(list_rect.right(), list_rect.bottom() - STATUS_HEIGHT),
    );
    list(cx, rows, view, memory);
    status_bar(
        cx,
        Rect::from_min_max(egui::pos2(list_rect.left(), rows.bottom()), list_rect.max),
        view.browser,
    );
    // The menu last, so it is drawn above the rest and covers the controls it hides.
    if let Some(open) = memory.menu
        && !(cx.interactive
            && context_menu(
                cx,
                view.browser,
                open,
                memory,
                view.settings.tag_writing == TagWriting::Everything,
            ))
    {
        memory.menu = None;
    }
}

/// The preview player, ending at `right` on the line `middle` of the toolbar: play or pause
/// with the headphones icon, the strip of its track and the time played. Returns where it
/// starts.
fn preview_player(cx: &mut Cx<'_>, right: f32, middle: f32, view: &View<'_>) -> f32 {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let preview = &view.engine.preview;
    let headphones = view.audio.has_headphones();
    let track = preview
        .track
        .as_ref()
        .filter(|track| track.seconds > 0.0 && !preview.loading);

    let time = Rect::from_min_size(
        egui::pos2(right - PREVIEW_TIME_WIDTH, middle - 9.0),
        egui::vec2(PREVIEW_TIME_WIDTH, 18.0),
    );
    if track.is_some() {
        paint::text(
            cx.painter(),
            theme,
            time,
            Align2::RIGHT_CENTER,
            &format::duration(preview.position_seconds),
            tokens.text.readout,
            color.text_secondary,
        );
    }

    let strip = Rect::from_min_size(
        egui::pos2(
            time.left() - tokens.space.s2 - PREVIEW_STRIP.x,
            middle - PREVIEW_STRIP.y / 2.0,
        ),
        PREVIEW_STRIP,
    );
    cx.painter()
        .rect_filled(strip, paint::radius(tokens.radius.sm), color.bg_app);
    let enabled = headphones && track.is_some();
    let seek = |fraction: f64| Command::PreviewSeek {
        position: fraction.clamp(0.0, 1.0),
    };
    cx.record(seek(0.5).id(), strip, ControlKind::Area, enabled);
    let response = cx.ui.interact(
        strip,
        id("preview-strip"),
        cx.sense(enabled, Sense::click_and_drag()),
    );
    let name = texts.get("browser.preview_strip");
    response.widget_info(|| WidgetInfo::labeled(Role::Slider, enabled, name));
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        cx.out
            .emit(seek(f64::from((pointer.x - strip.left()) / strip.width())));
    }
    // A row dragged from the list and released here is previewed.
    if cx.interactive
        && let Some(path) = response.dnd_release_payload::<PathBuf>()
    {
        cx.out.emit(Command::PreviewLoad {
            path: path.as_ref().clone(),
        });
    }
    if let (Some(track), Some(peaks)) = (track, preview.peaks.as_deref()) {
        let columns = peaks.overview();
        let width = strip.width().floor().max(1.0) as usize;
        let painter = cx.painter();
        for pixel in 0..width {
            let from = pixel * columns.len() / width;
            let to = ((pixel + 1) * columns.len() / width).max(from + 1);
            let level = columns
                .get(from..to.min(columns.len()))
                .unwrap_or(&[])
                .iter()
                .flat_map(|column| column.iter())
                .copied()
                .max()
                .unwrap_or(0);
            let half = f32::from(level) / 255.0 * (strip.height() / 2.0 - 1.0);
            if half >= 0.5 {
                let x = strip.left() + pixel as f32 + 0.5;
                painter.vline(
                    x,
                    strip.center().y - half..=strip.center().y + half,
                    egui::Stroke::new(1.0, color.text_secondary),
                );
            }
        }
        let played = (preview.position_seconds / track.seconds).clamp(0.0, 1.0) as f32;
        let x = strip.left() + played * strip.width();
        painter.vline(
            x.clamp(strip.left() + 1.0, strip.right() - 1.0),
            strip.y_range(),
            egui::Stroke::new(1.0, color.playhead),
        );
    }

    let key_rect = Rect::from_min_size(
        egui::pos2(
            strip.left() - tokens.space.s2 - PREVIEW_KEY,
            middle - PREVIEW_KEY / 2.0,
        ),
        egui::vec2(PREVIEW_KEY, PREVIEW_KEY),
    );
    // The selected track goes in first; once it is in, the key plays and pauses it.
    let selected = view
        .browser
        .selected
        .and_then(|index| view.browser.rows.get(index))
        .filter(|row| matches!(row.kind, RowKind::Track(_) | RowKind::Pending));
    let holds_selected = selected
        .zip(preview.track.as_ref())
        .is_some_and(|(row, track)| row.path == track.path);
    let (action, glyph, name): (Action, Glyph, &str) = if holds_selected || selected.is_none() {
        let playing = preview.playing;
        (
            Command::PreviewPlayPause.into(),
            if playing { Glyph::Pause } else { Glyph::Play },
            texts.get(if playing {
                "browser.preview_pause"
            } else {
                "browser.preview_play"
            }),
        )
    } else {
        (
            AppCommand::PreviewLoadSelected.into(),
            Glyph::Play,
            texts.get("browser.preview_load"),
        )
    };
    let spec = KeySpec {
        face: Face::Icon(glyph, 14.0),
        name,
        lit: preview.playing.then_some(color.text_primary),
        held: None,
        enabled: headphones && (holds_selected || selected.is_some() || track.is_some()),
        keeps_led: false,
        text: None,
    };
    key::key(cx, key_rect, id("preview-play"), spec, Press::Click(action));
    let icon = Rect::from_center_size(
        egui::pos2(key_rect.left() - tokens.space.s2 - 8.0, middle),
        egui::vec2(16.0, 16.0),
    );
    theme.icons.paint(
        cx.painter(),
        Glyph::Headphones,
        icon,
        if headphones {
            color.text_secondary
        } else {
            color.text_disabled
        },
    );
    icon.left()
}

/// The line under the list: the analysis of the collection or why it stopped, and the
/// selected track.
fn status_bar(cx: &mut Cx<'_>, rect: Rect, browser: &BrowserView) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    painter.hline(
        rect.x_range(),
        rect.top(),
        paint::hairline(theme, color.line_divider),
    );
    let inner = rect.shrink2(egui::vec2(tokens.space.s3, 0.0));
    let half = inner.width() / 2.0;
    let left = Rect::from_min_size(
        inner.min,
        egui::vec2(half - tokens.space.s3, inner.height()),
    );
    let right = Rect::from_min_max(egui::pos2(inner.left() + half, inner.top()), inner.max);
    let state = match (&browser.analysis, &browser.problem) {
        (Some(status), _) => Some(texts.format(
            "browser.analysing",
            &[
                ("done", &status.done.to_string()),
                ("total", &status.total.to_string()),
            ],
        )),
        (None, Some(problem)) => {
            Some(texts.format("browser.analysis_stopped", &[("why", problem)]))
        }
        (None, None) => None,
    };
    if let Some(state) = state {
        paint::text_elided(
            painter,
            theme,
            left,
            &state,
            tokens.text.label,
            color.text_secondary,
        );
    }
    let selected = browser
        .selected
        .and_then(|index| browser.rows.get(index))
        .and_then(|row| match &row.kind {
            RowKind::Track(track) => {
                let mut parts = vec![track.title.clone().unwrap_or_else(|| row.name.clone())];
                parts.extend(track.seconds.map(format::duration));
                if !track.format.is_empty() {
                    parts.push(track.format.clone());
                }
                Some(texts.format("browser.status_selected", &[("track", &parts.join(", "))]))
            }
            _ => None,
        });
    if let Some(selected) = selected {
        let width =
            paint::text_width(painter, theme, &selected, tokens.text.label).min(right.width());
        paint::text_elided(
            painter,
            theme,
            Rect::from_min_max(egui::pos2(right.right() - width, right.top()), right.max),
            &selected,
            tokens.text.label,
            color.text_secondary,
        );
    }
}

/// The context menu of the playlists, a folder or a playlist of the tree, or of a row of a
/// playlist. Returns whether it stays open; it closes when what it acts on is gone.
/// `covers` says whether the entries of the covers are offered: a cover lives only in the
/// file, so only while Platter writes every tag.
fn context_menu(
    cx: &mut Cx<'_>,
    browser: &BrowserView,
    open: BrowserMenu,
    memory: &mut BrowserMemory,
    covers: bool,
) -> bool {
    let texts = cx.texts;
    if open.target == MenuTarget::Columns {
        return columns_menu(cx, browser, open);
    }
    let Some(collection) = browser.collection.as_ref() else {
        return false;
    };
    let item_of = |id: NodeId| collection.playlists.iter().find(|row| row.id == id);
    // Files are written to and read from the folder the explorer shows.
    let explorer = browser.explorer.as_ref();
    let explorer_name = explorer.map(|path| {
        path.file_name()
            .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
            .into_owned()
    });
    let export_label = explorer_name
        .as_deref()
        .map(|name| texts.format("browser.menu_export", &[("folder", name)]));
    let import_label = explorer_name
        .as_deref()
        .map(|name| texts.format("browser.menu_import", &[("folder", name)]));
    let import = |folder: Option<NodeId>| {
        explorer.zip(import_label.as_deref()).map(|(path, label)| {
            (
                Item {
                    label,
                    action: Some(
                        AppCommand::BrowserPlaylistImport {
                            folder,
                            path: path.clone(),
                        }
                        .into(),
                    ),
                },
                Then::Nothing,
            )
        })
    };
    // The question names the track it deletes.
    let question = match open.target {
        MenuTarget::ConfirmDelete => browser
            .selected
            .and_then(|index| browser.rows.get(index))
            .map(|row| {
                let name = match &row.kind {
                    RowKind::Track(track) => track.title.as_deref().unwrap_or(&row.name),
                    _ => &row.name,
                };
                texts.format("browser.confirm_delete", &[("track", name)])
            }),
        MenuTarget::ConfirmSessionDelete(session) => collection
            .sessions
            .iter()
            .find(|row| row.id == session)
            .map(|row| texts.format("browser.confirm_delete_history", &[("session", &row.label)])),
        _ => None,
    };
    // The colours by number and the ratings by their stars, for the menus that set them.
    let colour_labels: Vec<String> = (1..=8)
        .map(|number: u8| texts.format("browser.colour_number", &[("number", &number.to_string())]))
        .collect();
    let picture_names: Vec<String> = browser
        .pictures
        .iter()
        .map(|path| {
            path.file_name()
                .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
                .into_owned()
        })
        .collect();
    let opens = |label: &'static str, naming: Naming| {
        (
            Item {
                label: texts.get(label),
                action: None,
            },
            Then::Name(naming),
        )
    };
    let sends = |label: &'static str, action: AppCommand| {
        (
            Item {
                label: texts.get(label),
                action: Some(action.into()),
            },
            Then::Nothing,
        )
    };
    // The entries of the covers of the selected tracks; importing needs a picture to offer.
    let cover_entries = || {
        let mut entries = Vec::new();
        if covers {
            if !browser.pictures.is_empty() {
                entries.push((
                    Item {
                        label: texts.get("browser.menu_cover_import"),
                        action: None,
                    },
                    Then::Menu(MenuTarget::Pictures),
                ));
            }
            entries.push(sends(
                "browser.menu_cover_delete",
                AppCommand::BrowserTrackCoverDelete,
            ));
        }
        entries
    };
    let move_to = |item: NodeId| {
        (
            Item {
                label: texts.get("browser.menu_move"),
                action: None,
            },
            Then::Menu(MenuTarget::MoveTo(item)),
        )
    };
    let new = |folder: Option<NodeId>| {
        [
            opens(
                "browser.menu_new_playlist",
                Naming::New {
                    folder,
                    item: NewItem::Playlist,
                },
            ),
            opens(
                "browser.menu_new_folder",
                Naming::New {
                    folder,
                    item: NewItem::Folder,
                },
            ),
            opens(
                "browser.menu_new_smart",
                Naming::New {
                    folder,
                    item: NewItem::Smart,
                },
            ),
        ]
    };
    let (entries, back): (Vec<(Item<'_>, Then)>, egui::Id) = match open.target {
        MenuTarget::Playlists => {
            let mut entries: Vec<_> = new(None).into();
            entries.extend(import(None));
            (entries, id("tree-playlists"))
        }
        MenuTarget::Folder(folder) if item_of(folder).is_some_and(|row| row.folder) => {
            let mut entries: Vec<_> = new(Some(folder)).into();
            entries.push(opens("browser.menu_rename", Naming::Rename(folder)));
            entries.push(move_to(folder));
            entries.extend(import(Some(folder)));
            (entries, id(("tree-playlist", folder)))
        }
        MenuTarget::Playlist(smart) if item_of(smart).is_some_and(|row| row.smart) => {
            let mut entries = vec![
                sends(
                    "browser.menu_edit_rules",
                    AppCommand::BrowserSmartRulesOpen { smart },
                ),
                opens("browser.menu_rename", Naming::Rename(smart)),
                move_to(smart),
                sends(
                    "browser.menu_save_playlist",
                    AppCommand::BrowserSmartToPlaylist { smart },
                ),
                sends(
                    "browser.menu_duplicate",
                    AppCommand::BrowserSmartDuplicate { smart },
                ),
                sends(
                    "browser.menu_delete",
                    AppCommand::BrowserPlaylistDelete { playlist: smart },
                ),
            ];
            if let (Some(path), Some(label)) = (explorer, export_label.as_deref()) {
                entries.push((
                    Item {
                        label,
                        action: Some(
                            AppCommand::BrowserPlaylistExport {
                                playlist: smart,
                                path: path.clone(),
                            }
                            .into(),
                        ),
                    },
                    Then::Nothing,
                ));
            }
            (entries, id(("tree-playlist", smart)))
        }
        MenuTarget::Playlist(playlist) if item_of(playlist).is_some_and(|row| !row.folder) => {
            let mut entries = vec![
                opens("browser.menu_rename", Naming::Rename(playlist)),
                move_to(playlist),
                sends(
                    "browser.menu_clear",
                    AppCommand::BrowserPlaylistClear { playlist },
                ),
                sends(
                    "browser.menu_remove_repeats",
                    AppCommand::BrowserPlaylistRemoveRepeats { playlist },
                ),
                sends(
                    "browser.menu_delete",
                    AppCommand::BrowserPlaylistDelete { playlist },
                ),
            ];
            if let (Some(path), Some(label)) = (explorer, export_label.as_deref()) {
                entries.push((
                    Item {
                        label,
                        action: Some(
                            AppCommand::BrowserPlaylistExport {
                                playlist,
                                path: path.clone(),
                            }
                            .into(),
                        ),
                    },
                    Then::Nothing,
                ));
            }
            if let Some(place) = free_place(collection)
                && item_of(playlist).is_some_and(|row| row.favourite.is_none())
            {
                entries.push(sends(
                    "browser.menu_add_favourite",
                    AppCommand::BrowserFavouriteSet { place, playlist },
                ));
            }
            if item_of(playlist).is_some_and(|row| !row.preparation) {
                entries.push(sends(
                    "browser.menu_use_preparation",
                    AppCommand::BrowserPreparationSelect { playlist },
                ));
            }
            (entries, id(("tree-playlist", playlist)))
        }
        // Several rows selected: only the entries that act on all of them.
        MenuTarget::Entry if collection.shown() && browser.marked.len() > 1 => {
            let mut entries = vec![sends(
                "browser.menu_details",
                AppCommand::BrowserTrackDetailsOpen,
            )];
            entries.extend(cover_entries());
            (entries, id("browser-list"))
        }
        MenuTarget::Entry if collection.shown() && browser.selected.is_some() => {
            let mut entries = Vec::new();
            // A smart playlist has no entries to take out.
            let entries_shown = collection.playlist.is_some_and(|shown| {
                collection
                    .playlists
                    .iter()
                    .any(|row| row.id == shown && !row.smart)
            });
            if entries_shown {
                entries.push(sends(
                    "browser.menu_remove",
                    AppCommand::BrowserPlaylistRemove,
                ));
            }
            if collection.playlists.iter().any(|row| row.preparation) {
                entries.push(sends(
                    "browser.menu_append_preparation",
                    AppCommand::BrowserPreparationAppend,
                ));
                entries.push(sends(
                    "browser.menu_next_preparation",
                    AppCommand::BrowserPreparationNext,
                ));
            }
            entries.push(sends(
                "browser.menu_reset_played",
                AppCommand::BrowserPlayedReset,
            ));
            entries.push(sends(
                "browser.menu_details",
                AppCommand::BrowserTrackDetailsOpen,
            ));
            entries.extend(cover_entries());
            for (label, target) in [
                ("browser.menu_colour", MenuTarget::Colour),
                ("browser.menu_rating", MenuTarget::Rating),
                ("browser.menu_delete_track", MenuTarget::ConfirmDelete),
            ] {
                entries.push((
                    Item {
                        label: texts.get(label),
                        action: None,
                    },
                    Then::Menu(target),
                ));
            }
            (entries, id("browser-list"))
        }
        MenuTarget::Pictures
            if collection.shown()
                && browser.selected.is_some()
                && covers
                && !browser.pictures.is_empty() =>
        {
            (
                browser
                    .pictures
                    .iter()
                    .zip(&picture_names)
                    .map(|(path, label)| {
                        (
                            Item {
                                label,
                                action: Some(
                                    AppCommand::BrowserTrackCoverImport { path: path.clone() }
                                        .into(),
                                ),
                            },
                            Then::Nothing,
                        )
                    })
                    .collect(),
                id("browser-list"),
            )
        }
        MenuTarget::Colour if collection.shown() && browser.selected.is_some() => {
            let none = sends(
                "browser.colour_none",
                AppCommand::BrowserTrackColourSet { colour: 0 },
            );
            let colours = colour_labels.iter().zip(1..=8).map(|(label, colour)| {
                (
                    Item {
                        label,
                        action: Some(AppCommand::BrowserTrackColourSet { colour }.into()),
                    },
                    Then::Nothing,
                )
            });
            (
                std::iter::once(none).chain(colours).collect(),
                id("browser-list"),
            )
        }
        MenuTarget::Rating if collection.shown() && browser.selected.is_some() => (
            (0..=5)
                .map(|rating| {
                    sends(
                        RATING_LABELS[usize::from(rating)],
                        AppCommand::BrowserTrackRatingSet { rating },
                    )
                })
                .collect(),
            id("browser-list"),
        ),
        MenuTarget::Session(session) => {
            let this_run = collection
                .session
                .as_ref()
                .is_some_and(|row| row.id == session);
            let mut entries = vec![sends(
                "browser.menu_save_playlist",
                AppCommand::BrowserHistoryToPlaylist { session },
            )];
            if let (Some(path), Some(label)) = (explorer, export_label.as_deref()) {
                entries.push((
                    Item {
                        label,
                        action: Some(
                            AppCommand::BrowserHistoryExport {
                                session,
                                path: path.clone(),
                            }
                            .into(),
                        ),
                    },
                    Then::Nothing,
                ));
            }
            if !this_run {
                entries.push((
                    Item {
                        label: texts.get("browser.menu_delete_history"),
                        action: None,
                    },
                    Then::Menu(MenuTarget::ConfirmSessionDelete(session)),
                ));
            }
            (entries, id(("tree-session", session)))
        }
        MenuTarget::ConfirmSessionDelete(session) => match question.as_deref() {
            Some(question) => (
                vec![
                    (
                        Item {
                            label: question,
                            action: Some(AppCommand::BrowserHistoryDelete { session }.into()),
                        },
                        Then::Nothing,
                    ),
                    (
                        Item {
                            label: texts.get("browser.keep_track"),
                            action: None,
                        },
                        Then::Nothing,
                    ),
                ],
                id(("tree-session", session)),
            ),
            None => return false,
        },
        MenuTarget::ConfirmDelete => match question.as_deref() {
            Some(question) if collection.shown() => (
                vec![
                    (
                        Item {
                            label: question,
                            action: Some(AppCommand::BrowserTrackDelete.into()),
                        },
                        Then::Nothing,
                    ),
                    (
                        Item {
                            label: texts.get("browser.keep_track"),
                            action: None,
                        },
                        Then::Nothing,
                    ),
                ],
                id("browser-list"),
            ),
            _ => return false,
        },
        MenuTarget::Favourite(place)
            if collection
                .playlists
                .iter()
                .any(|row| row.favourite == Some(place)) =>
        {
            (
                vec![sends(
                    "browser.menu_remove_favourite",
                    AppCommand::BrowserFavouriteClear { place },
                )],
                id(("favourite", place)),
            )
        }
        MenuTarget::MoveTo(item) if item_of(item).is_some() => {
            let top = sends(
                "browser.move_top",
                AppCommand::BrowserPlaylistMove { item, folder: None },
            );
            let folders = destinations(collection, item).into_iter().map(|row| {
                (
                    Item {
                        label: &row.name,
                        action: Some(
                            AppCommand::BrowserPlaylistMove {
                                item,
                                folder: Some(row.id),
                            }
                            .into(),
                        ),
                    },
                    Then::Nothing,
                )
            });
            (
                std::iter::once(top).chain(folders).collect(),
                id(("tree-playlist", item)),
            )
        }
        _ => return false,
    };
    let (items, thens): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
    let outcome = menu::menu(cx, id("browser-menu"), open.at, &items, back);
    match outcome.picked.and_then(|index| thens.get(index).copied()) {
        Some(Then::Name(naming)) => memory.naming = Some(naming),
        // The next menu opens where this one was asked for.
        Some(Then::Menu(target)) => {
            memory.menu = Some(BrowserMenu {
                target,
                at: open.at,
            });
            return true;
        }
        Some(Then::Nothing) | None => {}
    }
    outcome.open
}

/// The menu of the header of the list: one entry per column that can be shown or hidden.
fn columns_menu(cx: &mut Cx<'_>, browser: &BrowserView, open: BrowserMenu) -> bool {
    let texts = cx.texts;
    let labels: Vec<String> = ListColumn::ALL
        .iter()
        .map(|column| {
            let toggle = if browser.columns.contains(column) {
                "browser.column_hide"
            } else {
                "browser.column_show"
            };
            texts.format(toggle, &[("name", texts.get(label_of(*column)))])
        })
        .collect();
    let items: Vec<Item<'_>> = labels
        .iter()
        .zip(ListColumn::ALL)
        .map(|(label, column)| Item {
            label,
            action: Some(AppCommand::BrowserColumnToggle { column }.into()),
        })
        .collect();
    menu::menu(cx, id("browser-menu"), open.at, &items, id("browser-list")).open
}

/// The entries of the menu of ratings, from no star to five.
const RATING_LABELS: [&str; 6] = [
    "browser.rating_0",
    "browser.rating_1",
    "browser.rating_2",
    "browser.rating_3",
    "browser.rating_4",
    "browser.rating_5",
];

/// The first favourite place that holds no playlist.
fn free_place(collection: &CollectionView) -> Option<usize> {
    FAVOURITE_PLACES.into_iter().find(|place| {
        !collection
            .playlists
            .iter()
            .any(|row| row.favourite == Some(*place))
    })
}

/// What an item of a menu does besides its command.
#[derive(Clone, Copy)]
enum Then {
    Nothing,
    /// Opens the name field.
    Name(Naming),
    /// Opens another menu in its place.
    Menu(MenuTarget),
}

/// The folders `item` can go into: every folder of the tree but itself and the folders inside
/// it, since a folder cannot go into itself.
fn destinations(collection: &CollectionView, item: NodeId) -> Vec<&PlaylistRow> {
    let mut inside: Option<usize> = None;
    collection
        .playlists
        .iter()
        .filter(|row| {
            // The tree lists a folder right before what it holds, one level deeper.
            if let Some(depth) = inside {
                if row.depth > depth {
                    return false;
                }
                inside = None;
            }
            if row.id == item {
                inside = Some(row.depth);
                return false;
            }
            row.folder
        })
        .collect()
}

fn tree(cx: &mut Cx<'_>, rect: Rect, browser: &BrowserView, memory: &mut BrowserMemory) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    cx.painter()
        .rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    let mut area = rect.shrink(tokens.space.s3);
    // The favourites, when there is a library, above the tree.
    if let Some(collection) = &browser.collection {
        let height = FAVOURITE_LINES as f32 * (CHIP_HEIGHT + CHIP_GAP) - CHIP_GAP;
        let row = Rect::from_min_size(area.min, egui::vec2(area.width(), height));
        favourites(cx, row, collection, memory);
        area.min.y = row.bottom() + tokens.space.s3;
    }
    // The collection, the heading of the playlists and the playlists, when there is a
    // library, come before the folders.
    let rows = browser
        .collection
        .as_ref()
        .map_or_else(Vec::new, |collection| {
            library_rows(cx, collection, browser, memory)
        });
    let fixed = rows.len();
    // The track the list has selected, for a playlist that takes it with Insert.
    let selected = browser
        .selected
        .and_then(|index| browser.rows.get(index))
        .filter(|row| matches!(row.kind, RowKind::Track(_) | RowKind::Pending))
        .map(|row| row.path.as_path());
    // *Recordings* comes last, after the folders of the explorer.
    let recordings = browser.recordings.as_ref().map(|folder| Node {
        id: id("tree-recordings"),
        glyph: Glyph::Folder,
        name: cx.texts.get("browser.recordings"),
        depth: 0,
        count: None,
        current: browser.folder.as_ref() == Some(folder),
        action: Some(AppCommand::BrowserNavigate {
            path: folder.clone(),
        }),
        menu: None,
        takes: None,
        open: None,
    });
    let places = fixed + browser.tree.len() + usize::from(recordings.is_some());
    let content = places as f32 * (TREE_ROW + 1.0);
    let hover = cx.ui.interact(rect, id("browser-tree"), Sense::hover());
    if hover.hovered() && cx.interactive {
        memory.tree_scroll -= cx.ui.input(|input| input.smooth_scroll_delta.y);
    }
    memory.tree_scroll = clamp_scroll(memory.tree_scroll, content, area.height());
    let scroll = memory.tree_scroll;
    let painter = cx.painter().with_clip_rect(area);
    let slot = |place: usize| {
        let top = area.top() + place as f32 * (TREE_ROW + 1.0) - scroll;
        let row_rect = Rect::from_min_size(
            egui::pos2(area.left(), top),
            egui::vec2(area.width(), TREE_ROW),
        );
        // A row cut by the edge of the area is drawn and cannot be operated.
        (row_rect, area.contains_rect(row_rect))
    };
    for (place, row) in rows.into_iter().enumerate() {
        let (row_rect, whole) = slot(place);
        match row {
            LibraryRow::Node(node) => {
                if area.intersects(row_rect) {
                    node_row(cx, &painter, row_rect, *node, whole, memory, selected);
                }
            }
            // The field stays drawn while it is open, even scrolled out of sight.
            LibraryRow::Field {
                depth,
                naming,
                initial,
            } => name_row(cx, row_rect, depth, naming, initial, memory),
        }
    }
    for (index, row) in browser.tree.iter().enumerate() {
        let (row_rect, whole) = slot(fixed + index);
        if area.intersects(row_rect) {
            tree_row(cx, &painter, row_rect, index, row, whole);
        }
    }
    if let Some(node) = recordings {
        let (row_rect, whole) = slot(fixed + browser.tree.len());
        if area.intersects(row_rect) {
            node_row(cx, &painter, row_rect, node, whole, memory, selected);
        }
    }
    scroll_thumb(cx, area, memory.tree_scroll, content);
}

/// Height of a chip of the favourites, the gap between two and the lines they fill.
const CHIP_HEIGHT: f32 = 24.0;
const CHIP_GAP: f32 = 4.0;
const FAVOURITE_LINES: usize = 2;
/// The fixed chips show an icon alone, so the places of the playlists have room for a name.
const ICON_CHIP_WIDTH: f32 = 32.0;

/// A chip of the favourites.
struct Chip<'a> {
    place: usize,
    /// The icon of a fixed chip, which names its source by its label alone.
    glyph: Option<Glyph>,
    label: Option<&'a str>,
    /// What pressing it does; a chip without one is not operated.
    action: Option<AppCommand>,
    current: bool,
}

/// The favourites: *Collection*, *Preparation* and *History* first, then the places that take
/// a playlist (`docs/behavior/browser.md`, *Favourites*).
fn favourites(
    cx: &mut Cx<'_>,
    area: Rect,
    collection: &CollectionView,
    memory: &mut BrowserMemory,
) {
    let texts = cx.texts;
    let preparation = collection.playlists.iter().find(|row| row.preparation);
    let mut chips = vec![
        Chip {
            place: 1,
            glyph: Some(Glyph::FileAudio),
            label: Some(texts.get("browser.collection")),
            action: Some(AppCommand::BrowserNodeSelect {
                node: BrowserNode::Collection,
            }),
            current: collection.current,
        },
        Chip {
            place: 2,
            glyph: Some(Glyph::Bookmark),
            label: Some(texts.get("browser.preparation")),
            action: preparation.map(|_| AppCommand::BrowserNodeSelect {
                node: BrowserNode::Preparation,
            }),
            current: preparation.is_some_and(|row| row.current),
        },
        Chip {
            place: 3,
            glyph: Some(Glyph::History),
            label: Some(texts.get("browser.history")),
            action: collection
                .session
                .as_ref()
                .map(|_| AppCommand::BrowserNodeSelect {
                    node: BrowserNode::History,
                }),
            current: collection.session.as_ref().is_some_and(|row| row.current),
        },
    ];
    chips.extend(FAVOURITE_PLACES.map(|place| {
        let row = collection
            .playlists
            .iter()
            .find(|row| row.favourite == Some(place));
        Chip {
            place,
            glyph: None,
            label: row.map(|row| row.name.as_str()),
            action: row.map(|row| AppCommand::BrowserPlaylistSelect { playlist: row.id }),
            current: row.is_some_and(|row| row.current),
        }
    }));
    // The first line: the three icons, then two places; the second: three places.
    let fixed = 3.0 * (ICON_CHIP_WIDTH + CHIP_GAP);
    let first = (area.width() - fixed - CHIP_GAP) / 2.0;
    let second = (area.width() - 2.0 * CHIP_GAP) / 3.0;
    let mut x = area.left();
    for chip in chips {
        let (line, width) = match chip.place {
            1..=3 => (0, ICON_CHIP_WIDTH),
            4 | 5 => (0, first),
            _ => (1, second),
        };
        if chip.place == 6 {
            x = area.left();
        }
        let top = area.top() + line as f32 * (CHIP_HEIGHT + CHIP_GAP);
        let rect = Rect::from_min_size(egui::pos2(x, top), egui::vec2(width, CHIP_HEIGHT));
        x = rect.right() + CHIP_GAP;
        favourite_chip(cx, rect, chip, collection, memory);
    }
}

fn favourite_chip(
    cx: &mut Cx<'_>,
    rect: Rect,
    chip: Chip<'_>,
    collection: &CollectionView,
    memory: &mut BrowserMemory,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let assigned = FAVOURITE_PLACES.contains(&chip.place) && chip.label.is_some();
    let free = FAVOURITE_PLACES.contains(&chip.place) && chip.label.is_none();
    if let Some(action) = &chip.action {
        cx.record(action.id(), rect, ControlKind::Button, true);
    }
    let sense = if chip.action.is_some() {
        cx.sense(true, Sense::click())
    } else {
        Sense::hover()
    };
    let response = cx.ui.interact(rect, id(("favourite", chip.place)), sense);
    let label = chip.label.unwrap_or_default();
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, chip.current, label));
    // A playlist of the tree dropped on a free place goes there; a folder does not.
    let mut dropping = false;
    if free && cx.interactive {
        let playlist = |dragged: &NodeId| {
            collection
                .playlists
                .iter()
                .any(|row| row.id == *dragged && !row.folder)
        };
        dropping = response
            .dnd_hover_payload::<NodeId>()
            .is_some_and(|dragged| playlist(&dragged));
        if let Some(dragged) = response.dnd_release_payload::<NodeId>()
            && playlist(&dragged)
        {
            cx.out.emit(AppCommand::BrowserFavouriteSet {
                place: chip.place,
                playlist: *dragged,
            });
        }
    }
    let painter = cx.painter();
    let radius = paint::radius(tokens.radius.md);
    if dropping {
        painter.rect(
            rect,
            radius,
            color.surface_hover,
            paint::hairline(theme, color.focus_ring),
            egui::StrokeKind::Inside,
        );
    } else if free {
        painter.rect_stroke(
            rect,
            radius,
            paint::hairline(theme, color.line_divider),
            egui::StrokeKind::Inside,
        );
    } else {
        let fill = if chip.current {
            color.surface_selected
        } else if chip.action.is_some() && response.hovered() && cx.interactive {
            color.surface_hover
        } else {
            color.surface_control
        };
        painter.rect_filled(rect, radius, fill);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            radius,
            paint::hairline(theme, color.focus_ring),
            egui::StrokeKind::Inside,
        );
        cx.out.claim(&[Key::Space, Key::Enter]);
    }
    let ink = if chip.action.is_some() {
        color.text_primary
    } else {
        color.text_secondary
    };
    if let Some(glyph) = chip.glyph {
        let icon = Rect::from_center_size(rect.center(), egui::vec2(14.0, 14.0));
        theme.icons.paint(painter, glyph, icon, ink);
    } else {
        paint::text_elided(
            painter,
            theme,
            rect.shrink2(egui::vec2(tokens.space.s2, 0.0)),
            label,
            tokens.text.label,
            ink,
        );
    }
    if response.clicked() {
        response.request_focus();
        if let Some(action) = chip.action {
            cx.out.emit(action);
        }
    }
    if assigned && cx.interactive {
        let keyed = response.has_focus() && menu_key(cx);
        if keyed || response.secondary_clicked() {
            let at = response
                .interact_pointer_pos()
                .filter(|_| !keyed)
                .unwrap_or_else(|| rect.left_bottom());
            memory.menu = Some(BrowserMenu {
                target: MenuTarget::Favourite(chip.place),
                at,
            });
        }
    }
}

/// A row of the library in the tree: a node, or the field that names one.
enum LibraryRow<'a> {
    // Boxed: a node carries the command of its row, much larger than a field.
    Node(Box<Node<'a>>),
    Field {
        depth: usize,
        naming: Naming,
        initial: &'a str,
    },
}

/// The rows of the collection and the playlists, with the name field where it is open. A
/// field whose folder or item is gone closes.
fn library_rows<'a>(
    cx: &Cx<'a>,
    collection: &'a CollectionView,
    browser: &BrowserView,
    memory: &mut BrowserMemory,
) -> Vec<LibraryRow<'a>> {
    let mut rows = vec![
        LibraryRow::Node(Box::new(collection_node(cx, collection))),
        LibraryRow::Node(Box::new(playlists_heading(cx))),
    ];
    // What a closed folder holds is left out: the rows come folder by folder, each followed
    // by what it holds, one level deeper.
    let mut hidden_below: Option<usize> = None;
    let shown: Vec<&PlaylistRow> = collection
        .playlists
        .iter()
        .filter(|row| {
            if hidden_below.is_some_and(|depth| row.depth > depth) {
                return false;
            }
            hidden_below =
                (row.folder && browser.closed_folders.contains(&row.id)).then_some(row.depth);
            true
        })
        .collect();
    rows.extend(
        shown
            .iter()
            .map(|row| LibraryRow::Node(Box::new(playlist_node(row, browser)))),
    );
    let place = |item: NodeId| {
        shown
            .iter()
            .position(|row| row.id == item)
            .map(|index| (index + 2, shown[index]))
    };
    match memory.naming {
        Some(naming @ Naming::New { folder: None, .. }) => rows.insert(
            2,
            LibraryRow::Field {
                depth: 1,
                naming,
                initial: "",
            },
        ),
        Some(
            naming @ Naming::New {
                folder: Some(folder),
                ..
            },
        ) => match place(folder) {
            Some((at, row)) if row.folder => rows.insert(
                at + 1,
                LibraryRow::Field {
                    depth: row.depth + 2,
                    naming,
                    initial: "",
                },
            ),
            _ => memory.naming = None,
        },
        Some(naming @ Naming::Rename(item)) => match place(item) {
            Some((at, row)) => {
                rows[at] = LibraryRow::Field {
                    depth: row.depth + 1,
                    naming,
                    initial: &row.name,
                };
            }
            None => memory.naming = None,
        },
        None => {}
    }
    rows.push(LibraryRow::Node(Box::new(preparation_node(cx, collection))));
    // The history of this run, then the earlier ones, below the playlists.
    if let Some(session) = &collection.session {
        let earlier = !collection.sessions.is_empty();
        rows.push(LibraryRow::Node(Box::new(history_node(
            cx,
            session,
            earlier.then_some(browser.history_open),
        ))));
        if browser.history_open {
            rows.extend(
                collection
                    .sessions
                    .iter()
                    .map(|row| LibraryRow::Node(Box::new(session_node(row)))),
            );
        }
    }
    rows
}

fn preparation_node<'a>(cx: &Cx<'a>, collection: &CollectionView) -> Node<'a> {
    let list = collection.playlists.iter().find(|row| row.preparation);
    Node {
        id: id("tree-preparation"),
        glyph: Glyph::Bookmark,
        name: cx.texts.get("browser.preparation"),
        depth: 0,
        count: list.and_then(|row| row.entries),
        current: list.is_some_and(|row| row.current),
        action: list.map(|_| AppCommand::BrowserNodeSelect {
            node: BrowserNode::Preparation,
        }),
        menu: None,
        takes: list.map(|row| row.id),
        open: None,
    }
}

/// *History*, with a chevron while it has earlier sessions under it.
fn history_node<'a>(cx: &Cx<'a>, session: &SessionRow, open: Option<bool>) -> Node<'a> {
    Node {
        id: id("tree-history"),
        glyph: Glyph::History,
        name: cx.texts.get("browser.history"),
        depth: 0,
        count: Some(session.tracks),
        current: session.current,
        action: Some(AppCommand::BrowserNodeSelect {
            node: BrowserNode::History,
        }),
        menu: Some(MenuTarget::Session(session.id)),
        takes: None,
        open: open.map(|open| (open, Turn::History)),
    }
}

fn session_node(row: &SessionRow) -> Node<'_> {
    Node {
        id: id(("tree-session", row.id)),
        glyph: Glyph::Clock,
        name: &row.label,
        depth: 1,
        count: Some(row.tracks),
        current: row.current,
        action: Some(AppCommand::BrowserHistorySelect { session: row.id }),
        menu: Some(MenuTarget::Session(row.id)),
        takes: None,
        open: None,
    }
}

/// The field that names a playlist or a folder, in a row of the tree. Enter with a name sends
/// the command; Escape, Enter without a name or a press elsewhere close it and send nothing.
fn name_row(
    cx: &mut Cx<'_>,
    rect: Rect,
    depth: usize,
    naming: Naming,
    initial: &str,
    memory: &mut BrowserMemory,
) {
    let tokens = &cx.theme.tokens;
    let indent = rect.left() + tokens.space.s3 + depth as f32 * TREE_INDENT + 12.0;
    let field = Rect::from_min_max(
        egui::pos2(indent, rect.top() + 1.0),
        rect.max - egui::vec2(0.0, 1.0),
    );
    let (command, placeholder) = match naming {
        Naming::New {
            item: NewItem::Folder,
            ..
        } => ("browser.playlist.create", "browser.name_folder"),
        Naming::New {
            item: NewItem::Smart,
            ..
        } => ("browser.playlist.create", "browser.name_smart"),
        Naming::New { .. } => ("browser.playlist.create", "browser.name_playlist"),
        Naming::Rename(_) => ("browser.playlist.rename", "browser.name_playlist"),
    };
    let placeholder = cx.texts.get(placeholder);
    let Some(finish) = field::name(
        cx,
        field,
        id(("tree-name", naming)),
        initial,
        placeholder,
        command,
    ) else {
        return;
    };
    memory.naming = None;
    if let Finish::Confirm(name) = finish {
        cx.out.emit(match naming {
            Naming::New { folder, item } => {
                AppCommand::BrowserPlaylistCreate { folder, item, name }
            }
            Naming::Rename(item) => AppCommand::BrowserPlaylistRename { item, name },
        });
    }
}

/// A node of the library in the tree: the collection, the heading of the playlists, a folder
/// of playlists or a playlist.
struct Node<'a> {
    id: egui::Id,
    glyph: Glyph,
    name: &'a str,
    /// Level below the top of the tree.
    depth: usize,
    count: Option<usize>,
    current: bool,
    /// What operating the row does; a row without one is not operated.
    action: Option<AppCommand>,
    /// What its context menu acts on, when it has one.
    menu: Option<MenuTarget>,
    /// The playlist that takes the tracks dropped on the row or added with Insert.
    takes: Option<NodeId>,
    /// A row that closes and opens with a chevron: whether it is open, and what it is.
    open: Option<(bool, Turn)>,
}

/// A row of the library that closes and opens.
#[derive(Clone, Copy)]
enum Turn {
    Folder(NodeId),
    History,
}

impl Turn {
    fn command(self) -> AppCommand {
        match self {
            Self::Folder(folder) => AppCommand::BrowserFolderToggle { folder },
            Self::History => AppCommand::BrowserHistoryToggle,
        }
    }
}

fn collection_node<'a>(cx: &Cx<'a>, collection: &CollectionView) -> Node<'a> {
    Node {
        id: id("tree-collection"),
        glyph: Glyph::FileAudio,
        name: cx.texts.get("browser.collection"),
        depth: 0,
        count: collection.tracks,
        current: collection.current,
        action: Some(AppCommand::BrowserNodeSelect {
            node: BrowserNode::Collection,
        }),
        menu: None,
        takes: None,
        open: None,
    }
}

fn playlists_heading<'a>(cx: &Cx<'a>) -> Node<'a> {
    Node {
        id: id("tree-playlists"),
        glyph: Glyph::Playlist,
        name: cx.texts.get("browser.playlists"),
        depth: 0,
        count: None,
        current: false,
        action: None,
        menu: Some(MenuTarget::Playlists),
        takes: None,
        open: None,
    }
}

fn playlist_node<'a>(row: &'a PlaylistRow, browser: &BrowserView) -> Node<'a> {
    Node {
        id: id(("tree-playlist", row.id)),
        glyph: if row.folder {
            Glyph::Folder
        } else if row.preparation {
            Glyph::Bookmark
        } else {
            Glyph::Playlist
        },
        name: &row.name,
        depth: row.depth + 1,
        count: row.entries,
        current: row.current,
        action: (!row.folder).then_some(AppCommand::BrowserPlaylistSelect { playlist: row.id }),
        menu: Some(if row.folder {
            MenuTarget::Folder(row.id)
        } else {
            MenuTarget::Playlist(row.id)
        }),
        // A smart playlist takes its tracks from its rules, not from a drop.
        takes: (!row.folder && !row.smart).then_some(row.id),
        open: row.folder.then(|| {
            (
                !browser.closed_folders.contains(&row.id),
                Turn::Folder(row.id),
            )
        }),
    }
}

/// The chevron of a node that closes and opens, as an explorer folder has: the pointer operates
/// it, and with the keyboard on the row the left arrow closes and the right arrow opens.
fn chevron(
    cx: &mut Cx<'_>,
    painter: &egui::Painter,
    rect: Rect,
    indent: f32,
    &(open, turn): &(bool, Turn),
    row: &egui::Response,
    whole: bool,
) {
    let toggle = turn.command();
    let theme = cx.theme;
    let middle = rect.center().y;
    let place = Rect::from_center_size(egui::pos2(indent + 6.0, middle), egui::vec2(12.0, 12.0));
    let glyph = if open {
        Glyph::ChevronDown
    } else {
        Glyph::ChevronRight
    };
    theme
        .icons
        .paint(painter, glyph, place, theme.tokens.color.text_secondary);
    let hit = place.expand(4.0);
    if whole {
        cx.record_pointer_only(toggle.id(), hit, true);
        cx.record(toggle.id(), rect, ControlKind::Keys, true);
    }
    let response = cx.ui.interact(
        hit,
        row.id.with("chevron"),
        if whole && cx.interactive {
            Sense::CLICK
        } else {
            Sense::hover()
        },
    );
    let mut toggled = response.clicked();
    if row.has_focus() {
        let filter = EventFilter {
            horizontal_arrows: true,
            ..EventFilter::default()
        };
        cx.ui
            .memory_mut(|memory| memory.set_focus_lock_filter(row.id, filter));
        cx.out.claim(&[Key::ArrowLeft, Key::ArrowRight]);
        let (left, right) = cx.ui.input(|input| {
            (
                input.key_pressed(Key::ArrowLeft),
                input.key_pressed(Key::ArrowRight),
            )
        });
        toggled |= (left && open) || (right && !open);
    }
    if toggled {
        cx.out.emit(toggle);
    }
}

/// A node of the library: a chevron for a folder or *History*, its icon, its name and the
/// number of its tracks.
fn node_row(
    cx: &mut Cx<'_>,
    painter: &egui::Painter,
    rect: Rect,
    node: Node<'_>,
    whole: bool,
    memory: &mut BrowserMemory,
    selected: Option<&std::path::Path>,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    if let Some(action) = &node.action
        && whole
    {
        cx.record(action.id(), rect, ControlKind::Button, true);
    }
    // A row with a menu takes the keyboard too, so Shift+F10 reaches its menu.
    // A playlist or a folder can also be dragged onto a folder, or onto *Playlists*.
    let item = match node.menu {
        Some(MenuTarget::Playlist(item) | MenuTarget::Folder(item)) => Some(item),
        _ => None,
    };
    let sense = if item.is_some() {
        cx.sense(whole, Sense::click_and_drag())
    } else if node.action.is_some() || node.menu.is_some() {
        cx.sense(whole, Sense::click())
    } else {
        Sense::hover()
    };
    let response = cx.ui.interact(rect, node.id, sense);
    response.widget_info(|| WidgetInfo::selected(Role::TreeItem, true, node.current, node.name));
    let playlist = node.takes;
    // A track dragged from the list goes at the end of the playlist it is dropped on; with
    // the row focused, Insert adds the track the list has selected.
    let mut dropping = false;
    if let Some(playlist) = playlist
        && cx.interactive
    {
        if whole {
            cx.record("browser.playlist.add", rect, ControlKind::Keys, true);
        }
        dropping = response.dnd_hover_payload::<PathBuf>().is_some();
        let dropped = response.dnd_release_payload::<PathBuf>();
        let inserted = response.has_focus() && cx.ui.input(|input| input.key_pressed(Key::Insert));
        if response.has_focus() {
            cx.out.claim(&[Key::Insert]);
        }
        let path = dropped.map(|path| path.as_ref().clone()).or_else(|| {
            selected
                .filter(|_| inserted)
                .map(std::path::Path::to_path_buf)
        });
        if let Some(path) = path {
            cx.out
                .emit(AppCommand::BrowserPlaylistAdd { playlist, path });
        }
    }
    // Where a dragged playlist or folder may go: a folder, or *Playlists* for the top.
    let folder = match node.menu {
        Some(MenuTarget::Folder(folder)) => Some(Some(folder)),
        Some(MenuTarget::Playlists) => Some(None),
        _ => None,
    };
    if cx.interactive {
        if let Some(item) = item
            && response.drag_started()
        {
            response.dnd_set_drag_payload(item);
        }
        if let Some(folder) = folder {
            dropping |= response
                .dnd_hover_payload::<NodeId>()
                .is_some_and(|dragged| Some(*dragged) != folder);
            if let Some(dragged) = response.dnd_release_payload::<NodeId>()
                && Some(*dragged) != folder
            {
                cx.out.emit(AppCommand::BrowserPlaylistMove {
                    item: *dragged,
                    folder,
                });
            }
        }
    }
    if dropping {
        painter.rect(
            rect,
            paint::radius(tokens.radius.md),
            color.surface_hover,
            paint::hairline(theme, color.focus_ring),
            egui::StrokeKind::Inside,
        );
    } else if node.current {
        painter.rect_filled(
            rect,
            paint::radius(tokens.radius.md),
            color.surface_selected,
        );
    } else if node.action.is_some() && response.hovered() && cx.interactive {
        painter.rect_filled(rect, paint::radius(tokens.radius.md), color.surface_hover);
    }
    if response.has_focus() {
        cx.out.claim(&[Key::Space, Key::Enter]);
    }
    if response.clicked() {
        response.request_focus();
        if let Some(action) = node.action {
            cx.out.emit(action);
        }
    }
    if let Some(target) = node.menu
        && cx.interactive
    {
        let keyed = response.has_focus() && menu_key(cx);
        if keyed || response.secondary_clicked() {
            let at = response
                .interact_pointer_pos()
                .filter(|_| !keyed)
                .unwrap_or_else(|| rect.left_bottom());
            memory.menu = Some(BrowserMenu { target, at });
        }
    }
    let middle = rect.center().y;
    // Lined up with the folder icons of the explorer, after the room of their chevron.
    let indent = rect.left() + tokens.space.s3 + node.depth as f32 * TREE_INDENT;
    if let Some(state) = &node.open {
        chevron(cx, painter, rect, indent, state, &response, whole);
    }
    let icon = Rect::from_center_size(
        egui::pos2(indent + 12.0 + 6.0 + 7.0, middle),
        egui::vec2(14.0, 14.0),
    );
    theme
        .icons
        .paint(painter, node.glyph, icon, color.text_secondary);
    let count = node.count.map(|count| count.to_string());
    let count_width = count.as_deref().map_or(0.0, |count| {
        paint::text_width(painter, theme, count, tokens.text.readout) + tokens.space.s3
    });
    let label = Rect::from_min_max(
        egui::pos2(icon.right() + 6.0, rect.top()),
        egui::pos2(rect.right() - tokens.space.s3 - count_width, rect.bottom()),
    );
    paint::text_elided(
        painter,
        theme,
        label,
        node.name,
        tokens.text.body,
        color.text_primary,
    );
    if let Some(count) = &count {
        let area = Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right() - tokens.space.s3, rect.bottom()),
        );
        paint::text(
            painter,
            theme,
            area,
            Align2::RIGHT_CENTER,
            count,
            tokens.text.readout,
            color.text_secondary,
        );
    }
    if response.has_focus() {
        paint::focus_ring(
            painter,
            theme,
            rect.shrink(tokens.space.s1 + tokens.stroke.focus),
            tokens.radius.md,
        );
    }
}

fn tree_row(
    cx: &mut Cx<'_>,
    painter: &egui::Painter,
    rect: Rect,
    index: usize,
    row: &TreeRow,
    whole: bool,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let action = AppCommand::BrowserNavigate {
        path: row.path.clone(),
    };
    if whole {
        cx.record(action.id(), rect, ControlKind::Button, true);
    }
    let response = cx.ui.interact(
        rect,
        id(("tree-row", index)),
        cx.sense(whole, Sense::click()),
    );
    response.widget_info(|| WidgetInfo::selected(Role::TreeItem, true, row.current, &row.name));
    let focused = response.has_focus();
    if row.current {
        painter.rect_filled(
            rect,
            paint::radius(tokens.radius.md),
            color.surface_selected,
        );
    } else if response.hovered() && cx.interactive {
        painter.rect_filled(rect, paint::radius(tokens.radius.md), color.surface_hover);
    }
    let indent = rect.left() + tokens.space.s3 + row.depth as f32 * TREE_INDENT;
    let middle = rect.center().y;
    let chevron = Rect::from_center_size(egui::pos2(indent + 6.0, middle), egui::vec2(12.0, 12.0));
    let glyph = if row.open {
        Glyph::ChevronDown
    } else {
        Glyph::ChevronRight
    };
    theme
        .icons
        .paint(painter, glyph, chevron, color.text_secondary);
    let toggle = AppCommand::BrowserToggle {
        path: row.path.clone(),
    };
    let chevron_hit = chevron.expand(4.0);
    if whole {
        cx.record_pointer_only(toggle.id(), chevron_hit, true);
        cx.record(toggle.id(), rect, ControlKind::Keys, true);
    }
    // The chevron is operated by the pointer; with the keyboard the row itself opens and
    // closes with the left and right arrows.
    let chevron_response = cx.ui.interact(
        chevron_hit,
        id(("tree-chevron", index)),
        if whole && cx.interactive {
            Sense::CLICK
        } else {
            Sense::hover()
        },
    );
    let mut toggled = chevron_response.clicked();
    if focused {
        let filter = EventFilter {
            horizontal_arrows: true,
            ..EventFilter::default()
        };
        cx.ui
            .memory_mut(|memory| memory.set_focus_lock_filter(response.id, filter));
        cx.out
            .claim(&[Key::Space, Key::Enter, Key::ArrowLeft, Key::ArrowRight]);
        let (left, right) = cx.ui.input(|input| {
            (
                input.key_pressed(Key::ArrowLeft),
                input.key_pressed(Key::ArrowRight),
            )
        });
        toggled |= (left && row.open) || (right && !row.open);
    }
    if toggled {
        cx.out.emit(toggle);
    } else if response.clicked() {
        cx.out.emit(action);
    }
    let icon = Rect::from_center_size(
        egui::pos2(chevron.right() + 6.0 + 7.0, middle),
        egui::vec2(14.0, 14.0),
    );
    theme
        .icons
        .paint(painter, Glyph::Folder, icon, color.text_secondary);
    let count = row.files.map(|files| files.to_string());
    let count_width = count.as_deref().map_or(0.0, |count| {
        paint::text_width(painter, theme, count, tokens.text.readout) + tokens.space.s3
    });
    let name = Rect::from_min_max(
        egui::pos2(icon.right() + 6.0, rect.top()),
        egui::pos2(rect.right() - tokens.space.s3 - count_width, rect.bottom()),
    );
    paint::text_elided(
        painter,
        theme,
        name,
        &row.name,
        tokens.text.body,
        color.text_primary,
    );
    if let Some(count) = &count {
        let area = Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right() - tokens.space.s3, rect.bottom()),
        );
        paint::text(
            painter,
            theme,
            area,
            Align2::RIGHT_CENTER,
            count,
            tokens.text.readout,
            color.text_secondary,
        );
    }
    if focused {
        paint::focus_ring(
            painter,
            theme,
            rect.shrink(tokens.space.s1 + tokens.stroke.focus),
            tokens.radius.md,
        );
    }
}

/// The path of the current folder, its file count and the two load keys.
fn toolbar(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>, focus_search: bool) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    let middle = rect.center().y;

    // The load keys first: the path is cut to the room they leave.
    let has_track = view
        .browser
        .selected
        .and_then(|index| view.browser.rows.get(index))
        .is_some_and(|row| matches!(row.kind, RowKind::Track(_) | RowKind::Pending));
    let mut right = rect.right() - 10.0;
    let collection = view.browser.collection.as_ref();
    // A folder of the explorer can be imported into the collection.
    if let (Some(collection), Some(folder)) = (collection, &view.browser.folder) {
        // While an import runs, the key stops it.
        let label = texts.get(if collection.importing {
            "browser.import_stop"
        } else {
            "browser.import"
        });
        let width = key::width_for(cx, label, 0.0) + tokens.space.s4;
        let key_rect = Rect::from_min_size(
            egui::pos2(right - width, middle - tokens.size.button_h / 2.0),
            egui::vec2(width, tokens.size.button_h),
        );
        let spec = KeySpec {
            face: Face::Text(label),
            name: label,
            lit: None,
            held: None,
            enabled: collection.available == Some(true),
            keeps_led: false,
            text: None,
        };
        let action = if collection.importing {
            AppCommand::LibraryImportStop
        } else {
            AppCommand::LibraryImportFolder {
                path: folder.clone(),
            }
        };
        key::key(
            cx,
            key_rect,
            id("browser-import"),
            spec,
            Press::Click(action.into()),
        );
        right = key_rect.left() - tokens.space.s4;
    }
    // One key per deck the window shows, A first; the arrow points to the column of the deck.
    for &deck in view.settings.shown_decks().iter().rev() {
        let (glyph, label) = match deck {
            DeckId::A => (Glyph::LoadLeft, "browser.load_a"),
            DeckId::B => (Glyph::LoadRight, "browser.load_b"),
            DeckId::C => (Glyph::LoadLeft, "browser.load_c"),
            DeckId::D => (Glyph::LoadRight, "browser.load_d"),
        };
        let label = texts.get(label);
        let width = key::width_for(cx, label, 0.0) + 14.0 + tokens.space.s2 + tokens.space.s1;
        let key_rect = Rect::from_min_size(
            egui::pos2(right - width, middle - tokens.size.button_h / 2.0),
            egui::vec2(width, tokens.size.button_h),
        );
        right = key_rect.left() - tokens.space.s4;
        let spec = KeySpec {
            face: Face::IconText(glyph, label),
            name: label,
            lit: None,
            held: None,
            enabled: has_track,
            keeps_led: false,
            text: None,
        };
        key::key(
            cx,
            key_rect,
            id(("browser-load", deck.index())),
            spec,
            Press::Click(AppCommand::BrowserLoadSelected { deck }.into()),
        );
    }

    right = preview_player(cx, right, middle, view) - tokens.space.s4;

    // The search field, left of the keys; the path is cut to the room that is left.
    let field = Rect::from_min_size(
        egui::pos2(right - SEARCH_WIDTH, middle - SEARCH_HEIGHT / 2.0),
        egui::vec2(SEARCH_WIDTH, SEARCH_HEIGHT),
    );
    field::search(
        cx,
        field,
        id("browser-search"),
        &view.browser.search,
        focus_search,
        id("browser-list"),
    );
    right = field.left() - tokens.space.s4;
    let painter = cx.painter();
    let mut x = rect.left() + 10.0;
    // What the list shows of the library names itself; a folder shows its path.
    let shown = collection.filter(|collection| collection.shown());
    let playlist = shown.and_then(|collection| collection.playlists.iter().find(|row| row.current));
    // The history shown: the one of this run, or an earlier one, which carries its label.
    let earlier = shown.and_then(|collection| collection.sessions.iter().find(|row| row.current));
    let history = shown.and_then(|collection| {
        collection
            .session
            .as_ref()
            .filter(|row| row.current)
            .or(earlier)
    });
    let crumbs = match shown {
        Some(collection) if collection.current => {
            vec![texts.get("browser.collection").to_owned()]
        }
        Some(collection) if collection.history.is_some() => {
            std::iter::once(texts.get("browser.history").to_owned())
                .chain(earlier.map(|row| row.label.clone()))
                .collect()
        }
        Some(_) => std::iter::once(texts.get("browser.playlists").to_owned())
            .chain(playlist.map(|row| row.name.clone()))
            .collect(),
        None => view.browser.crumbs.clone(),
    };
    let last = crumbs.len().saturating_sub(1);
    for (index, crumb) in crumbs.iter().enumerate() {
        if index > 0 {
            let chevron =
                Rect::from_center_size(egui::pos2(x + 6.0, middle), egui::vec2(12.0, 12.0));
            theme
                .icons
                .paint(painter, Glyph::ChevronRight, chevron, color.text_secondary);
            x = chevron.right() + tokens.space.s2 + tokens.space.s1;
        }
        let ink = if index == last {
            color.text_primary
        } else {
            color.text_secondary
        };
        let area = Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(right, rect.bottom()));
        if area.width() < 24.0 {
            break;
        }
        let drawn = paint::text(
            painter,
            theme,
            area,
            Align2::LEFT_CENTER,
            crumb,
            tokens.text.body,
            ink,
        );
        x = drawn.right().min(right) + tokens.space.s2 + tokens.space.s1;
    }
    let mut count = match shown {
        // Every track of what is shown: the collection, or the entries of the playlist.
        Some(collection) => match (
            if collection.current {
                collection.tracks
            } else if collection.history.is_some() {
                history.map(|row| row.tracks)
            } else {
                playlist.and_then(|row| row.entries)
            },
            collection.matched,
        ) {
            (Some(tracks), Some(matched)) if !view.browser.search.trim().is_empty() => texts
                .format(
                    "browser.match_count",
                    &[
                        ("count", &matched.to_string()),
                        ("total", &tracks.to_string()),
                    ],
                ),
            (Some(tracks), _) => {
                texts.format("browser.track_count", &[("count", &tracks.to_string())])
            }
            (None, _) => String::new(),
        },
        None => texts.format(
            "browser.file_count",
            &[("count", &view.browser.file_count().to_string())],
        ),
    };
    let exchange = collection.and_then(|collection| match &collection.exchanged {
        _ if collection.exchanging => Some(texts.get("browser.exchanging").to_owned()),
        Some(PlaylistFileOutcome::Exported { file, entries }) => Some(texts.format(
            "browser.exported",
            &[("file", file), ("entries", &entries.to_string())],
        )),
        Some(PlaylistFileOutcome::Imported {
            playlists,
            tracks,
            skipped,
        }) => Some(texts.format(
            "browser.playlists_imported",
            &[
                ("playlists", &playlists.to_string()),
                ("tracks", &tracks.to_string()),
                ("skipped", &skipped.to_string()),
            ],
        )),
        Some(PlaylistFileOutcome::Failed) => Some(texts.get("browser.exchange_failed").to_owned()),
        None => None,
    });
    let status = exchange.or_else(|| {
        collection.and_then(|collection| match collection.imported {
            _ if collection.importing => Some(texts.format(
                "browser.importing",
                &[("looked", &collection.import_looked.to_string())],
            )),
            Some(ImportOutcome::Done {
                added,
                known,
                unreadable,
                stopped,
            }) => Some(texts.format(
                if stopped {
                    "browser.import_stopped"
                } else {
                    "browser.imported"
                },
                &[
                    ("added", &added.to_string()),
                    ("known", &known.to_string()),
                    ("unreadable", &unreadable.to_string()),
                ],
            )),
            Some(ImportOutcome::Failed) => Some(texts.get("browser.import_failed").to_owned()),
            None => None,
        })
    });
    if let Some(status) = status {
        count = if count.is_empty() {
            status
        } else {
            texts.format(
                "browser.count_status",
                &[("count", &count), ("status", &status)],
            )
        };
    }
    let area = Rect::from_min_max(
        egui::pos2(x + tokens.space.s2, rect.top()),
        egui::pos2(right, rect.bottom()),
    );
    if area.width() > 60.0 {
        paint::text(
            painter,
            theme,
            area,
            Align2::LEFT_CENTER,
            &count,
            tokens.text.readout,
            color.text_secondary,
        );
    }
}

/// The deck a file is loaded in, if any.
fn loaded_in(view: &View<'_>, row: &FileRow) -> Option<DeckId> {
    DeckId::ALL.into_iter().find(|deck| {
        view.engine.decks[deck.index()]
            .track
            .as_ref()
            .is_some_and(|track| track.path == row.path)
    })
}

fn list(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>, memory: &mut BrowserMemory) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    let browser = view.browser;
    let look = RowLook::of(view);

    let header = Rect::from_min_size(rect.min, egui::vec2(rect.width(), ROW_HEIGHT));
    let columns = Columns::of(header, &browser.columns);
    // A secondary click anywhere on the header opens the menu of the columns; the headings
    // that sort lie over this.
    let whole = cx
        .ui
        .interact(header, id("browser-header"), cx.sense(true, Sense::click()));
    let mut menu_asked = whole.secondary_clicked();
    menu_asked |= heading(
        cx,
        columns.title,
        texts.get("browser.title"),
        Some(SortColumn::Title),
        browser.sort,
        false,
    );
    for column in ListColumn::ALL {
        if let Some(place) = columns.get(column) {
            menu_asked |= heading(
                cx,
                place,
                texts.get(label_of(column)),
                column.sort(),
                browser.sort,
                column == ListColumn::Colour,
            );
        }
    }
    if menu_asked && cx.interactive {
        let at = cx
            .ui
            .input(|input| input.pointer.interact_pos())
            .unwrap_or_else(|| header.left_bottom());
        memory.menu = Some(BrowserMenu {
            target: MenuTarget::Columns,
            at,
        });
    }
    let painter = cx.painter();
    painter.hline(
        header.x_range(),
        header.bottom() - 0.5,
        paint::hairline(theme, color.line_divider),
    );

    let area = Rect::from_min_max(header.left_bottom(), rect.max);
    let content = browser.rows.len() as f32 * ROW_HEIGHT;
    // The list is one control: the pointer picks rows by where it is, and the keyboard
    // moves the selection.
    let select = AppCommand::BrowserSelect { index: 0 };
    cx.record(
        select.id(),
        area,
        ControlKind::Area,
        !browser.rows.is_empty(),
    );
    // Control and Shift change what a click or an arrow does to the selection.
    for command in [
        "browser.move",
        "browser.activate",
        "browser.up",
        "browser.select.toggle",
        "browser.select.extend",
    ] {
        cx.record(command, area, ControlKind::Keys, true);
    }
    let response = cx.ui.interact(
        area,
        id("browser-list"),
        cx.sense(true, Sense::click_and_drag()),
    );
    let name = texts.get("browser.tracks");
    response.widget_info(|| WidgetInfo::labeled(Role::ListBox, true, name));
    if response.hovered() && cx.interactive {
        memory.list_scroll -= cx.ui.input(|input| input.smooth_scroll_delta.y);
    }
    if browser.selected != memory.seen_selection {
        memory.seen_selection = browser.selected;
        if let Some(selected) = browser.selected {
            // Bring the selection into view by the shortest scroll.
            let top = selected as f32 * ROW_HEIGHT;
            memory.list_scroll = memory.list_scroll.clamp(
                top + ROW_HEIGHT - area.height(),
                top.max(top + ROW_HEIGHT - area.height()),
            );
        }
    }
    memory.list_scroll = clamp_scroll(memory.list_scroll, content, area.height());
    let row_at = |y: f32| {
        let index = ((y - area.top() + memory.list_scroll) / ROW_HEIGHT).floor();
        (index >= 0.0 && (index as usize) < browser.rows.len()).then_some(index as usize)
    };
    // A drag is recognised only once the pointer has moved; the row is the one it was
    // pressed on, not the one it has reached by then.
    let pressed_at = cx.ui.input(|input| input.pointer.press_origin());
    let pointed = pressed_at
        .filter(|_| response.is_pointer_button_down_on() || response.drag_started())
        .or_else(|| response.interact_pointer_pos())
        .or_else(|| response.hover_pos())
        .and_then(|pos| row_at(pos.y));
    if cx.interactive {
        if response.clicked() || response.drag_started() {
            response.request_focus();
            if let Some(index) = pointed {
                click_row(cx, browser, &response, index);
            }
        }
        if response.double_clicked()
            && let Some(row) = pointed.and_then(|index| browser.rows.get(index))
            && matches!(row.kind, RowKind::Folder)
        {
            cx.out.emit(AppCommand::BrowserNavigate {
                path: row.path.clone(),
            });
        }
        row_menu(cx, browser, &response, pointed, area, memory);
        // A track dragged out of the list carries its path to the deck it is dropped on.
        if response.drag_started()
            && let Some(row) = pointed.and_then(|index| browser.rows.get(index))
            && matches!(row.kind, RowKind::Track(_) | RowKind::Pending)
        {
            response.dnd_set_drag_payload::<PathBuf>(row.path.clone());
        }
    }
    if response.has_focus() {
        let filter = EventFilter {
            vertical_arrows: true,
            ..EventFilter::default()
        };
        cx.ui
            .memory_mut(|memory| memory.set_focus_lock_filter(response.id, filter));
        cx.out
            .claim(&[Key::ArrowUp, Key::ArrowDown, Key::Enter, Key::Backspace]);
        let (up, down, enter, back) = cx.ui.input(|input| {
            (
                input.key_pressed(Key::ArrowUp),
                input.key_pressed(Key::ArrowDown),
                input.key_pressed(Key::Enter),
                input.key_pressed(Key::Backspace),
            )
        });
        let delta = i32::from(down) - i32::from(up);
        let shift = cx.ui.input(|input| input.modifiers.shift);
        match extended(browser, delta).filter(|_| shift) {
            Some(index) => cx.out.emit(AppCommand::BrowserSelectExtend { index }),
            None if delta != 0 && !shift => cx.out.emit(AppCommand::BrowserMove { delta }),
            None => {}
        }
        if enter {
            cx.out.emit(AppCommand::BrowserActivate);
        }
        if back {
            cx.out.emit(AppCommand::BrowserUp);
        }
    }

    let painter = cx.painter().with_clip_rect(area);
    if browser.rows.is_empty() {
        let message = empty_message(browser);
        let line = Rect::from_min_size(
            area.min + egui::vec2(10.0 + BADGE + COLUMN_GAP, 0.0),
            egui::vec2(area.width() - 60.0, ROW_HEIGHT),
        );
        paint::text(
            &painter,
            theme,
            line,
            Align2::LEFT_CENTER,
            texts.get(message),
            tokens.text.body,
            color.text_secondary,
        );
    }
    let first = (memory.list_scroll / ROW_HEIGHT).floor() as usize;
    let visible = (area.height() / ROW_HEIGHT).ceil() as usize + 1;
    for (index, row) in browser.rows.iter().enumerate().skip(first).take(visible) {
        let top = area.top() + index as f32 * ROW_HEIGHT - memory.list_scroll;
        let row_rect = Rect::from_min_size(
            egui::pos2(area.left(), top),
            egui::vec2(area.width(), ROW_HEIGHT),
        );
        let selected = browser.marked.contains(&index) || browser.selected == Some(index);
        if selected {
            painter.rect_filled(row_rect, CornerRadius::ZERO, color.surface_selected);
        } else if pointed == Some(index) && response.hovered() && cx.interactive {
            painter.rect_filled(row_rect, CornerRadius::ZERO, color.surface_hover);
        }
        painter.hline(
            row_rect.x_range(),
            row_rect.bottom() - 0.5,
            paint::hairline(theme, color.line_divider),
        );
        let cover = browser.covers.get(&row.path);
        file_row(
            cx,
            &painter,
            row_rect,
            row,
            loaded_in(view, row),
            cover,
            look,
        );
        if selected && response.has_focus() {
            let ring = row_rect.shrink(tokens.space.s1 + tokens.stroke.focus);
            paint::focus_ring(&painter, theme, ring, tokens.radius.md);
        }
    }
    scroll_thumb(cx, area, memory.list_scroll, content);
}

/// The name of a column that sorts the list when operated, with a chevron for the order
/// when the list is sorted by it.
/// The name of a column in the header; one that sorts sends `browser.sort` when operated. The
/// colour column shows an empty square in place of its name. Returns whether it was given a
/// secondary click, which opens the menu of the columns.
fn heading(
    cx: &mut Cx<'_>,
    column: Rect,
    name: &str,
    sort: Option<SortColumn>,
    sorted: Option<(SortColumn, bool)>,
    square: bool,
) -> bool {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let label = |painter: &egui::Painter, ink| {
        if square {
            let place = Rect::from_min_size(
                egui::pos2(column.left(), column.center().y - 4.0),
                egui::vec2(8.0, 8.0),
            );
            painter.rect_stroke(
                place,
                CornerRadius::ZERO,
                paint::hairline(theme, ink),
                egui::StrokeKind::Inside,
            );
        } else {
            paint::text(
                painter,
                theme,
                column,
                Align2::LEFT_CENTER,
                name,
                tokens.text.label,
                ink,
            );
        }
    };
    let Some(sort) = sort else {
        label(painter, color.text_secondary);
        return false;
    };
    let width = if square {
        8.0
    } else {
        paint::text_width(painter, theme, name, tokens.text.label)
    };
    let order = sorted.and_then(|(by, descending)| (by == sort).then_some(descending));
    let reach = width + if order.is_some() { 18.0 } else { 0.0 };
    let rect = Rect::from_min_max(
        egui::pos2(column.left() - 4.0, column.top() + 3.0),
        egui::pos2(
            (column.left() + reach + 4.0).min(column.right()),
            column.bottom() - 3.0,
        ),
    );
    let command = AppCommand::BrowserSort { column: sort };
    cx.record(command.id(), rect, ControlKind::Button, true);
    let response = cx.ui.interact(
        rect,
        id(("browser-sort", sort.name())),
        cx.sense(true, Sense::click()),
    );
    response.widget_info(|| WidgetInfo::labeled(Role::ColumnHeader, true, name));
    if response.clicked() {
        cx.out.emit(command);
    }
    let ink = if order.is_some() || (response.hovered() && cx.interactive) {
        color.text_primary
    } else {
        color.text_secondary
    };
    let painter = cx.painter();
    label(painter, ink);
    if let Some(descending) = order {
        let chevron = Rect::from_center_size(
            egui::pos2(column.left() + width + 10.0, column.center().y),
            egui::vec2(12.0, 12.0),
        );
        let glyph = if descending {
            Glyph::ChevronDown
        } else {
            Glyph::ChevronUp
        };
        theme.icons.paint(painter, glyph, chevron, ink);
    }
    if response.has_focus() {
        paint::focus_ring(painter, theme, rect, tokens.radius.sm);
    }
    response.secondary_clicked()
}

/// What a click on the row at `index` does to the selection: Control adds or takes out the
/// row, Shift selects up to it, a plain click selects it alone.
fn click_row(cx: &mut Cx<'_>, browser: &BrowserView, response: &egui::Response, index: usize) {
    let modifiers = cx.ui.input(|input| input.modifiers);
    if modifiers.command && response.clicked() {
        cx.out.emit(AppCommand::BrowserSelectToggle { index });
    } else if modifiers.shift && response.clicked() {
        cx.out.emit(AppCommand::BrowserSelectExtend { index });
    } else if browser.selected != Some(index) || browser.marked.len() > 1 {
        // A drag from a selected row carries that row alone.
        cx.out.emit(AppCommand::BrowserSelect { index });
    }
}

/// The row Shift and an arrow extend the selection to: one past the end of the selection
/// away from the current row, which stays.
fn extended(browser: &BrowserView, delta: i32) -> Option<usize> {
    if delta == 0 {
        return None;
    }
    let current = browser.selected?;
    let first = browser.marked.first().copied().unwrap_or(current);
    let last = browser.marked.last().copied().unwrap_or(current);
    let far = if last > current { last } else { first };
    let to = far as i64 + i64::from(delta);
    (to >= 0 && (to as usize) < browser.rows.len()).then_some(to as usize)
}

/// Opens the menu of a row of the playlist shown: the right button selects the row first,
/// Shift+F10 opens it for the selected row.
fn row_menu(
    cx: &mut Cx<'_>,
    browser: &BrowserView,
    response: &egui::Response,
    pointed: Option<usize>,
    area: Rect,
    memory: &mut BrowserMemory,
) {
    // The rows of the collection and of a playlist have a menu; those of the explorer not.
    let library_shown = browser
        .collection
        .as_ref()
        .is_some_and(CollectionView::shown);
    if !library_shown {
        return;
    }
    if response.secondary_clicked()
        && let Some(index) = pointed
    {
        // A selected row keeps the selection; another row is selected alone.
        if !browser.marked.contains(&index) {
            cx.out.emit(AppCommand::BrowserSelect { index });
        }
        let at = response
            .interact_pointer_pos()
            .unwrap_or_else(|| area.left_top());
        memory.menu = Some(BrowserMenu {
            target: MenuTarget::Entry,
            at,
        });
    }
    if response.has_focus()
        && menu_key(cx)
        && let Some(selected) = browser.selected
    {
        let bottom = area.top() + (selected + 1) as f32 * ROW_HEIGHT - memory.list_scroll;
        memory.menu = Some(BrowserMenu {
            target: MenuTarget::Entry,
            at: egui::pos2(area.left() + 10.0, bottom.clamp(area.top(), area.bottom())),
        });
    }
}

/// The textures of the covers drawn, kept between frames by the context of egui, each with
/// the picture it was made from.
#[derive(Clone, Default)]
struct CoverTextures(
    std::collections::HashMap<PathBuf, (std::sync::Arc<[u8]>, egui::TextureHandle)>,
);

/// Textures kept; past that they are dropped and made again when drawn.
const MAX_TEXTURES: usize = 512;

/// The texture of the cover of the track at `path`, made once and kept with the others.
pub(crate) fn cover_texture(
    cx: &Cx<'_>,
    path: &std::path::Path,
    cover: &CoverImage,
) -> egui::TextureId {
    kept_texture(cx.ui.ctx(), path, cover)
}

/// The texture of `cover` for the track at `path`. A cover read again after one was written
/// into the file is another picture, so its texture is made again; the same picture keeps
/// its texture from frame to frame.
fn kept_texture(
    ctx: &egui::Context,
    path: &std::path::Path,
    cover: &CoverImage,
) -> egui::TextureId {
    let key = id("cover-textures");
    let kept = ctx.data_mut(|data| {
        let textures = data.get_temp_mut_or_default::<CoverTextures>(key);
        if textures.0.len() >= MAX_TEXTURES && !textures.0.contains_key(path) {
            textures.0.clear();
        }
        textures
            .0
            .get(path)
            .filter(|(rgba, _)| std::sync::Arc::ptr_eq(rgba, &cover.rgba))
            .map(|(_, texture)| texture.id())
    });
    if let Some(kept) = kept {
        return kept;
    }
    // Made outside `data_mut`: it holds the lock of the context, which making a texture
    // takes again, and egui's lock does not let one thread take it twice.
    let size = [cover.width as usize, cover.height as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &cover.rgba);
    let texture = ctx.load_texture(
        format!("cover:{}", path.display()),
        image,
        egui::TextureOptions::LINEAR,
    );
    let made = texture.id();
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<CoverTextures>(key).0.insert(
            path.to_path_buf(),
            (std::sync::Arc::clone(&cover.rgba), texture),
        );
    });
    made
}

/// The cover of a track in its cell, or the icon of an audio file while it has none.
fn cover_cell(
    cx: &Cx<'_>,
    painter: &egui::Painter,
    cell: Rect,
    path: &std::path::Path,
    cover: Option<&CoverImage>,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let square = Rect::from_center_size(cell.center(), egui::vec2(COVER, COVER));
    let texture = cover.map(|cover| cover_texture(cx, path, cover));
    match texture {
        Some(texture) => {
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture, square, uv, egui::Color32::WHITE);
        }
        None => {
            let icon = Rect::from_center_size(cell.center(), egui::vec2(12.0, 12.0));
            theme
                .icons
                .paint(painter, Glyph::FileAudio, icon, tokens.color.text_secondary);
        }
    }
}

/// What an empty list says, by what it shows and why it is empty.
fn empty_message(browser: &BrowserView) -> &'static str {
    let searching = !browser.search.trim().is_empty();
    match &browser.collection {
        _ if searching && !browser.reading => "browser.no_match",
        Some(collection) if collection.playlist.is_some() => match collection.available {
            _ if browser.reading => "browser.playlist_reading",
            Some(false) => "browser.library_unavailable",
            _ => "browser.playlist_empty",
        },
        Some(collection) if collection.history.is_some() => match collection.available {
            _ if browser.reading => "browser.history_reading",
            Some(false) => "browser.library_unavailable",
            _ => "browser.history_empty",
        },
        Some(collection) if collection.current => match collection.available {
            _ if browser.reading => "browser.collection_reading",
            Some(false) => "browser.library_unavailable",
            _ => "browser.collection_empty",
        },
        _ if browser.reading => "browser.reading",
        _ => "browser.empty",
    }
}

/// What every row of the list shares: how it writes keys, the key of the tempo master that the
/// compatible ones match, and the columns it shows.
#[derive(Clone, Copy)]
struct RowLook<'a> {
    notation: KeyNotation,
    reference: Option<u8>,
    columns: &'a [ListColumn],
}

impl<'a> RowLook<'a> {
    fn of(view: &View<'a>) -> Self {
        let engine = view.engine;
        let reference = engine
            .clock
            .master
            .and_then(|deck| engine.decks.get(deck.index()))
            .and_then(|deck| deck.resulting_key)
            .map(|transposed| keys::number_of(transposed.key));
        Self {
            notation: view.settings.key_notation,
            reference,
            columns: &view.browser.columns,
        }
    }
}

fn file_row(
    cx: &Cx<'_>,
    painter: &egui::Painter,
    rect: Rect,
    row: &FileRow,
    loaded: Option<DeckId>,
    cover: Option<&CoverImage>,
    look: RowLook<'_>,
) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let columns = Columns::of(rect, look.columns);
    let body = tokens.text.body;
    if let Some(deck) = loaded {
        let badge = Rect::from_center_size(
            egui::pos2(columns.badge.left() + 8.0, rect.center().y),
            egui::vec2(16.0, 16.0),
        );
        paint::badge(
            painter,
            theme,
            badge,
            deck.letter(),
            deck_color(theme, deck),
        );
    } else if let RowKind::Track(track) = &row.kind
        && track.prepared
    {
        // A track of the preparation list, unless a deck has it.
        let icon = Rect::from_center_size(
            egui::pos2(columns.badge.left() + 8.0, rect.center().y),
            egui::vec2(14.0, 14.0),
        );
        theme
            .icons
            .paint(painter, Glyph::Bookmark, icon, color.text_secondary);
    }
    if matches!(row.kind, RowKind::Track(_) | RowKind::Pending) {
        cover_cell(cx, painter, columns.cover, &row.path, cover);
    }
    match &row.kind {
        RowKind::Folder => {
            let icon = Rect::from_center_size(
                egui::pos2(columns.badge.left() + 8.0, rect.center().y),
                egui::vec2(14.0, 14.0),
            );
            theme
                .icons
                .paint(painter, Glyph::Folder, icon, color.text_secondary);
            let name = texts.format("browser.folder_name", &[("name", &row.name)]);
            paint::text_elided(
                painter,
                theme,
                columns.title,
                &name,
                body,
                color.text_primary,
            );
        }
        RowKind::Pending => {
            paint::text_elided(
                painter,
                theme,
                columns.title,
                &row.name,
                body,
                color.text_primary,
            );
        }
        RowKind::Unreadable => {
            let (title, flag) = columns.flag();
            paint::text_elided(painter, theme, title, &row.name, body, color.text_secondary);
            flagged(cx, painter, rect, flag, "browser.cant_read");
        }
        RowKind::Track(track) => {
            let title = track.title.as_deref().unwrap_or(&row.name);
            // A track played in this session: its title in the secondary colour and a check
            // after it.
            let (ink, room) = if track.played {
                (color.text_secondary, 14.0 + tokens.space.s2)
            } else {
                (color.text_primary, 0.0)
            };
            // The colour tag, a square in its column.
            if let Some(column) = columns.colour
                && let Some(tag) = usize::from(track.colour)
                    .checked_sub(1)
                    .and_then(|index| tokens.hotcue.get(index))
            {
                let square = Rect::from_min_size(
                    egui::pos2(column.left(), rect.center().y - 4.0),
                    egui::vec2(8.0, 8.0),
                );
                painter.rect_filled(square, CornerRadius::ZERO, *tag);
            }
            // A missing file with the format column hidden is flagged at the end of the title.
            let (title_cell, flag) = if track.missing {
                columns.flag()
            } else {
                (columns.title, columns.title)
            };
            let area = Rect::from_min_max(
                title_cell.min,
                egui::pos2(title_cell.right() - room, title_cell.bottom()),
            );
            paint::text_elided(painter, theme, area, title, body, ink);
            if track.played {
                let width = paint::text_width(painter, theme, title, body).min(area.width());
                let check = Rect::from_center_size(
                    egui::pos2(area.left() + width + tokens.space.s2 + 7.0, rect.center().y),
                    egui::vec2(14.0, 14.0),
                );
                theme
                    .icons
                    .paint(painter, Glyph::Check, check, color.text_secondary);
            }
            let secondary = |column: Option<Rect>, text: Option<&str>| {
                if let Some((column, text)) = column.zip(text) {
                    paint::text_elided(painter, theme, column, text, body, color.text_secondary);
                }
            };
            secondary(columns.artist, track.artist.as_deref());
            secondary(columns.album, track.album.as_deref());
            secondary(columns.genre, track.genre.as_deref());
            secondary(columns.comment, track.comment.as_deref());
            if let Some((column, bpm)) = columns.bpm.zip(track.bpm) {
                paint::text(
                    painter,
                    theme,
                    column,
                    Align2::LEFT_CENTER,
                    &format::bpm(bpm),
                    body,
                    color.text_primary,
                );
            }
            if let Some((column, key)) = columns.key.zip(track.key) {
                // A key that mixes well with what the tempo master plays, in its colour.
                let ink = if look
                    .reference
                    .is_some_and(|reference| keys::compatible(key, reference))
                {
                    color.key_match
                } else {
                    color.text_secondary
                };
                paint::text(
                    painter,
                    theme,
                    column,
                    Align2::LEFT_CENTER,
                    &keys::key_text(
                        key,
                        look.notation,
                        (
                            texts.get("browser.key_major"),
                            texts.get("browser.key_minor"),
                        ),
                    ),
                    body,
                    ink,
                );
            }
            if let Some((column, seconds)) = columns.time.zip(track.seconds) {
                paint::text(
                    painter,
                    theme,
                    column,
                    Align2::LEFT_CENTER,
                    &format::duration(seconds),
                    body,
                    color.text_secondary,
                );
            }
            if let Some((column, rating)) = columns.rating.zip(track.rating) {
                for star in 0..5u8 {
                    let place = Rect::from_min_size(
                        egui::pos2(
                            column.left() + f32::from(star) * 14.0,
                            rect.center().y - 6.0,
                        ),
                        egui::vec2(12.0, 12.0),
                    );
                    if star < rating {
                        theme
                            .icons
                            .paint_filled(painter, Glyph::Star, place, color.text_secondary);
                    } else {
                        theme
                            .icons
                            .paint(painter, Glyph::Star, place, color.text_secondary);
                    }
                }
            }
            if track.missing {
                flagged(cx, painter, rect, flag, "browser.missing");
                return;
            }
            let Some(file_column) = columns.file else {
                return;
            };
            let rate = format::kilohertz(track.sample_rate);
            let file = match (track.format.is_empty(), track.sample_rate) {
                (false, _) => texts.format(
                    "browser.file_value",
                    &[("format", &track.format), ("rate", &rate)],
                ),
                (true, 0) => String::new(),
                (true, _) => texts.format("browser.rate_value", &[("rate", &rate)]),
            };
            paint::text_elided(
                painter,
                theme,
                file_column,
                &file,
                body,
                color.text_secondary,
            );
        }
    }
}

/// A red dot and a short text in the file column, for a file that cannot be played.
fn flagged(cx: &Cx<'_>, painter: &egui::Painter, rect: Rect, column: Rect, text: &str) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let dot = egui::pos2(column.left() + 4.0, rect.center().y);
    painter.circle_filled(dot, 4.0, tokens.color.status_error);
    let label = Rect::from_min_max(
        egui::pos2(dot.x + 4.0 + tokens.space.s2 + 2.0, rect.top()),
        column.max,
    );
    paint::text_elided(
        painter,
        theme,
        label,
        cx.texts.get(text),
        tokens.text.body,
        tokens.color.text_secondary,
    );
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::{clamp_scroll, kept_texture};
    use crate::model::CoverImage;

    /// Behaviour note: `docs/behavior/cover-art.md`, *Importing and deleting a cover*.
    #[test]
    fn a_cover_read_again_gets_a_new_texture_and_the_same_picture_keeps_its_own() {
        let ctx = egui::Context::default();
        let path = Path::new("track.flac");
        let cover = |red: u8| CoverImage {
            width: 1,
            height: 1,
            rgba: Arc::from(vec![red, 0, 0, 255]),
        };
        let first = cover(255);
        let made = kept_texture(&ctx, path, &first);
        assert_eq!(kept_texture(&ctx, path, &first.clone()), made);
        // The same path with a picture read again, as after an import.
        assert_ne!(kept_texture(&ctx, path, &cover(0)), made);
    }

    #[test]
    fn scrolling_stops_at_both_ends() {
        assert_eq!(clamp_scroll(-5.0, 100.0, 40.0), 0.0);
        assert_eq!(clamp_scroll(30.0, 100.0, 40.0), 30.0);
        assert_eq!(clamp_scroll(90.0, 100.0, 40.0), 60.0);
        // Content shorter than the area does not scroll.
        assert_eq!(clamp_scroll(10.0, 20.0, 40.0), 0.0);
    }
}
