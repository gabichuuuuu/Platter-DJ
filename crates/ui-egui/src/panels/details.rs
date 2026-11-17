//! The dialog of the details of a track: what the library knows of it, to edit, for one track
//! or a selection of tracks.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Track details*. The layout follows
//! `docs/design/screens/Track-details.html`.

use std::path::PathBuf;

use egui::{Align2, CornerRadius, Rect, StrokeKind};

use crate::command::{Action, AppCommand, DetailField, DetailFields, DetailsThen, TrackDetails};
use crate::cx::{Cx, id};
use crate::icons::Glyph;
use crate::paint;
use crate::widgets::choice::{self, Choice};
use crate::widgets::field;
use crate::widgets::key::{self, KeySpec, Press};

/// Size of the dialog, as the reference screen of the design system.
pub const DETAILS_SIZE: egui::Vec2 = egui::vec2(760.0, 500.0);
const LABEL_WIDTH: f32 = 120.0;
const FIELD_HEIGHT: f32 = 28.0;
const ROW_GAP: f32 = 10.0;
const FOOTER_HEIGHT: f32 = 56.0;
/// The tick key before the name of a field, for several tracks.
const TICK_SIDE: f32 = 22.0;
const TICK_GAP: f32 = 8.0;

/// What the dialog edits: the fields of a track, or of a selection, as they are typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DetailsForm {
    /// The name of the file and the folder that holds it, as the dialog names one track.
    pub file: String,
    pub folder: String,
    pub details: TrackDetails,
    /// The values the dialog opened with.
    pub opened: TrackDetails,
    /// How many tracks the dialog is for, and which it shows: 0 all of them, 1 and up one.
    pub count: usize,
    pub position: usize,
    /// The fields the tracks shown shared when the dialog opened, and those ticked.
    pub shared: DetailFields,
    pub ticked: DetailFields,
    /// The selected tracks the dialog opened on.
    pub tracks: Vec<PathBuf>,
}

impl DetailsForm {
    /// The dialog shows several tracks at once, with a tick key on each field.
    pub fn several(&self) -> bool {
        self.count > 1 && self.position == 0
    }

    /// The fields a save writes: for several tracks those ticked that changed or did not
    /// share a value, for one track those that changed.
    pub fn to_write(&self) -> DetailFields {
        let mut fields = DetailFields::NONE;
        for field in DetailField::ALL {
            let changed = !same_field(&self.details, &self.opened, field);
            let write = if self.several() {
                self.ticked.contains(field) && (changed || !self.shared.contains(field))
            } else {
                changed
            };
            if write {
                fields.insert(field);
            }
        }
        fields
    }

    /// The tracks a save at `position` is for.
    pub fn tracks_at(&self, position: usize) -> Vec<PathBuf> {
        match position {
            0 => self.tracks.clone(),
            at => self.tracks.get(at - 1).cloned().into_iter().collect(),
        }
    }
}

/// What a save of the dialog wrote, for the tracks it was for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailsEdit {
    pub tracks: Vec<PathBuf>,
    pub details: TrackDetails,
    pub fields: DetailFields,
}

/// Whether two sets of details hold the same value of `field`.
pub fn same_field(a: &TrackDetails, b: &TrackDetails, field: DetailField) -> bool {
    match field {
        DetailField::Title => a.title == b.title,
        DetailField::Artist => a.artist == b.artist,
        DetailField::Album => a.album == b.album,
        DetailField::Genre => a.genre == b.genre,
        DetailField::Comment => a.comment == b.comment,
        DetailField::Rating => a.rating == b.rating,
    }
}

/// Copies the value of `field` from `from` into `to`.
pub fn copy_field(to: &mut TrackDetails, from: &TrackDetails, field: DetailField) {
    match field {
        DetailField::Title => to.title.clone_from(&from.title),
        DetailField::Artist => to.artist.clone_from(&from.artist),
        DetailField::Album => to.album.clone_from(&from.album),
        DetailField::Genre => to.genre.clone_from(&from.genre),
        DetailField::Comment => to.comment.clone_from(&from.comment),
        DetailField::Rating => to.rating = from.rating,
    }
}

/// Draws the dialog in `rect` for `form`, whose fields take what is typed.
pub fn details(cx: &mut Cx<'_>, rect: Rect, form: &mut DetailsForm) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect(
        rect.expand(1.0),
        CornerRadius::ZERO,
        color.surface_panel,
        paint::hairline(theme, color.line_key),
        StrokeKind::Outside,
    );
    let inner = rect.shrink2(egui::vec2(20.0, 16.0));
    let line = |top: f32, height: f32| {
        Rect::from_min_size(
            egui::pos2(inner.left(), top),
            egui::vec2(inner.width(), height),
        )
    };
    let several = form.several();
    let count = form.count.to_string();
    let (name, place) = if several {
        (
            texts.format("details.several", &[("count", &count)]),
            texts.get("details.where_several").to_owned(),
        )
    } else {
        (form.file.clone(), form.folder.clone())
    };
    let mut top = inner.top();
    paint::text(
        painter,
        theme,
        line(top, 26.0),
        Align2::LEFT_CENTER,
        texts.get("details.title"),
        tokens.text.title,
        color.text_primary,
    );
    top += 32.0;
    paint::text_elided(
        painter,
        theme,
        line(top, 18.0),
        &name,
        tokens.text.body,
        color.text_primary,
    );
    top += 20.0;
    paint::text_elided(
        painter,
        theme,
        line(top, 18.0),
        &place,
        tokens.text.body,
        color.text_secondary,
    );
    top += 32.0;

    let DetailsForm {
        details,
        shared,
        ticked,
        position,
        ..
    } = form;
    let fields: [(&str, &str, DetailField, &mut String); 5] = [
        (
            "details.field_title",
            "details-title",
            DetailField::Title,
            &mut details.title,
        ),
        (
            "details.field_artist",
            "details-artist",
            DetailField::Artist,
            &mut details.artist,
        ),
        (
            "details.field_album",
            "details-album",
            DetailField::Album,
            &mut details.album,
        ),
        (
            "details.field_genre",
            "details-genre",
            DetailField::Genre,
            &mut details.genre,
        ),
        (
            "details.field_comment",
            "details-comment",
            DetailField::Comment,
            &mut details.comment,
        ),
    ];
    for (label, key, field, text) in fields {
        let row = line(top, FIELD_HEIGHT);
        let name = name_of_field(cx, row, label, several.then_some((field, *ticked)));
        let input = Rect::from_min_max(egui::pos2(name.right(), row.top()), row.max);
        let before = text.clone();
        field::text(
            cx,
            input,
            id(key),
            text,
            texts.get(label),
            "browser.track.details.save",
        );
        // Typing into a field of several tracks ticks it.
        if *text != before {
            ticked.insert(field);
        }
        top += FIELD_HEIGHT + ROW_GAP;
    }

    let row = line(top, FIELD_HEIGHT);
    let name = name_of_field(
        cx,
        row,
        "details.field_rating",
        several.then_some((DetailField::Rating, *ticked)),
    );
    let labels: [&str; 6] = [texts.get("browser.rating_0"), "1", "2", "3", "4", "5"];
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let control = Rect::from_min_max(egui::pos2(name.right(), row.top()), row.max);
    // Ratings that differ show no choice until one is made.
    let unknown =
        several && !shared.contains(DetailField::Rating) && !ticked.contains(DetailField::Rating);
    choice::segmented(
        cx,
        control,
        id("details-rating"),
        &choices,
        (!unknown).then_some(usize::from(details.rating)),
        |index| {
            Action::App(AppCommand::BrowserTrackDetailsRating {
                rating: u8::try_from(index).unwrap_or(0),
            })
        },
    );
    let position = *position;
    footer(cx, rect, form, position);
}

/// Draws the name of a field in the left of `row` and returns where it ends. For several
/// tracks a tick key comes before it, lit while the field is ticked.
fn name_of_field(
    cx: &mut Cx<'_>,
    row: Rect,
    label: &str,
    tick: Option<(DetailField, DetailFields)>,
) -> Rect {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let mut left = row.left();
    if let Some((field, ticked)) = tick {
        let on = ticked.contains(field);
        let key_rect = Rect::from_min_size(
            egui::pos2(left, row.center().y - TICK_SIDE / 2.0),
            egui::vec2(TICK_SIDE, TICK_SIDE),
        );
        let state = if on {
            "details.ticked"
        } else {
            "details.unticked"
        };
        let name = texts.format(state, &[("field", texts.get(label))]);
        let mut spec = if on {
            KeySpec::icon(Glyph::Check, 14.0, &name)
        } else {
            KeySpec::text("")
        };
        spec.name = &name;
        spec.lit = on.then_some(tokens.color.text_primary);
        key::key(
            cx,
            key_rect,
            id(("details-tick", field.name())),
            spec,
            Press::Click(Action::App(AppCommand::BrowserTrackDetailsTick { field })),
        );
        left = key_rect.right() + TICK_GAP;
    }
    let name = Rect::from_min_max(
        egui::pos2(left, row.top()),
        // The fields start after the names, and after the tick keys when there are some.
        egui::pos2(left + LABEL_WIDTH, row.bottom()),
    );
    paint::text(
        cx.painter(),
        theme,
        name,
        Align2::LEFT_CENTER,
        texts.get(label),
        tokens.text.body,
        tokens.color.text_secondary,
    );
    name
}

/// The foot: *Previous*, the position and *Next* for several tracks, or a line on where the
/// fields go; *Cancel* and *Save* on the right.
fn footer(cx: &mut Cx<'_>, rect: Rect, form: &DetailsForm, position: usize) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let footer_top = rect.bottom() - FOOTER_HEIGHT;
    let middle = footer_top + FOOTER_HEIGHT / 2.0;
    let save = |then: DetailsThen| {
        Action::App(AppCommand::BrowserTrackDetailsSave {
            details: form.details.clone(),
            fields: form.to_write(),
            position,
            then,
        })
    };
    let key_rect = |left: f32, width: f32| {
        Rect::from_min_size(
            egui::pos2(left, middle - tokens.size.button_h / 2.0),
            egui::vec2(width, tokens.size.button_h),
        )
    };
    let mut left = rect.left() + 20.0;
    if form.count > 1 {
        let count = form.count.to_string();
        let at = position.to_string();
        let shown = if position == 0 {
            texts.format("details.position_all", &[("count", &count)])
        } else {
            texts.format("details.position", &[("at", &at), ("count", &count)])
        };
        let previous = texts.get("details.previous");
        let width = key::width_for(cx, previous, 88.0);
        key::key(
            cx,
            key_rect(left, width),
            id("details-previous"),
            KeySpec::text(previous).enabled(position > 0),
            Press::Click(save(DetailsThen::Previous)),
        );
        left += width + tokens.space.s3;
        let label = Rect::from_min_size(egui::pos2(left, middle - 9.0), egui::vec2(72.0, 18.0));
        paint::text(
            cx.painter(),
            theme,
            label,
            Align2::CENTER_CENTER,
            &shown,
            tokens.text.body,
            tokens.color.text_secondary,
        );
        left = label.right() + tokens.space.s3;
        let next = texts.get("details.next");
        let width = key::width_for(cx, next, 88.0);
        key::key(
            cx,
            key_rect(left, width),
            id("details-next"),
            KeySpec::text(next).enabled(position < form.count),
            Press::Click(save(DetailsThen::Next)),
        );
    } else {
        paint::text_elided(
            cx.painter(),
            theme,
            Rect::from_min_max(
                egui::pos2(left, middle - 9.0),
                egui::pos2(rect.right() - 260.0, middle + 9.0),
            ),
            texts.get("details.where"),
            tokens.text.body,
            tokens.color.text_secondary,
        );
    }
    let mut right = rect.right() - 20.0;
    let keys = [
        (texts.get("details.save"), save(DetailsThen::Close)),
        (
            texts.get("details.cancel"),
            Action::App(AppCommand::BrowserTrackDetailsClose),
        ),
    ];
    for (index, (label, action)) in keys.into_iter().enumerate() {
        let width = key::width_for(cx, label, 88.0);
        let at = key_rect(right - width, width);
        right = at.left() - tokens.space.s3;
        key::key(
            cx,
            at,
            id(("details-key", index)),
            KeySpec::text(label),
            Press::Click(action),
        );
    }
}
