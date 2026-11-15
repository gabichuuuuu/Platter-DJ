//! The file selector along the bottom of the window: a tree of folders on the left and the
//! files of the chosen folder on the right.
//!
//! Component: `TrackList` in `docs/design/DESIGN.md`; placement from
//! `docs/design/screens/M2-main.html`. Behaviour note: `docs/behavior/browser.md`.

use std::path::PathBuf;

use egui::{Align2, CornerRadius, EventFilter, Key, Rect, Role, Sense, WidgetInfo};
use platter_core::DeckId;

use crate::command::{AppCommand, BrowserNode, SortColumn};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::icons::Glyph;
use crate::model::{BrowserView, CollectionView, FileRow, ImportOutcome, RowKind, TreeRow, View};
use crate::paint;
use crate::panels::waveform::deck_color;
use crate::widgets::field;
use crate::widgets::key::{self, Face, KeySpec, Press};

pub const TREE_WIDTH: f32 = 260.0;
const TREE_ROW: f32 = 26.0;
const TREE_INDENT: f32 = 16.0;
const TOOLBAR_HEIGHT: f32 = 44.0;
const SEARCH_WIDTH: f32 = 240.0;
const SEARCH_HEIGHT: f32 = 28.0;
pub const ROW_HEIGHT: f32 = 27.0;
const COLUMN_GAP: f32 = 12.0;

/// The columns of the file list after the title, which takes the width that is left.
const ARTIST: f32 = 232.0;
const ALBUM: f32 = 232.0;
const BPM: f32 = 64.0;
const TIME: f32 = 56.0;
const FILE: f32 = 110.0;
const BADGE: f32 = 22.0;

/// Where each column of the file list starts and ends.
struct Columns {
    badge: Rect,
    title: Rect,
    artist: Rect,
    album: Rect,
    bpm: Rect,
    time: Rect,
    file: Rect,
}

impl Columns {
    fn of(row: Rect) -> Self {
        let left = row.left() + 10.0;
        let right = row.right() - 10.0;
        let fixed = BADGE + ARTIST + ALBUM + BPM + TIME + FILE + 6.0 * COLUMN_GAP;
        let title = (right - left - fixed).max(80.0);
        let mut x = left;
        let mut next = |width: f32| {
            let rect = Rect::from_min_max(
                egui::pos2(x, row.top()),
                egui::pos2(x + width, row.bottom()),
            );
            x += width + COLUMN_GAP;
            rect
        };
        Self {
            badge: next(BADGE),
            title: next(title),
            artist: next(ARTIST),
            album: next(ALBUM),
            bpm: next(BPM),
            time: next(TIME),
            file: next(FILE),
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
        list_rect.max,
    );
    list(cx, rows, view, memory);
}

fn tree(cx: &mut Cx<'_>, rect: Rect, browser: &BrowserView, memory: &mut BrowserMemory) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    cx.painter()
        .rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    let area = rect.shrink(tokens.space.s3);
    // The collection, when there is one, takes the first place above the folders.
    let fixed = usize::from(browser.collection.is_some());
    let content = (fixed + browser.tree.len()) as f32 * (TREE_ROW + 1.0);
    let hover = cx.ui.interact(rect, id("browser-tree"), Sense::hover());
    if hover.hovered() && cx.interactive {
        memory.tree_scroll -= cx.ui.input(|input| input.smooth_scroll_delta.y);
    }
    memory.tree_scroll = clamp_scroll(memory.tree_scroll, content, area.height());
    let painter = cx.painter().with_clip_rect(area);
    let slot = |place: usize| {
        let top = area.top() + place as f32 * (TREE_ROW + 1.0) - memory.tree_scroll;
        let row_rect = Rect::from_min_size(
            egui::pos2(area.left(), top),
            egui::vec2(area.width(), TREE_ROW),
        );
        // A row cut by the edge of the area is drawn and cannot be operated.
        (row_rect, area.contains_rect(row_rect))
    };
    if let Some(collection) = &browser.collection {
        let (row_rect, whole) = slot(0);
        if area.intersects(row_rect) {
            collection_row(cx, &painter, row_rect, collection, whole);
        }
    }
    for (index, row) in browser.tree.iter().enumerate() {
        let (row_rect, whole) = slot(fixed + index);
        if area.intersects(row_rect) {
            tree_row(cx, &painter, row_rect, index, row, whole);
        }
    }
    scroll_thumb(cx, area, memory.tree_scroll, content);
}

/// The collection node: no chevron, its icon, its name and the number of its tracks.
fn collection_row(
    cx: &mut Cx<'_>,
    painter: &egui::Painter,
    rect: Rect,
    collection: &CollectionView,
    whole: bool,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let action = AppCommand::BrowserNodeSelect {
        node: BrowserNode::Collection,
    };
    if whole {
        cx.record(action.id(), rect, ControlKind::Button, true);
    }
    let name = cx.texts.get("browser.collection");
    let response = cx
        .ui
        .interact(rect, id("tree-collection"), cx.sense(whole, Sense::click()));
    response.widget_info(|| WidgetInfo::selected(Role::TreeItem, true, collection.current, name));
    if collection.current {
        painter.rect_filled(
            rect,
            paint::radius(tokens.radius.md),
            color.surface_selected,
        );
    } else if response.hovered() && cx.interactive {
        painter.rect_filled(rect, paint::radius(tokens.radius.md), color.surface_hover);
    }
    if response.has_focus() {
        cx.out.claim(&[Key::Space, Key::Enter]);
    }
    if response.clicked() {
        cx.out.emit(action);
    }
    let middle = rect.center().y;
    // Lined up with the folder icons of the roots, after the room of their chevron.
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + tokens.space.s3 + 12.0 + 6.0 + 7.0, middle),
        egui::vec2(14.0, 14.0),
    );
    theme
        .icons
        .paint(painter, Glyph::FileAudio, icon, color.text_secondary);
    let count = collection.tracks.map(|tracks| tracks.to_string());
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
        name,
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
        let label = texts.get("browser.import");
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
            enabled: collection.available == Some(true) && !collection.importing,
            keeps_led: false,
            text: None,
        };
        key::key(
            cx,
            key_rect,
            id("browser-import"),
            spec,
            Press::Click(
                AppCommand::LibraryImportFolder {
                    path: folder.clone(),
                }
                .into(),
            ),
        );
        right = key_rect.left() - tokens.space.s4;
    }
    for (deck, glyph, label) in [
        (DeckId::B, Glyph::LoadRight, "browser.load_b"),
        (DeckId::A, Glyph::LoadLeft, "browser.load_a"),
    ] {
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
    let showing_collection = collection.is_some_and(|collection| collection.current);
    let crumbs = if showing_collection {
        vec![texts.get("browser.collection").to_owned()]
    } else {
        view.browser.crumbs.clone()
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
    let mut count = match collection.filter(|collection| collection.current) {
        Some(collection) => match (collection.tracks, collection.matched) {
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
    let status = collection.and_then(|collection| match collection.imported {
        _ if collection.importing => Some(texts.get("browser.importing").to_owned()),
        Some(ImportOutcome::Done {
            added,
            known,
            unreadable,
        }) => Some(texts.format(
            "browser.imported",
            &[
                ("added", &added.to_string()),
                ("known", &known.to_string()),
                ("unreadable", &unreadable.to_string()),
            ],
        )),
        Some(ImportOutcome::Failed) => Some(texts.get("browser.import_failed").to_owned()),
        None => None,
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

    let header = Rect::from_min_size(rect.min, egui::vec2(rect.width(), ROW_HEIGHT));
    let columns = Columns::of(header);
    let headings = [
        (columns.title, "browser.title", Some(SortColumn::Title)),
        (columns.artist, "browser.artist", Some(SortColumn::Artist)),
        (columns.album, "browser.album", Some(SortColumn::Album)),
        (columns.bpm, "browser.bpm", Some(SortColumn::Bpm)),
        (columns.time, "browser.time", Some(SortColumn::Time)),
        (columns.file, "browser.file", None),
    ];
    for (column, name, sort) in headings {
        match sort {
            Some(sort) => heading(cx, column, texts.get(name), sort, browser.sort),
            None => {
                paint::text(
                    painter,
                    theme,
                    column,
                    Align2::LEFT_CENTER,
                    texts.get(name),
                    tokens.text.label,
                    color.text_secondary,
                );
            }
        }
    }
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
    for command in ["browser.move", "browser.activate", "browser.up"] {
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
            if let Some(index) = pointed
                && browser.selected != Some(index)
            {
                cx.out.emit(AppCommand::BrowserSelect { index });
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
        if delta != 0 {
            cx.out.emit(AppCommand::BrowserMove { delta });
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
        let selected = browser.selected == Some(index);
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
        file_row(cx, &painter, row_rect, row, loaded_in(view, row));
        if selected && response.has_focus() {
            let ring = row_rect.shrink(tokens.space.s1 + tokens.stroke.focus);
            paint::focus_ring(&painter, theme, ring, tokens.radius.md);
        }
    }
    scroll_thumb(cx, area, memory.list_scroll, content);
}

/// The name of a column that sorts the list when operated, with a chevron for the order
/// when the list is sorted by it.
fn heading(
    cx: &mut Cx<'_>,
    column: Rect,
    name: &str,
    sort: SortColumn,
    sorted: Option<(SortColumn, bool)>,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let width = paint::text_width(painter, theme, name, tokens.text.label);
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
    paint::text(
        painter,
        theme,
        column,
        Align2::LEFT_CENTER,
        name,
        tokens.text.label,
        ink,
    );
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
}

/// What an empty list says, by what it shows and why it is empty.
fn empty_message(browser: &BrowserView) -> &'static str {
    let searching = !browser.search.trim().is_empty();
    match &browser.collection {
        _ if searching && !browser.reading => "browser.no_match",
        Some(collection) if collection.current => match collection.available {
            _ if browser.reading => "browser.collection_reading",
            Some(false) => "browser.library_unavailable",
            _ => "browser.collection_empty",
        },
        _ if browser.reading => "browser.reading",
        _ => "browser.empty",
    }
}

fn file_row(
    cx: &Cx<'_>,
    painter: &egui::Painter,
    rect: Rect,
    row: &FileRow,
    loaded: Option<DeckId>,
) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let columns = Columns::of(rect);
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
            paint::text_elided(
                painter,
                theme,
                columns.title,
                &row.name,
                body,
                color.text_secondary,
            );
            flagged(cx, painter, rect, columns.file, "browser.cant_read");
        }
        RowKind::Track(track) => {
            let title = track.title.as_deref().unwrap_or(&row.name);
            paint::text_elided(
                painter,
                theme,
                columns.title,
                title,
                body,
                color.text_primary,
            );
            let secondary = |column: Rect, text: Option<&str>| {
                if let Some(text) = text {
                    paint::text_elided(painter, theme, column, text, body, color.text_secondary);
                }
            };
            secondary(columns.artist, track.artist.as_deref());
            secondary(columns.album, track.album.as_deref());
            if let Some(bpm) = track.bpm {
                paint::text(
                    painter,
                    theme,
                    columns.bpm,
                    Align2::LEFT_CENTER,
                    &format::bpm(bpm),
                    body,
                    color.text_primary,
                );
            }
            if let Some(seconds) = track.seconds {
                paint::text(
                    painter,
                    theme,
                    columns.time,
                    Align2::LEFT_CENTER,
                    &format::duration(seconds),
                    body,
                    color.text_secondary,
                );
            }
            if track.missing {
                flagged(cx, painter, rect, columns.file, "browser.missing");
                return;
            }
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
                columns.file,
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
    use super::clamp_scroll;

    #[test]
    fn scrolling_stops_at_both_ends() {
        assert_eq!(clamp_scroll(-5.0, 100.0, 40.0), 0.0);
        assert_eq!(clamp_scroll(30.0, 100.0, 40.0), 30.0);
        assert_eq!(clamp_scroll(90.0, 100.0, 40.0), 60.0);
        // Content shorter than the area does not scroll.
        assert_eq!(clamp_scroll(10.0, 20.0, 40.0), 0.0);
    }
}
