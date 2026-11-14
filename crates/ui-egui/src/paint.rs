//! Drawing helpers shared by the widgets and the panels: text in the styles of the design
//! system, readouts whose digits do not move, the focus ring and the deck badge.

use std::sync::Arc;

use egui::epaint::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, CornerRadius, Galley, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

use crate::theme::Theme;
use crate::tokens::TextStyle;

/// A corner radius from a token. Tokens are whole pixels far below the limit of the type.
pub fn radius(token: f32) -> CornerRadius {
    CornerRadius::same(token.clamp(0.0, 255.0).round() as u8)
}

pub fn hairline(theme: &Theme, color: Color32) -> Stroke {
    Stroke::new(theme.tokens.stroke.hairline, color)
}

fn format(theme: &Theme, style: TextStyle, color: Color32) -> TextFormat {
    TextFormat {
        font_id: theme.font(style),
        line_height: Some(style.line_height),
        color,
        ..TextFormat::default()
    }
}

/// A laid-out text and the width it occupies, which for a readout includes the padding
/// after its last digit.
pub struct Laid {
    galley: Arc<Galley>,
    pub size: Vec2,
}

/// Lays out one line of text. In a tabular style every digit sits centred in a slot as
/// wide as the widest digit, so a value that changes keeps its width: egui cannot switch on
/// the tabular figures of the font, and `docs/design/DESIGN.md` names this as the fallback.
pub fn lay(painter: &Painter, theme: &Theme, text: &str, style: TextStyle, color: Color32) -> Laid {
    let format = format(theme, style, color);
    if !style.tabular || !text.bytes().any(|byte| byte.is_ascii_digit()) {
        let job = LayoutJob::single_section(text.to_owned(), format);
        let galley = painter.layout_job(job);
        let size = Vec2::new(galley.size().x, style.line_height);
        return Laid { galley, size };
    }
    let font = theme.font(style);
    let widths: Vec<f32> = painter.fonts_mut(|fonts| {
        ('0'..='9')
            .map(|digit| fonts.glyph_width(&font, digit))
            .collect()
    });
    let slot = widths.iter().copied().fold(0.0f32, f32::max);
    let mut job = LayoutJob::default();
    // Padding owed by the previous digit, placed before whatever follows it.
    let mut owed = 0.0;
    let mut buffer = [0u8; 4];
    for character in text.chars() {
        let pad = character
            .to_digit(10)
            .map_or(0.0, |digit| (slot - widths[digit as usize]) / 2.0);
        job.append(
            character.encode_utf8(&mut buffer),
            owed + pad,
            format.clone(),
        );
        owed = pad;
    }
    let galley = painter.layout_job(job);
    let size = Vec2::new(galley.size().x + owed, style.line_height);
    Laid { galley, size }
}

impl Laid {
    /// Paints the text with the top left corner of its line box at `pos`.
    pub fn paint(self, painter: &Painter, pos: Pos2) -> Rect {
        let rect = Rect::from_min_size(pos, self.size);
        // The galley may be taller or shorter than the line box of the style by a fraction;
        // centring it keeps the baseline where the design puts it.
        let offset = (self.size.y - self.galley.size().y) / 2.0;
        painter.galley(
            pos + Vec2::new(0.0, offset),
            self.galley,
            Color32::PLACEHOLDER,
        );
        rect
    }
}

/// Paints one line of text anchored inside `rect` and returns the rectangle it covers.
pub fn text(
    painter: &Painter,
    theme: &Theme,
    rect: Rect,
    anchor: Align2,
    text: &str,
    style: TextStyle,
    color: Color32,
) -> Rect {
    let laid = lay(painter, theme, text, style, color);
    let target = anchor.align_size_within_rect(laid.size, rect);
    laid.paint(painter, target.min)
}

/// Paints one line of text at the left of `rect`, cut with an ellipsis when it is wider.
/// Returns whether it was cut, so the caller can offer the full text some other way.
pub fn text_elided(
    painter: &Painter,
    theme: &Theme,
    rect: Rect,
    text: &str,
    style: TextStyle,
    color: Color32,
) -> bool {
    let mut job = LayoutJob::single_section(text.to_owned(), format(theme, style, color));
    job.wrap = TextWrapping {
        max_width: rect.width(),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = painter.layout_job(job);
    let elided = galley.elided;
    let offset = (rect.height() - galley.size().y) / 2.0;
    painter.galley(
        rect.min + Vec2::new(0.0, offset),
        galley,
        Color32::PLACEHOLDER,
    );
    elided
}

/// Paints text from the top left of `rect`, broken into as many lines as its width needs.
/// Lines that do not fit the height of `rect` are left out and the last one shown ends in
/// an ellipsis, so a long translation never shows half a line. Returns the height the
/// lines take.
pub fn text_wrapped(
    painter: &Painter,
    theme: &Theme,
    rect: Rect,
    text: &str,
    style: TextStyle,
    color: Color32,
) -> f32 {
    let mut job = LayoutJob::single_section(text.to_owned(), format(theme, style, color));
    job.wrap.max_width = rect.width();
    let mut galley = painter.layout_job(job.clone());
    let rows = galley.rows.len();
    if galley.size().y > rect.height() && rows > 0 {
        let row_height = galley.size().y / rows as f32;
        job.wrap.max_rows = ((rect.height() / row_height).floor() as usize).max(1);
        galley = painter.layout_job(job);
    }
    let height = galley.size().y;
    painter.galley(rect.min, galley, Color32::PLACEHOLDER);
    height
}

/// Width of one line of text in a style.
pub fn text_width(painter: &Painter, theme: &Theme, text: &str, style: TextStyle) -> f32 {
    lay(painter, theme, text, style, Color32::PLACEHOLDER)
        .size
        .x
}

/// The keyboard focus ring: drawn outside the control, with a gap, over everything else.
pub fn focus_ring(painter: &Painter, theme: &Theme, rect: Rect, corner: f32) {
    let gap = theme.tokens.space.s1;
    painter.rect_stroke(
        rect.expand(gap),
        radius(corner + gap),
        Stroke::new(theme.tokens.stroke.focus, theme.tokens.color.focus_ring),
        StrokeKind::Outside,
    );
}

/// The letter of a deck on its colour.
pub fn badge(painter: &Painter, theme: &Theme, rect: Rect, letter: char, color: Color32) {
    painter.rect_filled(rect, radius(theme.tokens.radius.sm), color);
    let mut style = theme.tokens.text.label;
    style.weight = crate::tokens::Weight::Bold;
    let mut buffer = [0u8; 4];
    text(
        painter,
        theme,
        rect,
        Align2::CENTER_CENTER,
        letter.encode_utf8(&mut buffer),
        style,
        theme.tokens.color.text_on_accent,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_text_never_runs_past_the_height_it_is_given() {
        let theme = Theme::load().unwrap();
        let ctx = egui::Context::default();
        theme.install(&ctx);
        let style = theme.tokens.text.body;
        let text = "Arrastra una pista aquí, o selecciona una en el navegador y pulsa Mayús + ←";
        let mut heights = (0.0, 0.0, 0.0);
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let painter = ui.painter();
                let row = text_wrapped(
                    painter,
                    &theme,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(160.0, 1000.0)),
                    "Ag",
                    style,
                    Color32::WHITE,
                );
                let all = text_wrapped(
                    painter,
                    &theme,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(160.0, 1000.0)),
                    text,
                    style,
                    Color32::WHITE,
                );
                let two_rows = Rect::from_min_size(Pos2::ZERO, Vec2::new(160.0, row * 2.5));
                heights = (
                    row,
                    all,
                    text_wrapped(painter, &theme, two_rows, text, style, Color32::WHITE),
                );
            });
            output.textures_delta.clear();
        }
        let (row, all, fitted) = heights;
        assert!(
            all > row * 2.5,
            "the text should need more than two rows here"
        );
        assert!(fitted <= row * 2.5, "{fitted} > {}", row * 2.5);
        assert!(fitted >= row * 2.0, "two whole rows should still be shown");
    }
}
