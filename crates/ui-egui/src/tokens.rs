//! The design tokens: every colour, size and text style of the interface.
//!
//! They are read from `docs/design/tokens.toml`, which is compiled into the binary. That
//! file is the single source: no panel names a colour or a size of its own, and a test
//! checks that every colour a panel paints is in the table.

use std::collections::BTreeMap;
use std::fmt;

use egui::Color32;

const SOURCE: &str = include_str!("../../../docs/design/tokens.toml");

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenError {
    /// The file is not valid TOML.
    Syntax(String),
    Missing(String),
    /// A value has the wrong type or cannot be read as a colour or a size.
    Invalid(String),
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(message) => write!(f, "the design tokens cannot be read: {message}"),
            Self::Missing(name) => write!(f, "the design token `{name}` is missing"),
            Self::Invalid(name) => write!(f, "the design token `{name}` has an invalid value"),
        }
    }
}

impl std::error::Error for TokenError {}

/// Weight of a text style. Each one is a font file of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl Weight {
    pub const ALL: [Self; 4] = [Self::Regular, Self::Medium, Self::SemiBold, Self::Bold];

    fn from_number(weight: i64) -> Option<Self> {
        match weight {
            400 => Some(Self::Regular),
            500 => Some(Self::Medium),
            600 => Some(Self::SemiBold),
            700 => Some(Self::Bold),
            _ => None,
        }
    }

    /// Name of the font family that holds this weight.
    pub fn family(self) -> &'static str {
        match self {
            Self::Regular => "geist-regular",
            Self::Medium => "geist-medium",
            Self::SemiBold => "geist-semibold",
            Self::Bold => "geist-bold",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub line_height: f32,
    pub weight: Weight,
    /// Digits keep a fixed width, so a value that changes does not move.
    pub tabular: bool,
}

/// Declares a group of tokens as a struct and reads each field from its name in the file.
macro_rules! token_group {
    ($(#[$meta:meta])* $group:ident: $kind:ty, $read:ident { $($field:ident => $name:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $group {
            $(pub $field: $kind),+
        }

        impl $group {
            fn read(source: &Source<'_>) -> Result<Self, TokenError> {
                Ok(Self { $($field: source.$read($name)?),+ })
            }
        }
    };
}

token_group! {
    /// The colours the panels use so far. The file holds more, for later milestones.
    Palette: Color32, color {
        bg_app => "bg-app",
        surface_panel => "surface-panel",
        surface_control => "surface-control",
        surface_hover => "surface-hover",
        surface_pressed => "surface-pressed",
        surface_selected => "surface-selected",
        line_divider => "line-divider",
        line_key => "line-key",
        line_control => "line-control",
        text_primary => "text-primary",
        text_secondary => "text-secondary",
        text_disabled => "text-disabled",
        text_on_accent => "text-on-accent",
        focus_ring => "focus-ring",
        playhead => "playhead",
        deck_a => "deck-a",
        deck_a_low => "deck-a-low",
        deck_a_high => "deck-a-high",
        deck_b => "deck-b",
        deck_b_low => "deck-b-low",
        deck_b_high => "deck-b-high",
        status_ok => "status-ok",
        status_warn => "status-warn",
        status_error => "status-error",
        meter_ok => "meter-ok",
        meter_warn => "meter-warn",
        meter_clip => "meter-clip",
        grid_beat => "grid-beat",
        grid_downbeat => "grid-downbeat",
        loop_region => "loop-region",
    }
}

token_group! {
    Spacing: f32, spacing {
        s1 => "space-1",
        s2 => "space-2",
        s3 => "space-3",
        s4 => "space-4",
        s5 => "space-5",
        s6 => "space-6",
    }
}

token_group! {
    Radius: f32, radius {
        sm => "radius-sm",
        md => "radius-md",
        lg => "radius-lg",
    }
}

token_group! {
    Strokes: f32, stroke {
        hairline => "stroke-hairline",
        focus => "stroke-focus",
        playhead => "stroke-playhead",
        led => "stroke-led",
    }
}

token_group! {
    Sizes: f32, size {
        button_h => "size-button-h",
        transport_h => "size-transport-h",
        button_transport => "size-button-transport",
        knob => "size-knob",
        fader_track => "size-fader-track",
        crossfader_track => "size-crossfader-track",
        meter_w => "size-meter-w",
    }
}

token_group! {
    TextStyles: TextStyle, text {
        title => "title",
        body => "body",
        button => "button",
        label => "label",
        time => "time",
        tempo => "tempo",
        readout => "readout",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    pub color: Palette,
    pub space: Spacing,
    pub radius: Radius,
    pub stroke: Strokes,
    pub size: Sizes,
    pub text: TextStyles,
    /// The colours of the eight hotcue slots, in their order.
    pub hotcue: [Color32; 8],
    /// Every colour of the file by its name, including those no panel uses yet.
    pub all_colors: BTreeMap<String, Color32>,
}

struct Source<'a> {
    root: &'a toml::Table,
}

impl Source<'_> {
    fn table(&self, name: &str) -> Result<&toml::Table, TokenError> {
        self.root
            .get(name)
            .and_then(toml::Value::as_table)
            .ok_or_else(|| TokenError::Missing(format!("[{name}]")))
    }

    fn number(&self, table: &str, name: &str) -> Result<f32, TokenError> {
        let value = self
            .table(table)?
            .get(name)
            .ok_or_else(|| TokenError::Missing(name.to_owned()))?;
        let number = match value {
            toml::Value::Integer(number) => *number as f32,
            toml::Value::Float(number) => *number as f32,
            _ => return Err(TokenError::Invalid(name.to_owned())),
        };
        if number.is_finite() && (0.0..=4_096.0).contains(&number) {
            Ok(number)
        } else {
            Err(TokenError::Invalid(name.to_owned()))
        }
    }

    fn spacing(&self, name: &str) -> Result<f32, TokenError> {
        self.number("spacing", name)
    }

    fn radius(&self, name: &str) -> Result<f32, TokenError> {
        self.number("radius", name)
    }

    fn stroke(&self, name: &str) -> Result<f32, TokenError> {
        self.number("stroke", name)
    }

    fn size(&self, name: &str) -> Result<f32, TokenError> {
        self.number("size", name)
    }

    fn color(&self, name: &str) -> Result<Color32, TokenError> {
        let colors = self.table("color")?;
        resolve_color(colors, name, 0)
    }

    fn text(&self, name: &str) -> Result<TextStyle, TokenError> {
        let invalid = || TokenError::Invalid(format!("text.{name}"));
        let style = self
            .table("text")?
            .get(name)
            .and_then(toml::Value::as_table)
            .ok_or_else(|| TokenError::Missing(format!("text.{name}")))?;
        let number = |field: &str| {
            style
                .get(field)
                .and_then(toml::Value::as_integer)
                .filter(|number| (1..=1_000).contains(number))
                .ok_or_else(invalid)
        };
        Ok(TextStyle {
            size: number("size")? as f32,
            line_height: number("line_height")? as f32,
            weight: Weight::from_number(number("weight")?).ok_or_else(invalid)?,
            tabular: style
                .get("tabular")
                .and_then(toml::Value::as_bool)
                .ok_or_else(invalid)?,
        })
    }
}

/// Aliases may point at aliases; this bounds the chain so a loop in the file is an error.
const MAX_ALIAS_DEPTH: usize = 4;

fn resolve_color(colors: &toml::Table, name: &str, depth: usize) -> Result<Color32, TokenError> {
    let text = colors
        .get(name)
        .ok_or_else(|| TokenError::Missing(name.to_owned()))?
        .as_str()
        .ok_or_else(|| TokenError::Invalid(name.to_owned()))?;
    if let Some(alias) = text
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    {
        if depth >= MAX_ALIAS_DEPTH {
            return Err(TokenError::Invalid(name.to_owned()));
        }
        return resolve_color(colors, alias, depth + 1);
    }
    parse_color(text).ok_or_else(|| TokenError::Invalid(name.to_owned()))
}

/// Reads `#rrggbb` or `rgba(r, g, b, a)` with `a` from 0 to 1.
fn parse_color(text: &str) -> Option<Color32> {
    if let Some(hex) = text.strip_prefix('#') {
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16).ok();
        return Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?));
    }
    let inner = text.strip_prefix("rgba(")?.strip_suffix(')')?;
    let mut parts = inner.split(',').map(str::trim);
    let mut channel = || parts.next()?.parse::<u8>().ok();
    let (red, green, blue) = (channel()?, channel()?, channel()?);
    let alpha: f32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(0.0..=1.0).contains(&alpha) {
        return None;
    }
    Some(Color32::from_rgba_unmultiplied(
        red,
        green,
        blue,
        (alpha * 255.0).round() as u8,
    ))
}

impl Tokens {
    /// Reads the tokens compiled into the binary.
    pub fn load() -> Result<Self, TokenError> {
        Self::parse(SOURCE)
    }

    fn parse(source: &str) -> Result<Self, TokenError> {
        let root: toml::Table = source
            .parse()
            .map_err(|error: toml::de::Error| TokenError::Syntax(error.message().to_owned()))?;
        let source = Source { root: &root };
        let colors = source.table("color")?;
        let all_colors = colors
            .keys()
            .map(|name| Ok((name.clone(), resolve_color(colors, name, 0)?)))
            .collect::<Result<_, TokenError>>()?;
        let color = Palette::read(&source)?;
        let mut hotcue = [Color32::TRANSPARENT; 8];
        for (index, slot) in hotcue.iter_mut().enumerate() {
            *slot = resolve_color(colors, &format!("hotcue-{}", index + 1), 0)?;
        }
        Ok(Self {
            hotcue,
            color,
            space: Spacing::read(&source)?,
            radius: Radius::read(&source)?,
            stroke: Strokes::read(&source)?,
            size: Sizes::read(&source)?,
            text: TextStyles::read(&source)?,
            all_colors,
        })
    }
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::{TokenError, Tokens, Weight, parse_color};

    #[test]
    fn the_design_file_loads_with_every_token_the_interface_uses() {
        let tokens = Tokens::load().unwrap();
        assert_eq!(tokens.color.bg_app, Color32::from_rgb(0x11, 0x11, 0x11));
        assert_eq!(tokens.color.deck_b, Color32::from_rgb(0xff, 0x8a, 0x3d));
        assert_eq!(tokens.space.s4, 12.0);
        assert_eq!(tokens.size.knob, 40.0);
        assert_eq!(tokens.stroke.led, 3.0);
        assert_eq!(tokens.text.time.size, 22.0);
        assert_eq!(tokens.text.time.weight, Weight::SemiBold);
        assert!(tokens.text.readout.tabular && !tokens.text.body.tabular);
        // 49 colours in the file, later milestones included.
        assert_eq!(tokens.all_colors.len(), 49);
    }

    #[test]
    fn an_alias_takes_the_colour_it_points_at() {
        let tokens = Tokens::load().unwrap();
        assert_eq!(tokens.color.meter_clip, tokens.color.status_error);
        assert_eq!(tokens.all_colors["meter-ok"], tokens.color.status_ok);
    }

    #[test]
    fn colours_are_read_in_both_notations() {
        assert_eq!(
            parse_color("#3d8bff"),
            Some(Color32::from_rgb(61, 139, 255))
        );
        assert_eq!(
            parse_color("rgba(237, 237, 237, 0.6)"),
            Some(Color32::from_rgba_unmultiplied(237, 237, 237, 153))
        );
        for bad in [
            "",
            "#12345",
            "#12345g",
            "red",
            "rgba(1, 2, 3)",
            "rgba(1, 2, 3, 2)",
            "#ññññññ",
        ] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_broken_file_is_an_error_that_names_the_token() {
        assert!(matches!(
            Tokens::parse("not toml ["),
            Err(TokenError::Syntax(_))
        ));
        assert_eq!(
            Tokens::parse("[color]\n"),
            Err(TokenError::Missing("bg-app".to_owned()))
        );
        let looped =
            super::SOURCE.replace("meter-ok = \"{status-ok}\"", "meter-ok = \"{meter-ok}\"");
        assert_eq!(
            Tokens::parse(&looped),
            Err(TokenError::Invalid("meter-ok".to_owned()))
        );
        let wrong_type = super::SOURCE.replace("space-1 = 2", "space-1 = \"two\"");
        assert_eq!(
            Tokens::parse(&wrong_type),
            Err(TokenError::Invalid("space-1".to_owned()))
        );
    }
}
