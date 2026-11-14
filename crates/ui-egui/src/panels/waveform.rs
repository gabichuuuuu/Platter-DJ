//! The waveforms: one scrolling lane per deck under the header, and the overview of the
//! whole track inside each deck.
//!
//! Component: `Waveform` in `docs/design/DESIGN.md`. Behaviour note:
//! `docs/behavior/waveform.md`. Both views draw the three-band peaks the engine computed
//! when the track loaded; nothing here looks at audio.

use egui::{Color32, CornerRadius, Mesh, Pos2, Rect, Role, Sense, Shape, Stroke, WidgetInfo};
use platter_core::DeckId;
use platter_engine::{Command, DeckSnapshot, WaveformPeaks};

use crate::command::AppCommand;
use crate::cx::{ControlKind, Cx, id};
use crate::icons::Glyph;
use crate::paint;
use crate::theme::Theme;
use crate::widgets::key::{self, KeySpec, Press};

/// Seconds of track across a scrolling lane at each zoom step, from closest to widest.
pub const ZOOM_SECONDS: [f64; 5] = [4.0, 8.0, 16.0, 32.0, 64.0];
/// The zoom step a deck starts with.
pub const DEFAULT_ZOOM: usize = 1;
/// How much of the background shows through the veil over the played part of an overview.
const PLAYED_VEIL: f32 = 0.55;
const GUTTER_WIDTH: f32 = 32.0;

/// The low, mid and high colour of a deck, darkest first.
pub fn band_colors(theme: &Theme, deck: DeckId) -> [Color32; 3] {
    let color = &theme.tokens.color;
    match deck {
        DeckId::A => [color.deck_a_low, color.deck_a, color.deck_a_high],
        DeckId::B => [color.deck_b_low, color.deck_b, color.deck_b_high],
    }
}

pub fn deck_color(theme: &Theme, deck: DeckId) -> Color32 {
    band_colors(theme, deck)[1]
}

/// Adds one column of the waveform to `mesh`: three nested bars mirrored around the centre
/// line, the low band outermost.
fn add_column(
    mesh: &mut Mesh,
    lane: Rect,
    left: f32,
    right: f32,
    peaks: [u8; 3],
    colors: [Color32; 3],
) {
    let half = lane.height() / 2.0;
    let middle = lane.center().y;
    for (peak, color) in peaks.into_iter().zip(colors) {
        if peak == 0 {
            continue;
        }
        let reach = half * f32::from(peak) / 255.0;
        let bar = Rect::from_min_max(
            Pos2::new(left.max(lane.left()), middle - reach),
            Pos2::new(right.min(lane.right()), middle + reach),
        );
        if bar.width() > 0.0 {
            mesh.add_colored_rect(bar, color);
        }
    }
}

/// The highest peak of each band over a range of columns.
fn peak_over(columns: &[[u8; 3]], from: usize, to: usize) -> [u8; 3] {
    let to = to.min(columns.len());
    if from >= to {
        return [0; 3];
    }
    columns[from..to].iter().fold([0; 3], |peak, column| {
        [
            peak[0].max(column[0]),
            peak[1].max(column[1]),
            peak[2].max(column[2]),
        ]
    })
}

/// The line that marks the play position, with a small triangle at each end.
fn playhead(cx: &Cx<'_>, lane: Rect, x: f32) {
    let tokens = &cx.theme.tokens;
    let ink = tokens.color.playhead;
    let painter = cx.painter();
    painter.vline(x, lane.y_range(), Stroke::new(tokens.stroke.playhead, ink));
    let size = 4.0;
    for (edge, direction) in [(lane.top(), 1.0), (lane.bottom(), -1.0)] {
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(x - size, edge),
                Pos2::new(x + size, edge),
                Pos2::new(x, edge + direction * size),
            ],
            ink,
            Stroke::NONE,
        ));
    }
}

/// Draws the scrolling waveform of a deck with the play position at the centre of `lane`.
fn scrolling(cx: &Cx<'_>, lane: Rect, deck: DeckId, snapshot: &DeckSnapshot, zoom: usize) {
    let color = &cx.theme.tokens.color;
    let painter = cx.painter().with_clip_rect(lane);
    painter.rect_filled(lane, CornerRadius::ZERO, color.bg_app);
    let Some(peaks) = snapshot
        .peaks
        .as_deref()
        .filter(|_| snapshot.track.is_some())
    else {
        // An empty lane keeps its centre line, so the zone reads as a waveform lane.
        painter.hline(
            lane.x_range(),
            lane.center().y,
            paint::hairline(cx.theme, color.line_divider),
        );
        return;
    };
    let window = ZOOM_SECONDS[zoom.min(ZOOM_SECONDS.len() - 1)];
    let pixels_per_second = f64::from(lane.width()) / window;
    let columns_per_pixel = 1.0 / (pixels_per_second * peaks.seconds_per_column());
    let position = snapshot.position_seconds;
    let centre = f64::from(lane.center().x);
    // Bars sit on a grid fixed to the track, not to the screen: as the track moves, every
    // bar keeps its content and only its place changes, so the picture does not flicker.
    let first = ((position - window / 2.0) * pixels_per_second).floor() as i64;
    let last = ((position + window / 2.0) * pixels_per_second).ceil() as i64;
    let colors = super::waveform::band_colors(cx.theme, deck);
    let mut mesh = Mesh::default();
    for bar in first.max(0)..last {
        let from = (bar as f64 * columns_per_pixel).floor() as usize;
        if from >= peaks.columns().len() {
            break;
        }
        let to = (((bar + 1) as f64 * columns_per_pixel).ceil() as usize).max(from + 1);
        let left = centre + (bar as f64 - position * pixels_per_second);
        add_column(
            &mut mesh,
            lane,
            left as f32,
            left as f32 + 1.0,
            peak_over(peaks.columns(), from, to),
            colors,
        );
    }
    painter.add(mesh);
    playhead(cx, lane, lane.center().x);
}

/// The two scrolling lanes of the main window: for each deck a gutter with its badge and
/// zoom keys, and its lane.
pub fn lanes(cx: &mut Cx<'_>, rect: Rect, decks: &[DeckSnapshot; 2], zoom: [usize; 2]) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let seam = tokens.space.s1;
    let lane_height = (rect.height() - seam) / 2.0;
    for deck in DeckId::ALL {
        let top = rect.top() + deck.index() as f32 * (lane_height + seam);
        let row = Rect::from_min_size(
            egui::pos2(rect.left(), top),
            egui::vec2(rect.width(), lane_height),
        );
        let gutter = Rect::from_min_size(row.min, egui::vec2(GUTTER_WIDTH, lane_height));
        let lane = Rect::from_min_max(egui::pos2(gutter.right() + seam, row.top()), row.max);
        let painter = cx.painter();
        painter.rect_filled(gutter, CornerRadius::ZERO, color.surface_panel);
        let badge = Rect::from_min_size(gutter.min + egui::vec2(7.0, 6.0), egui::vec2(18.0, 18.0));
        paint::badge(
            painter,
            theme,
            badge,
            deck.letter(),
            deck_color(theme, deck),
        );
        let letter = deck.letter().to_string();
        let zoom_step = zoom[deck.index()];
        let keys = [
            (
                Glyph::Plus,
                "deck.zoom_in",
                AppCommand::WaveformZoomIn { deck },
                zoom_step > 0,
            ),
            (
                Glyph::Minus,
                "deck.zoom_out",
                AppCommand::WaveformZoomOut { deck },
                zoom_step + 1 < ZOOM_SECONDS.len(),
            ),
        ];
        for (index, (glyph, name, command, enabled)) in keys.into_iter().enumerate() {
            let key_rect = Rect::from_min_size(
                gutter.min + egui::vec2(5.0, 28.0 + 22.0 * index as f32),
                egui::vec2(22.0, 18.0),
            );
            let name = texts.format(name, &[("deck", &letter)]);
            key::key(
                cx,
                key_rect,
                id(("lane-zoom", deck.index(), index)),
                KeySpec::icon(glyph, 12.0, &name).enabled(enabled),
                Press::Click(command.into()),
            );
        }
        scrolling(cx, lane, deck, &decks[deck.index()], zoom_step);
        let name = texts.format("deck.lane", &[("deck", &letter)]);
        let response = cx
            .ui
            .interact(lane, id(("lane", deck.index())), Sense::hover());
        response.widget_info(|| WidgetInfo::labeled(Role::Image, true, &name));
    }
}

/// Draws the overview of a deck in `rect` and emits `deck.seek` when it is clicked.
pub fn overview(cx: &mut Cx<'_>, rect: Rect, deck: DeckId, snapshot: &DeckSnapshot) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect_filled(rect, paint::radius(tokens.radius.sm), color.bg_app);
    let name = cx
        .texts
        .format("deck.overview", &[("deck", &deck.letter().to_string())]);
    let track = snapshot
        .track
        .as_ref()
        .filter(|track| track.seconds > 0.0 && !snapshot.loading);
    let enabled = track.is_some();
    // The command is recorded with the position it would seek to at the centre.
    let seek = |fraction: f64| Command::DeckSeek {
        deck,
        seconds: track.map_or(0.0, |track| track.seconds * fraction.clamp(0.0, 1.0)),
    };
    cx.record(seek(0.5).id(), rect, ControlKind::Area, enabled);
    let response = cx.ui.interact(
        rect,
        id(("overview", deck.index())),
        cx.sense(enabled, Sense::click_and_drag()),
    );
    response.widget_info(|| WidgetInfo::labeled(Role::Slider, enabled, &name));
    let Some(track) = track else {
        return;
    };
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        cx.out
            .emit(seek(f64::from((pointer.x - rect.left()) / rect.width())));
    }
    if let Some(peaks) = snapshot.peaks.as_deref() {
        paint_overview(cx, rect, deck, peaks);
    }
    let played = (snapshot.position_seconds / track.seconds).clamp(0.0, 1.0) as f32;
    let x = rect.left() + played * rect.width();
    let veil = Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom()));
    painter.rect_filled(
        veil,
        CornerRadius::ZERO,
        color.bg_app.gamma_multiply(PLAYED_VEIL),
    );
    playhead(cx, rect, x.clamp(rect.left() + 1.0, rect.right() - 1.0));
    if response.has_focus() {
        paint::focus_ring(painter, theme, rect, tokens.radius.sm);
    }
}

fn paint_overview(cx: &Cx<'_>, rect: Rect, deck: DeckId, peaks: &WaveformPeaks) {
    let columns = peaks.overview();
    if columns.is_empty() {
        return;
    }
    let colors = band_colors(cx.theme, deck);
    let width = rect.width().floor().max(1.0) as usize;
    let mut mesh = Mesh::default();
    for pixel in 0..width {
        let from = pixel * columns.len() / width;
        let to = ((pixel + 1) * columns.len() / width).max(from + 1);
        let left = rect.left() + pixel as f32;
        add_column(
            &mut mesh,
            rect,
            left,
            left + 1.0,
            peak_over(columns, from, to),
            colors,
        );
    }
    cx.painter().add(mesh);
}

#[cfg(test)]
mod tests {
    use super::peak_over;

    #[test]
    fn the_peak_of_a_range_is_the_highest_of_each_band() {
        let columns = [[1, 9, 0], [5, 2, 0], [3, 3, 7]];
        assert_eq!(peak_over(&columns, 0, 3), [5, 9, 7]);
        assert_eq!(peak_over(&columns, 1, 2), [5, 2, 0]);
        // A range past the end, or an empty one, is silence.
        assert_eq!(peak_over(&columns, 2, 99), [3, 3, 7]);
        assert_eq!(peak_over(&columns, 3, 9), [0; 3]);
        assert_eq!(peak_over(&columns, 2, 1), [0; 3]);
    }
}
