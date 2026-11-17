//! The dialog of the maintenance of the collection: how many tracks are missing, which ones,
//! and the keys that check, relocate and remove them.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Collection maintenance*.

use egui::{Align2, CornerRadius, Rect, StrokeKind};

use crate::command::{Action, AppCommand};
use crate::cx::{Cx, id};
use crate::model::{MaintenanceOutcome, View};
use crate::paint;
use crate::widgets::key::{self, KeySpec, Press};

/// Size of the dialog, as the dialog of the track details of the design system.
pub const MAINTENANCE_SIZE: egui::Vec2 = egui::vec2(760.0, 500.0);
const ROW_HEIGHT: f32 = 22.0;
const FOOTER_HEIGHT: f32 = 56.0;

/// Draws the dialog in `rect`. `confirm` is set while *Remove missing* waits for its answer.
pub fn maintenance(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>, confirm: bool) {
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
    let Some(collection) = view.browser.collection.as_ref() else {
        return;
    };
    let inner = rect.shrink2(egui::vec2(20.0, 16.0));
    let mut top = inner.top();
    let line = |top: f32, height: f32| {
        Rect::from_min_size(
            egui::pos2(inner.left(), top),
            egui::vec2(inner.width(), height),
        )
    };
    paint::text(
        painter,
        theme,
        line(top, 26.0),
        Align2::LEFT_CENTER,
        texts.get("maintenance.title"),
        tokens.text.title,
        color.text_primary,
    );
    top += 34.0;
    let count = |value: Option<usize>| value.map_or_else(|| "–".to_owned(), |n| n.to_string());
    let summary = texts.format(
        "maintenance.summary",
        &[
            ("tracks", &count(collection.tracks)),
            ("missing", &count(collection.missing_total)),
        ],
    );
    paint::text_elided(
        painter,
        theme,
        line(top, 18.0),
        &summary,
        tokens.text.body,
        color.text_primary,
    );
    top += 22.0;
    let outcome = if collection.maintaining {
        Some(texts.get("maintenance.running").to_owned())
    } else {
        collection.maintained.map(|outcome| match outcome {
            MaintenanceOutcome::Checked { missing, found } => texts.format(
                "maintenance.checked",
                &[
                    ("missing", &missing.to_string()),
                    ("found", &found.to_string()),
                ],
            ),
            MaintenanceOutcome::Relocated {
                relocated,
                not_found,
                ambiguous,
            } => texts.format(
                "maintenance.relocated",
                &[
                    ("relocated", &relocated.to_string()),
                    ("not_found", &not_found.to_string()),
                    ("ambiguous", &ambiguous.to_string()),
                ],
            ),
            MaintenanceOutcome::Removed(removed) => {
                texts.format("maintenance.removed", &[("removed", &removed.to_string())])
            }
            MaintenanceOutcome::Failed => texts.get("maintenance.failed").to_owned(),
        })
    };
    if let Some(outcome) = outcome {
        paint::text_elided(
            painter,
            theme,
            line(top, 18.0),
            &outcome,
            tokens.text.body,
            color.text_secondary,
        );
    }
    top += 30.0;

    // The missing tracks, as many as fit, and how many more there are.
    let footer_top = rect.bottom() - FOOTER_HEIGHT;
    let list = Rect::from_min_max(
        egui::pos2(inner.left(), top),
        egui::pos2(inner.right(), footer_top - 8.0),
    );
    painter.rect_filled(list, paint::radius(tokens.radius.md), color.bg_app);
    let list = list.shrink2(egui::vec2(10.0, 6.0));
    let fits = (list.height() / ROW_HEIGHT).floor().max(1.0) as usize;
    let total = collection.missing_total.unwrap_or(0);
    let shown = collection.missing.len().min(fits);
    // The last line says what does not fit.
    let shown = if total > shown && shown == fits {
        fits.saturating_sub(1)
    } else {
        shown
    };
    for (index, path) in collection.missing.iter().take(shown).enumerate() {
        paint::text_elided(
            painter,
            theme,
            Rect::from_min_size(
                list.min + egui::vec2(0.0, index as f32 * ROW_HEIGHT),
                egui::vec2(list.width(), ROW_HEIGHT),
            ),
            path,
            tokens.text.body,
            color.text_primary,
        );
    }
    let rest = total.saturating_sub(shown);
    if total == 0 {
        paint::text_elided(
            painter,
            theme,
            Rect::from_min_size(list.min, egui::vec2(list.width(), ROW_HEIGHT)),
            texts.get("maintenance.none_missing"),
            tokens.text.body,
            color.text_secondary,
        );
    } else if rest > 0 {
        paint::text_elided(
            painter,
            theme,
            Rect::from_min_size(
                list.min + egui::vec2(0.0, shown as f32 * ROW_HEIGHT),
                egui::vec2(list.width(), ROW_HEIGHT),
            ),
            &texts.format("maintenance.more", &[("count", &rest.to_string())]),
            tokens.text.body,
            color.text_secondary,
        );
    }

    footer(cx, rect, view, confirm);
}

/// The keys at the foot of the dialog, from the right: Close, then the actions.
fn footer(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>, confirm: bool) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let Some(collection) = view.browser.collection.as_ref() else {
        return;
    };
    let idle = !collection.maintaining && collection.available == Some(true);
    let missing = collection.missing_total.unwrap_or(0);
    let folder = view.browser.explorer.as_ref();
    let folder_name = folder.map(|path| {
        path.file_name()
            .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
            .into_owned()
    });
    let relocate = texts.format(
        "maintenance.relocate",
        &[("folder", folder_name.as_deref().unwrap_or("–"))],
    );
    let remove_count = texts.format(
        "maintenance.remove_count",
        &[("count", &missing.to_string())],
    );
    let mut keys: Vec<(&str, bool, Action)> = vec![(
        texts.get("maintenance.close"),
        true,
        AppCommand::LibraryMaintenanceClose.into(),
    )];
    if confirm {
        keys.push((
            texts.get("maintenance.keep"),
            true,
            AppCommand::LibraryMissingKeep.into(),
        ));
        keys.push((
            &remove_count,
            idle && missing > 0,
            AppCommand::LibraryMissingRemove.into(),
        ));
    } else {
        keys.push((
            texts.get("maintenance.remove"),
            idle && missing > 0,
            AppCommand::LibraryMissingAsk.into(),
        ));
        keys.push((
            &relocate,
            idle && missing > 0 && folder.is_some(),
            AppCommand::LibraryRelocate {
                path: folder.cloned().unwrap_or_default(),
            }
            .into(),
        ));
        keys.push((
            texts.get("maintenance.check"),
            idle,
            AppCommand::LibraryCheck.into(),
        ));
    }
    let middle = rect.bottom() - FOOTER_HEIGHT / 2.0;
    let mut right = rect.right() - 20.0;
    for (index, (label, enabled, action)) in keys.into_iter().enumerate() {
        let width = key::width_for(cx, label, 88.0);
        let key_rect = Rect::from_min_size(
            egui::pos2(right - width, middle - tokens.size.button_h / 2.0),
            egui::vec2(width, tokens.size.button_h),
        );
        right = key_rect.left() - tokens.space.s3;
        let mut spec = KeySpec::text(label);
        spec.enabled = enabled;
        key::key(
            cx,
            key_rect,
            id(("maintenance-key", index)),
            spec,
            Press::Click(action),
        );
    }
}
