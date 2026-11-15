//! The icons of the design system, read from `docs/design/icons/icons.toml` and drawn with
//! the painter.
//!
//! The file stores each icon as SVG shape elements. The project has no SVG dependency: this
//! module reads the subset the file uses (`path`, `circle` and `rect`; path commands
//! `M L H V C S Q A Z`, absolute and relative) and turns every shape into straight segments
//! once, at start-up. `docs/decisions/0011-icons.md` records why it is done at run time.

use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};
use std::fmt;

use egui::epaint::PathShape;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};

const SOURCE: &str = include_str!("../../../docs/design/icons/icons.toml");
const MARK_SOURCE: &str = include_str!("../../../docs/design/logo/platter-mark.toml");

/// Segments a curve is cut into. Icons are drawn at 12 to 24 pixels, where more cannot be
/// seen.
const CURVE_SEGMENTS: usize = 12;
/// Segments of a full circle.
const CIRCLE_SEGMENTS: usize = 32;
/// Upper bounds for what one icon may hold: the file is data, and data is bounded.
const MAX_SHAPES: usize = 32;
const MAX_POINTS: usize = 4_096;

/// The icons the interface draws so far. Each name is a key of the icon file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Glyph {
    Play,
    Pause,
    Settings,
    Folder,
    FileAudio,
    LoadLeft,
    LoadRight,
    Warning,
    Error,
    ChevronDown,
    ChevronRight,
    Headphones,
    Plus,
    Minus,
    ChevronLeft,
    Lock,
    Trash,
    Fullscreen,
    Search,
    Close,
    ChevronUp,
}

impl Glyph {
    pub const ALL: [Self; 21] = [
        Self::Play,
        Self::Pause,
        Self::Settings,
        Self::Folder,
        Self::FileAudio,
        Self::LoadLeft,
        Self::LoadRight,
        Self::Warning,
        Self::Error,
        Self::ChevronDown,
        Self::ChevronRight,
        Self::Headphones,
        Self::Plus,
        Self::Minus,
        Self::ChevronLeft,
        Self::Lock,
        Self::Trash,
        Self::Fullscreen,
        Self::Search,
        Self::Close,
        Self::ChevronUp,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Settings => "settings",
            Self::Folder => "folder",
            Self::FileAudio => "file-audio",
            Self::LoadLeft => "load-left",
            Self::LoadRight => "load-right",
            Self::Warning => "warning",
            Self::Error => "error",
            Self::ChevronDown => "chevron-down",
            Self::ChevronRight => "chevron-right",
            Self::Headphones => "headphones",
            Self::Plus => "plus",
            Self::Minus => "minus",
            Self::ChevronLeft => "chevron-left",
            Self::Lock => "lock",
            Self::Trash => "trash",
            Self::Fullscreen => "fullscreen",
            Self::Search => "search",
            Self::Close => "close",
            Self::ChevronUp => "chevron-up",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconError {
    Syntax(String),
    /// The icon is missing, or its shapes use something outside the subset.
    Icon {
        name: String,
        problem: &'static str,
    },
}

impl fmt::Display for IconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(message) => write!(f, "the icon file cannot be read: {message}"),
            Self::Icon { name, problem } => {
                write!(f, "the icon `{name}` cannot be drawn: {problem}")
            }
        }
    }
}

impl std::error::Error for IconError {}

/// One outline of an icon, in the units of the icon grid.
#[derive(Clone, Debug, PartialEq)]
struct Outline {
    points: Vec<[f32; 2]>,
    closed: bool,
    /// Lengths of dash and gap, for a dashed stroke.
    dash: Option<[f32; 2]>,
    /// Stroke width of this shape, when it differs from the one of its file.
    width: Option<f32>,
    /// The shape is filled and has no stroke, whatever its file says for the rest.
    filled: bool,
}

impl Outline {
    fn new(points: Vec<[f32; 2]>, closed: bool) -> Self {
        Self {
            points,
            closed,
            dash: None,
            width: None,
            filled: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    /// Filled outlines without a stroke; otherwise stroked outlines without a fill.
    solid: bool,
    outlines: Vec<Outline>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Icons {
    grid: f32,
    stroke_width: f32,
    icons: BTreeMap<String, Icon>,
}

impl Icons {
    /// Reads the icon file compiled into the binary.
    pub fn load() -> Result<Self, IconError> {
        Self::parse(SOURCE)
    }

    fn parse(source: &str) -> Result<Self, IconError> {
        let root: toml::Table = source
            .parse()
            .map_err(|error: toml::de::Error| IconError::Syntax(error.message().to_owned()))?;
        let number = |name: &str| match root.get(name) {
            Some(toml::Value::Integer(number)) => Some(*number as f32),
            Some(toml::Value::Float(number)) => Some(*number as f32),
            _ => None,
        };
        let header = |name: &str| {
            number(name)
                .filter(|value| value.is_finite() && *value > 0.0 && *value <= 1_024.0)
                .ok_or_else(|| IconError::Syntax(format!("`{name}` is missing or out of range")))
        };
        let (grid, stroke_width) = (header("grid")?, header("stroke_width")?);
        let table = root
            .get("icons")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| IconError::Syntax("the table `icons` is missing".to_owned()))?;
        let mut icons = BTreeMap::new();
        for (name, value) in table {
            let failed = |problem| IconError::Icon {
                name: name.clone(),
                problem,
            };
            let entry = value
                .as_table()
                .ok_or_else(|| failed("it is not a table"))?;
            let solid = entry
                .get("solid")
                .and_then(toml::Value::as_bool)
                .ok_or_else(|| failed("`solid` is missing"))?;
            let shapes = entry
                .get("shapes")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| failed("`shapes` is missing"))?;
            let outlines = parse_shapes(shapes).map_err(failed)?;
            icons.insert(name.clone(), Icon { solid, outlines });
        }
        let icons = Self {
            grid,
            stroke_width,
            icons,
        };
        for glyph in Glyph::ALL {
            if !icons.icons.contains_key(glyph.key()) {
                return Err(IconError::Icon {
                    name: glyph.key().to_owned(),
                    problem: "it is not in the file",
                });
            }
        }
        Ok(icons)
    }

    /// Number of icons in the file.
    pub fn len(&self) -> usize {
        self.icons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }

    /// Draws `glyph` inside `rect` in one colour: the colour of the text beside it.
    pub fn paint(&self, painter: &Painter, glyph: Glyph, rect: Rect, color: Color32) {
        let Some(icon) = self.icons.get(glyph.key()) else {
            return;
        };
        let drawing = Drawing {
            grid: self.grid,
            stroke_width: self.stroke_width,
            solid: icon.solid,
        };
        drawing.paint(painter, &icon.outlines, rect, color);
    }
}

/// The mark of Platter, read from `docs/design/logo/platter-mark.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    view_box: f32,
    outlines: Vec<Outline>,
}

impl Mark {
    pub fn load() -> Result<Self, IconError> {
        let failed = |problem| IconError::Icon {
            name: "platter-mark".to_owned(),
            problem,
        };
        let root: toml::Table = MARK_SOURCE
            .parse()
            .map_err(|error: toml::de::Error| IconError::Syntax(error.message().to_owned()))?;
        let view_box = match root.get("view_box") {
            Some(toml::Value::Integer(size)) if (1..=4_096).contains(size) => *size as f32,
            _ => return Err(failed("`view_box` is missing or out of range")),
        };
        let shapes = root
            .get("shapes")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| failed("`shapes` is missing"))?;
        Ok(Self {
            view_box,
            outlines: parse_shapes(shapes).map_err(failed)?,
        })
    }

    /// Draws the mark inside `rect` in one colour.
    pub fn paint(&self, painter: &Painter, rect: Rect, color: Color32) {
        let drawing = Drawing {
            grid: self.view_box,
            // Every shape of the mark carries its own width.
            stroke_width: 1.0,
            solid: false,
        };
        drawing.paint(painter, &self.outlines, rect, color);
    }
}

/// How the outlines of one drawing are painted.
struct Drawing {
    grid: f32,
    stroke_width: f32,
    solid: bool,
}

impl Drawing {
    fn paint(&self, painter: &Painter, outlines: &[Outline], rect: Rect, color: Color32) {
        let scale = rect.width().min(rect.height()) / self.grid;
        let origin = rect.center() - egui::vec2(self.grid, self.grid) * 0.5 * scale;
        for outline in outlines {
            let points: Vec<Pos2> = outline
                .points
                .iter()
                .map(|[x, y]| origin + egui::vec2(*x, *y) * scale)
                .collect();
            let stroke = Stroke::new(outline.width.unwrap_or(self.stroke_width) * scale, color);
            if self.solid || outline.filled {
                painter.add(PathShape::convex_polygon(points, color, Stroke::NONE));
            } else if let Some([dash, gap]) = outline.dash {
                painter.extend(Shape::dashed_line(
                    &points,
                    stroke,
                    dash * scale,
                    gap * scale,
                ));
            } else if outline.closed {
                painter.add(PathShape::closed_line(points, stroke));
            } else {
                painter.add(PathShape::line(points, stroke));
            }
        }
    }
}

/// Reads the shape elements of one icon.
fn parse_shapes(shapes: &str) -> Result<Vec<Outline>, &'static str> {
    let mut outlines = Vec::new();
    let mut rest = shapes.trim();
    while !rest.is_empty() {
        let element = rest.strip_prefix('<').ok_or("text outside an element")?;
        let end = element.find("/>").ok_or("an element is not closed")?;
        let (body, after) = element.split_at(end);
        rest = after[2..].trim_start();
        let (tag, attributes) = body.split_once(' ').unwrap_or((body, ""));
        let attributes = parse_attributes(attributes)?;
        let dash = match attributes.get("stroke-dasharray") {
            None => None,
            Some(text) => match numbers(text)?.as_slice() {
                [dash, gap] if *dash > 0.0 && *gap > 0.0 => Some([*dash, *gap]),
                _ => return Err("a dash pattern needs two positive lengths"),
            },
        };
        let number = |name: &str| -> Result<f32, &'static str> {
            let text = attributes.get(name).ok_or("an attribute is missing")?;
            match numbers(text)?.as_slice() {
                [value] => Ok(*value),
                _ => Err("an attribute needs one number"),
            }
        };
        let before = outlines.len();
        match tag {
            "path" => {
                let data = attributes.get("d").ok_or("a path has no data")?;
                outlines.extend(parse_path(data)?);
            }
            "circle" => outlines.push(circle(number("cx")?, number("cy")?, number("r")?)),
            "rect" => {
                let radius = if attributes.contains_key("rx") {
                    number("rx")?
                } else {
                    0.0
                };
                let origin = [number("x")?, number("y")?];
                outlines.push(rounded_rect(
                    origin,
                    number("width")?,
                    number("height")?,
                    radius,
                ));
            }
            _ => return Err("an element outside the subset"),
        }
        let width = match attributes.get("stroke-width") {
            None => None,
            Some(_) => Some(number("stroke-width")?).filter(|width| *width > 0.0),
        };
        let filled = attributes.get("fill").is_some_and(|fill| *fill != "none");
        for outline in &mut outlines[before..] {
            outline.dash = dash;
            outline.width = width;
            outline.filled = filled;
        }
        if outlines.len() > MAX_SHAPES {
            return Err("too many shapes");
        }
    }
    Ok(outlines)
}

/// Reads `name="value"` pairs.
fn parse_attributes(text: &str) -> Result<BTreeMap<&str, &str>, &'static str> {
    let mut attributes = BTreeMap::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let (name, after) = rest.split_once("=\"").ok_or("an attribute has no value")?;
        let (value, after) = after.split_once('"').ok_or("a value is not closed")?;
        attributes.insert(name.trim(), value);
        rest = after.trim_start();
    }
    Ok(attributes)
}

/// Reads a list of numbers separated by spaces or commas.
fn numbers(text: &str) -> Result<Vec<f32>, &'static str> {
    let mut reader = Reader::new(text);
    let mut values = Vec::new();
    while reader.has_number() {
        values.push(reader.number()?);
    }
    if reader.peek().is_some() {
        return Err("a number is malformed");
    }
    Ok(values)
}

/// Reads numbers and command letters from path data, where separators are optional: a sign
/// or a second decimal point starts the next number.
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            position: 0,
        }
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.bytes.get(self.position),
            Some(b' ' | b',' | b'\t' | b'\n')
        ) {
            self.position += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_separators();
        self.bytes.get(self.position).copied()
    }

    fn has_number(&mut self) -> bool {
        matches!(self.peek(), Some(b'0'..=b'9' | b'-' | b'+' | b'.'))
    }

    fn number(&mut self) -> Result<f32, &'static str> {
        self.skip_separators();
        let start = self.position;
        if matches!(self.bytes.get(self.position), Some(b'-' | b'+')) {
            self.position += 1;
        }
        let mut seen_point = false;
        while let Some(byte) = self.bytes.get(self.position) {
            match byte {
                b'0'..=b'9' => {}
                b'.' if !seen_point => seen_point = true,
                _ => break,
            }
            self.position += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.position])
            .ok()
            .and_then(|text| text.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .ok_or("a number is malformed")
    }

    /// An arc flag is one digit, and may be glued to what follows.
    fn flag(&mut self) -> Result<bool, &'static str> {
        match self.peek() {
            Some(b'0') => {
                self.position += 1;
                Ok(false)
            }
            Some(b'1') => {
                self.position += 1;
                Ok(true)
            }
            _ => Err("an arc flag is not 0 or 1"),
        }
    }

    fn point(&mut self) -> Result<[f32; 2], &'static str> {
        Ok([self.number()?, self.number()?])
    }
}

/// State of a path while its data is read.
struct PathBuilder {
    outlines: Vec<Outline>,
    current: Vec<[f32; 2]>,
    /// Where the pen is, and where the current outline started.
    pen: [f32; 2],
    start: [f32; 2],
    /// Second control point of the last cubic curve, for the smooth command.
    last_control: Option<[f32; 2]>,
    points: usize,
}

impl PathBuilder {
    fn finish_outline(&mut self, closed: bool) {
        if self.current.len() > 1 {
            self.outlines
                .push(Outline::new(std::mem::take(&mut self.current), closed));
        } else {
            self.current.clear();
        }
    }

    fn line_to(&mut self, point: [f32; 2]) -> Result<(), &'static str> {
        self.points += 1;
        if self.points > MAX_POINTS {
            return Err("too many points");
        }
        if self.current.is_empty() {
            self.current.push(self.pen);
        }
        self.current.push(point);
        self.pen = point;
        Ok(())
    }

    fn curve(&mut self, sample: impl Fn(f32) -> [f32; 2]) -> Result<(), &'static str> {
        for step in 1..=CURVE_SEGMENTS {
            self.line_to(sample(step as f32 / CURVE_SEGMENTS as f32))?;
        }
        Ok(())
    }

    /// Resolves a coordinate pair that may be relative to the pen.
    fn resolve(&self, relative: bool, point: [f32; 2]) -> [f32; 2] {
        if relative {
            [self.pen[0] + point[0], self.pen[1] + point[1]]
        } else {
            point
        }
    }
}

fn parse_path(data: &str) -> Result<Vec<Outline>, &'static str> {
    let mut reader = Reader::new(data);
    let mut path = PathBuilder {
        outlines: Vec::new(),
        current: Vec::new(),
        pen: [0.0; 2],
        start: [0.0; 2],
        last_control: None,
        points: 0,
    };
    let mut command = None;
    loop {
        match reader.peek() {
            None => break,
            Some(letter) if letter.is_ascii_alphabetic() => {
                reader.position += 1;
                command = Some(letter);
                if letter.eq_ignore_ascii_case(&b'z') {
                    path.finish_outline(true);
                    path.pen = path.start;
                    path.last_control = None;
                }
                continue;
            }
            Some(_) => {}
        }
        // A number without a letter repeats the last command; a move repeats as a line.
        let letter = command.ok_or("the path does not start with a command")?;
        path_command(&mut path, &mut reader, letter)?;
        if letter.eq_ignore_ascii_case(&b'm') {
            command = Some(if letter == b'm' { b'l' } else { b'L' });
        }
    }
    path.finish_outline(false);
    Ok(path.outlines)
}

fn path_command(
    path: &mut PathBuilder,
    reader: &mut Reader<'_>,
    letter: u8,
) -> Result<(), &'static str> {
    let relative = letter.is_ascii_lowercase();
    let mut control_after = None;
    match letter.to_ascii_lowercase() {
        b'm' => {
            let point = path.resolve(relative, reader.point()?);
            path.finish_outline(false);
            path.pen = point;
            path.start = point;
        }
        b'l' => {
            let point = path.resolve(relative, reader.point()?);
            path.line_to(point)?;
        }
        b'h' => {
            let x = reader.number()? + if relative { path.pen[0] } else { 0.0 };
            path.line_to([x, path.pen[1]])?;
        }
        b'v' => {
            let y = reader.number()? + if relative { path.pen[1] } else { 0.0 };
            path.line_to([path.pen[0], y])?;
        }
        b'c' | b's' => {
            let from = path.pen;
            let first = if letter.eq_ignore_ascii_case(&b'c') {
                path.resolve(relative, reader.point()?)
            } else {
                // The smooth form mirrors the last control point through the pen.
                let last = path.last_control.unwrap_or(from);
                [2.0 * from[0] - last[0], 2.0 * from[1] - last[1]]
            };
            let second = path.resolve(relative, reader.point()?);
            let to = path.resolve(relative, reader.point()?);
            path.curve(|t| cubic(from, first, second, to, t))?;
            control_after = Some(second);
        }
        b'q' => {
            let from = path.pen;
            let control = path.resolve(relative, reader.point()?);
            let to = path.resolve(relative, reader.point()?);
            path.curve(|t| {
                let u = 1.0 - t;
                [0, 1]
                    .map(|axis| u * u * from[axis] + 2.0 * u * t * control[axis] + t * t * to[axis])
            })?;
        }
        b'a' => {
            let radii = reader.point()?;
            let rotation = reader.number()?;
            let (large, sweep) = (reader.flag()?, reader.flag()?);
            let to = path.resolve(relative, reader.point()?);
            arc(path, radii, rotation, large, sweep, to)?;
        }
        _ => return Err("a path command outside the subset"),
    }
    path.last_control = control_after;
    Ok(())
}

fn cubic(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    [0, 1].map(|axis| {
        u * u * u * p0[axis]
            + 3.0 * u * u * t * p1[axis]
            + 3.0 * u * t * t * p2[axis]
            + t * t * t * p3[axis]
    })
}

/// An elliptical arc from the pen to `to`, converted from the endpoint form of the path
/// syntax to its centre form as the SVG specification describes (section "Conversion from
/// endpoint to center parameterization", <https://www.w3.org/TR/SVG2/implnote.html>).
fn arc(
    path: &mut PathBuilder,
    radii: [f32; 2],
    rotation_degrees: f32,
    large: bool,
    sweep: bool,
    to: [f32; 2],
) -> Result<(), &'static str> {
    let from = path.pen;
    let (mut rx, mut ry) = (radii[0].abs(), radii[1].abs());
    if rx < 1e-6 || ry < 1e-6 || from == to {
        return path.line_to(to);
    }
    let (sin, cos) = rotation_degrees.to_radians().sin_cos();
    let half = [(from[0] - to[0]) / 2.0, (from[1] - to[1]) / 2.0];
    let x1 = cos * half[0] + sin * half[1];
    let y1 = -sin * half[0] + cos * half[1];
    // Radii too small to reach the end point are scaled up, as the specification says.
    let reach = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if reach > 1.0 {
        rx *= reach.sqrt();
        ry *= reach.sqrt();
    }
    let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut factor = (numerator.max(0.0) / denominator).sqrt();
    if large == sweep {
        factor = -factor;
    }
    let (cx1, cy1) = (factor * rx * y1 / ry, -factor * ry * x1 / rx);
    let centre = [
        cos * cx1 - sin * cy1 + (from[0] + to[0]) / 2.0,
        sin * cx1 + cos * cy1 + (from[1] + to[1]) / 2.0,
    ];
    let angle = |x: f32, y: f32| y.atan2(x);
    let start = angle((x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut delta = angle((-x1 - cx1) / rx, (-y1 - cy1) / ry) - start;
    if sweep && delta < 0.0 {
        delta += TAU;
    } else if !sweep && delta > 0.0 {
        delta -= TAU;
    }
    let segments = ((delta.abs() / TAU) * CIRCLE_SEGMENTS as f32)
        .ceil()
        .max(1.0) as usize;
    for step in 1..=segments {
        let theta = start + delta * step as f32 / segments as f32;
        let (x, y) = (rx * theta.cos(), ry * theta.sin());
        // The last point is the end point itself, so outlines join without a gap.
        let point = if step == segments {
            to
        } else {
            [cos * x - sin * y + centre[0], sin * x + cos * y + centre[1]]
        };
        path.line_to(point)?;
    }
    Ok(())
}

fn circle(cx: f32, cy: f32, radius: f32) -> Outline {
    let points = (0..CIRCLE_SEGMENTS)
        .map(|step| {
            let theta = TAU * step as f32 / CIRCLE_SEGMENTS as f32;
            [cx + radius * theta.cos(), cy + radius * theta.sin()]
        })
        .collect();
    Outline::new(points, true)
}

fn rounded_rect(origin: [f32; 2], width: f32, height: f32, radius: f32) -> Outline {
    let radius = radius.clamp(0.0, width.min(height) / 2.0);
    let [x, y] = origin;
    if radius == 0.0 {
        return Outline::new(
            vec![
                [x, y],
                [x + width, y],
                [x + width, y + height],
                [x, y + height],
            ],
            true,
        );
    }
    // Each corner is a quarter circle around its own centre, clockwise from the top left.
    let corners = [
        ([x + radius, y + radius], PI),
        ([x + width - radius, y + radius], 1.5 * PI),
        ([x + width - radius, y + height - radius], 0.0),
        ([x + radius, y + height - radius], 0.5 * PI),
    ];
    let steps = CIRCLE_SEGMENTS / 4;
    let points = corners
        .iter()
        .flat_map(|(centre, from)| {
            (0..=steps).map(move |step| {
                let theta = from + 0.5 * PI * step as f32 / steps as f32;
                [
                    centre[0] + radius * theta.cos(),
                    centre[1] + radius * theta.sin(),
                ]
            })
        })
        .collect();
    Outline::new(points, true)
}

#[cfg(test)]
mod tests {
    use super::{
        CIRCLE_SEGMENTS, Glyph, IconError, Icons, Mark, numbers, parse_path, parse_shapes,
    };

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn every_icon_of_the_file_is_read_and_stays_on_its_grid() {
        let icons = Icons::load().unwrap();
        assert_eq!(icons.len(), 43);
        assert_eq!((icons.grid, icons.stroke_width), (24.0, 1.5));
        for (name, icon) in &icons.icons {
            assert!(!icon.outlines.is_empty(), "{name}");
            for outline in &icon.outlines {
                assert!(outline.points.len() >= 2, "{name}");
                for [x, y] in &outline.points {
                    assert!(
                        (0.0..=24.0).contains(x) && (0.0..=24.0).contains(y),
                        "{name}: {x} {y}"
                    );
                }
            }
        }
        for glyph in Glyph::ALL {
            assert!(icons.icons.contains_key(glyph.key()));
        }
    }

    #[test]
    fn the_mark_has_three_rings_a_filled_centre_and_a_line() {
        let mark = Mark::load().unwrap();
        assert_eq!(mark.view_box, 96.0);
        assert_eq!(mark.outlines.len(), 5);
        let widths: Vec<_> = mark.outlines.iter().map(|outline| outline.width).collect();
        assert_eq!(widths, [Some(4.0), Some(2.0), Some(2.0), None, Some(4.0)]);
        let filled: Vec<_> = mark.outlines.iter().map(|outline| outline.filled).collect();
        assert_eq!(filled, [false, false, false, true, false]);
    }

    #[test]
    fn only_the_transport_glyphs_are_solid() {
        let icons = Icons::load().unwrap();
        let solid: Vec<&str> = icons
            .icons
            .iter()
            .filter(|(_, icon)| icon.solid)
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(solid, ["pause", "play"]);
        // Two bars, each a closed outline of its own.
        let pause = &icons.icons["pause"];
        assert_eq!(pause.outlines.len(), 2);
        assert!(pause.outlines.iter().all(|outline| outline.closed));
    }

    #[test]
    fn numbers_need_no_separator_before_a_sign_or_a_second_point() {
        assert_eq!(numbers("1.5-1.5").unwrap(), [1.5, -1.5]);
        assert_eq!(numbers(".75.25").unwrap(), [0.75, 0.25]);
        assert_eq!(numbers("2 3,4").unwrap(), [2.0, 3.0, 4.0]);
        assert!(numbers("1.5x").is_err());
        assert!(numbers("-").is_err());
    }

    #[test]
    fn relative_lines_and_repeated_commands_follow_the_pen() {
        // The chevrons of `beatjump`: two open outlines of three points.
        let outlines = parse_path("M5 7l5 5-5 5M12 7l5 5-5 5").unwrap();
        assert_eq!(outlines.len(), 2);
        assert_eq!(outlines[0].points, [[5.0, 7.0], [10.0, 12.0], [5.0, 17.0]]);
        assert_eq!(
            outlines[1].points,
            [[12.0, 7.0], [17.0, 12.0], [12.0, 17.0]]
        );
        assert!(!outlines[0].closed);
        // Horizontal and vertical lines, absolute and relative, and a close.
        let outlines = parse_path("M6 4.5h4v15H6z").unwrap();
        assert_eq!(
            outlines[0].points,
            [[6.0, 4.5], [10.0, 4.5], [10.0, 19.5], [6.0, 19.5]]
        );
        assert!(outlines[0].closed);
    }

    #[test]
    fn an_arc_ends_where_the_path_says_and_keeps_its_radius() {
        // A half circle of radius 4 from (8, 8) down to (8, 16), bulging right.
        let outlines = parse_path("M8 8a4 4 0 0 1 0 8").unwrap();
        let points = &outlines[0].points;
        assert!(close(points[0], [8.0, 8.0]));
        assert!(close(points[points.len() - 1], [8.0, 16.0]));
        assert_eq!(points.len(), CIRCLE_SEGMENTS / 2 + 1);
        for point in points {
            let radius = ((point[0] - 8.0).powi(2) + (point[1] - 12.0).powi(2)).sqrt();
            assert!((radius - 4.0).abs() < 1e-3, "{point:?}");
            assert!(point[0] >= 8.0 - 1e-3);
        }
        // The other sweep flag bulges the other way.
        let left = parse_path("M8 8a4 4 0 0 0 0 8").unwrap();
        assert!(left[0].points.iter().all(|point| point[0] <= 8.0 + 1e-3));
        // Flags glued to the numbers that follow, as minified path data writes them.
        let glued = parse_path("M8 8a4 4 0 010 8").unwrap();
        assert!(close(glued[0].points[CIRCLE_SEGMENTS / 4], [12.0, 12.0]));
    }

    #[test]
    fn curves_start_at_the_pen_and_end_at_their_end_point() {
        let outlines = parse_path("M0 0C0 10 10 10 10 0S20 -10 20 0Q25 5 30 0").unwrap();
        let points = &outlines[0].points;
        assert!(close(points[0], [0.0, 0.0]));
        assert!(close(points[12], [10.0, 0.0]));
        assert!(close(points[24], [20.0, 0.0]));
        assert!(close(points[36], [30.0, 0.0]));
        // The first cubic bulges down, the smooth one mirrors it upwards.
        assert!(points[6][1] > 5.0 && points[18][1] < -5.0);
    }

    #[test]
    fn shapes_outside_the_subset_are_errors() {
        for bad in [
            "<line x1=\"0\" y1=\"0\"/>",
            "<path d=\"M0 0T5 5\"/>",
            "<path d=\"5 5\"/>",
            "<path/>",
            "<circle cx=\"1\" cy=\"1\"/>",
            "<path d=\"M0 0L5 5\" stroke-dasharray=\"2\"/>",
            "loose text",
            "<path d=\"M0 0L5 5\"",
        ] {
            assert!(parse_shapes(bad).is_err(), "{bad}");
        }
        let dashed = parse_shapes("<path d=\"M4 17h10\" stroke-dasharray=\"2 3\"/>").unwrap();
        assert_eq!(dashed[0].dash, Some([2.0, 3.0]));
    }

    #[test]
    fn a_file_without_an_icon_the_interface_needs_is_an_error() {
        let source = "grid = 24\nstroke_width = 1.5\n[icons.play]\nsolid = true\nshapes = '<path d=\"M7 4.5v15l12-7.5z\"/>'\n";
        assert!(matches!(
            Icons::parse(source),
            Err(IconError::Icon { name, .. }) if name == "pause"
        ));
        assert!(matches!(
            Icons::parse("grid = 0\n"),
            Err(IconError::Syntax(_))
        ));
    }
}
