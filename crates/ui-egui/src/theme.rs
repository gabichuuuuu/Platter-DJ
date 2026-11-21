//! The look of the interface: tokens, icons and fonts, and how they are given to egui.

use crate::command::WaveColour;
use std::fmt;
use std::sync::Arc;

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, Visuals};

use crate::icons::{IconError, Icons, Mark};
use crate::tokens::{TextStyle, TokenError, Tokens, Weight};

/// The font files of the design system: Geist, one static file per weight.
fn font_file(weight: Weight) -> &'static [u8] {
    match weight {
        Weight::Regular => include_bytes!("../../../docs/design/fonts/Geist-Regular.ttf"),
        Weight::Medium => include_bytes!("../../../docs/design/fonts/Geist-Medium.ttf"),
        Weight::SemiBold => include_bytes!("../../../docs/design/fonts/Geist-SemiBold.ttf"),
        Weight::Bold => include_bytes!("../../../docs/design/fonts/Geist-Bold.ttf"),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeError {
    Tokens(TokenError),
    Icons(IconError),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tokens(error) => error.fmt(f),
            Self::Icons(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ThemeError {}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub tokens: Tokens,
    pub icons: Icons,
    pub mark: Mark,
    /// The colour mode of the waveforms, set by the shell from the settings at each frame.
    pub wave_colour: WaveColour,
}

impl Theme {
    /// Reads the design system compiled into the binary.
    pub fn load() -> Result<Self, ThemeError> {
        Ok(Self {
            tokens: Tokens::load().map_err(ThemeError::Tokens)?,
            icons: Icons::load().map_err(ThemeError::Icons)?,
            mark: Mark::load().map_err(ThemeError::Icons)?,
            wave_colour: WaveColour::default(),
        })
    }

    /// Gives egui the fonts and the colours. Call it once, before the first frame.
    pub fn install(&self, ctx: &egui::Context) {
        let mut fonts = FontDefinitions::empty();
        for weight in Weight::ALL {
            let name = weight.family().to_owned();
            fonts.font_data.insert(
                name.clone(),
                Arc::new(FontData::from_static(font_file(weight))),
            );
            fonts
                .families
                .insert(FontFamily::Name(weight.family().into()), vec![name]);
        }
        // Anything egui draws on its own account uses the regular weight too.
        let regular = vec![Weight::Regular.family().to_owned()];
        fonts
            .families
            .insert(FontFamily::Proportional, regular.clone());
        fonts.families.insert(FontFamily::Monospace, regular);
        ctx.set_fonts(fonts);
        ctx.set_visuals(self.visuals());
    }

    /// The mapping of `docs/design/DESIGN.md` from tokens to egui. The panels paint with
    /// the tokens directly; this covers what egui paints itself, such as scroll bars.
    fn visuals(&self) -> Visuals {
        let color = &self.tokens.color;
        let mut visuals = Visuals::dark();
        visuals.override_text_color = Some(color.text_primary);
        visuals.window_fill = color.bg_app;
        visuals.panel_fill = color.bg_app;
        visuals.extreme_bg_color = color.bg_app;
        visuals.faint_bg_color = color.surface_panel;
        visuals.window_stroke = Stroke::new(self.tokens.stroke.hairline, color.line_key);
        visuals.selection.bg_fill = color.surface_selected;
        visuals.selection.stroke = Stroke::new(self.tokens.stroke.hairline, color.text_primary);
        visuals.hyperlink_color = color.text_primary;
        visuals.window_shadow = egui::Shadow::NONE;
        visuals.popup_shadow = egui::Shadow::NONE;
        let base = visuals.widgets.inactive;
        let widget = |fill: Color32, outline: Color32| {
            let mut widget = base;
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = Stroke::new(self.tokens.stroke.hairline, outline);
            widget.fg_stroke = Stroke::new(self.tokens.stroke.hairline, color.text_primary);
            widget
        };
        visuals.widgets.noninteractive = widget(color.surface_panel, color.line_divider);
        visuals.widgets.inactive = widget(color.surface_control, color.line_key);
        visuals.widgets.hovered = widget(color.surface_hover, color.line_key);
        visuals.widgets.active = widget(color.surface_pressed, color.line_key);
        visuals.widgets.open = widget(color.surface_control, color.line_control);
        visuals
    }

    /// The font of a text style.
    pub fn font(&self, style: TextStyle) -> FontId {
        FontId::new(style.size, FontFamily::Name(style.weight.family().into()))
    }
}
