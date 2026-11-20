//! The mix recorder in the middle panel of the effect row, and its indicator in the status
//! header.
//!
//! Components: *Effect row* and `StatusHeader` in `docs/design/DESIGN.md`; placement measured on
//! `docs/design/screens/M4-effects.html`. Behaviour note: `docs/behavior/recording.md`.

use egui::{Align2, Rect};
use platter_engine::{Command, RecorderSnapshot};

use crate::cx::{Cx, id};
use crate::format;
use crate::paint;
use crate::widgets::key::{self, KeySpec, Press};

const PADDING: f32 = 12.0;
const REC: egui::Vec2 = egui::vec2(48.0, 28.0);
const TIME_WIDTH: f32 = 47.0;
const CUT: egui::Vec2 = egui::vec2(39.0, 28.0);
const GAP: f32 = 8.0;
const LABEL_HEIGHT: f32 = 14.0;
/// Between the label and the row under it.
const LABEL_GAP: f32 = 4.0;

/// The recorder, right-aligned in `rect`: its label over Rec, the time recorded and Cut.
/// Returns the rectangle it takes, for the loop recorder to its left.
pub fn panel(cx: &mut Cx<'_>, rect: Rect, recorder: &RecorderSnapshot) -> Rect {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let texts = cx.texts;
    let width = REC.x + GAP + TIME_WIDTH + GAP + CUT.x;
    let left = rect.right() - PADDING - width;
    let top = rect.center().y - (LABEL_HEIGHT + LABEL_GAP + REC.y) / 2.0;
    paint::text(
        cx.painter(),
        theme,
        Rect::from_min_size(egui::pos2(left, top), egui::vec2(width, LABEL_HEIGHT)),
        Align2::LEFT_CENTER,
        texts.get("recorder.title"),
        tokens.text.label,
        color.text_secondary,
    );
    let row = top + LABEL_HEIGHT + LABEL_GAP;

    let rec = Rect::from_min_size(egui::pos2(left, row), REC);
    let name = match (&recorder.failure, recorder.recording) {
        (_, true) => texts.get("recorder.stop_name").to_owned(),
        (Some(reason), false) => texts.format("recorder.failed_name", &[("reason", reason)]),
        (None, false) => texts.get("recorder.record_name").to_owned(),
    };
    let mut spec = KeySpec::text(texts.get("recorder.rec"));
    spec.name = &name;
    spec.lit = recorder.recording.then_some(color.status_error);
    key::key(
        cx,
        rec,
        id("recorder-rec"),
        spec,
        Press::Click(Command::RecorderToggle.into()),
    );

    let time = Rect::from_min_size(
        egui::pos2(rec.right() + GAP, row),
        egui::vec2(TIME_WIDTH, REC.y),
    );
    // A problem shows in the time until the next recording starts.
    let ink = if recorder.failure.is_some() || recorder.dropped_frames > 0 {
        color.status_warn
    } else if recorder.recording {
        color.text_primary
    } else {
        color.text_secondary
    };
    paint::text(
        cx.painter(),
        theme,
        time,
        Align2::LEFT_CENTER,
        &format::hours(recorder.seconds),
        tokens.text.readout,
        ink,
    );

    let cut = Rect::from_min_size(egui::pos2(time.right() + GAP, row), CUT);
    let mut spec = KeySpec::text(texts.get("recorder.cut")).enabled(recorder.recording);
    spec.name = texts.get("recorder.cut_name");
    key::key(
        cx,
        cut,
        id("recorder-cut"),
        spec,
        Press::Click(Command::RecorderCut.into()),
    );
    Rect::from_min_max(egui::pos2(left, top), egui::pos2(cut.right(), cut.bottom()))
}

/// The indicator of the status header while a recording runs: a dot and the label. The time
/// is left to the panel, since the header has little room (`docs/behavior/recording.md`,
/// *Indicator*). Returns where the next indicator starts.
pub fn indicator(cx: &Cx<'_>, x: f32, header: Rect) -> f32 {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let middle = header.center().y;
    painter.circle_filled(egui::pos2(x + 4.0, middle), 4.0, color.status_error);
    let line = Rect::from_min_max(
        egui::pos2(x + 8.0 + tokens.space.s3, header.top()),
        header.max,
    );
    paint::text(
        painter,
        theme,
        line,
        Align2::LEFT_CENTER,
        cx.t("recorder.indicator"),
        tokens.text.label,
        color.text_primary,
    )
    .right()
}
