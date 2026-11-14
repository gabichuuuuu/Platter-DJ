//! Controls that set a value: the knob, the vertical fader and the crossfader.
//!
//! All three work on a position from 0 to 1 and emit a command with the new position; the
//! caller maps it to the units of its command. None of them keeps the value: it comes from
//! the state snapshot every frame.
//!
//! Components: `Knob`, `Fader`, `Crossfader` and `TempoControl` in `docs/design/DESIGN.md`.

use std::f32::consts::PI;

use egui::epaint::PathShape;
use egui::{
    EventFilter, Id, Key, Pos2, Rect, Response, Role, Sense, Stroke, StrokeKind, WidgetInfo,
};

use crate::command::Action;
use crate::cx::{ControlKind, Cx};
use crate::paint;

/// Pixels of drag that take a knob across its whole travel.
const KNOB_DRAG_TRAVEL: f32 = 160.0;
/// How much slower a control moves with Shift held.
const FINE_FACTOR: f64 = 0.1;

/// Size of the steps the keyboard makes, as fractions of the travel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steps {
    pub small: f64,
    pub fine: f64,
    pub large: f64,
}

impl Steps {
    /// One per cent, a tenth of that with Shift, ten per cent with Page Up and Page Down.
    pub const PERCENT: Self = Self {
        small: 0.01,
        fine: 0.001,
        large: 0.1,
    };
}

pub struct ValueSpec<'a> {
    /// Name for assistive technology.
    pub name: &'a str,
    /// Current position, `0.0..=1.0`.
    pub position: f64,
    /// Position a double click or Home returns to.
    pub default: f64,
    pub steps: Steps,
    pub enabled: bool,
}

/// Which way the keys and the pointer move a control.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    /// A knob: every arrow key works, and dragging up raises the value.
    Rotary,
    Vertical,
    Horizontal,
}

/// The interaction all three controls share. Returns the response and, when the user moved
/// the control this frame, its new position.
fn interact(
    cx: &mut Cx<'_>,
    rect: Rect,
    track: Rect,
    id: Id,
    spec: &ValueSpec<'_>,
    axis: Axis,
) -> (Response, Option<f64>) {
    let sense = cx.sense(spec.enabled, Sense::click_and_drag());
    let response = cx.ui.interact(rect, id, sense);
    response.widget_info(|| WidgetInfo::slider(spec.enabled, spec.position, spec.name));
    if !spec.enabled || !cx.interactive {
        return (response, None);
    }
    // A click or the start of a drag gives the control the keyboard, so the keys that
    // adjust it work right after the pointer touched it.
    if response.clicked() || response.drag_started() {
        response.request_focus();
    }
    let mut position = None;
    let fine = cx.ui.input(|input| input.modifiers.shift);

    if response.double_clicked() {
        position = Some(spec.default);
    } else if axis == Axis::Rotary {
        if response.dragged() {
            let scale = if fine { FINE_FACTOR } else { 1.0 };
            let delta = f64::from(-response.drag_delta().y / KNOB_DRAG_TRAVEL) * scale;
            position = Some(spec.position + delta);
        }
    } else if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        // The cap follows the pointer; a click on the track jumps there.
        let along = match axis {
            Axis::Horizontal => (pointer.x - track.left()) / track.width(),
            _ => (track.bottom() - pointer.y) / track.height(),
        };
        position = Some(f64::from(along));
    }

    if response.has_focus() {
        // The arrows move the control instead of the focus.
        let filter = EventFilter {
            horizontal_arrows: axis != Axis::Vertical,
            vertical_arrows: axis != Axis::Horizontal,
            ..EventFilter::default()
        };
        cx.ui
            .memory_mut(|memory| memory.set_focus_lock_filter(id, filter));
        let (up_keys, down_keys): (&[Key], &[Key]) = match axis {
            Axis::Rotary => (
                &[Key::ArrowUp, Key::ArrowRight],
                &[Key::ArrowDown, Key::ArrowLeft],
            ),
            Axis::Vertical => (&[Key::ArrowUp], &[Key::ArrowDown]),
            Axis::Horizontal => (&[Key::ArrowRight], &[Key::ArrowLeft]),
        };
        cx.out.claim(up_keys);
        cx.out.claim(down_keys);
        cx.out.claim(&[Key::Home, Key::PageUp, Key::PageDown]);
        let step = if fine {
            spec.steps.fine
        } else {
            spec.steps.small
        };
        let pressed = |keys: &[Key]| {
            cx.ui
                .input(|input| keys.iter().filter(|key| input.key_pressed(**key)).count())
        };
        let mut moved = (pressed(up_keys) as f64 - pressed(down_keys) as f64) * step;
        moved +=
            (pressed(&[Key::PageUp]) as f64 - pressed(&[Key::PageDown]) as f64) * spec.steps.large;
        if pressed(&[Key::Home]) > 0 {
            position = Some(spec.default);
        } else if moved != 0.0 {
            position = Some(spec.position + moved);
        }
    }
    let position = position
        .filter(|position| position.is_finite())
        .map(|position| position.clamp(0.0, 1.0))
        .filter(|position| *position != spec.position);
    (response, position)
}

pub struct KnobSpec<'a> {
    pub value: ValueSpec<'a>,
    /// Diameter of the control with its arc.
    pub size: f32,
    /// The value arc starts at the centre of the travel instead of its minimum.
    pub centre_detent: bool,
    /// The value arc is hidden: an equaliser band that is cut.
    pub hide_arc: bool,
}

/// Angle of a knob position, in screen coordinates: the arc runs clockwise from 7:30 to
/// 4:30 o'clock.
fn knob_angle(position: f64) -> f32 {
    0.75 * PI + position.clamp(0.0, 1.0) as f32 * 1.5 * PI
}

fn arc_points(centre: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let steps = (((to - from).abs() / (1.5 * PI)) * 36.0).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|step| {
            let angle = from + (to - from) * step as f32 / steps as f32;
            centre + egui::vec2(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

/// Draws a knob in the square `rect` and emits `make(position)` when it is turned.
pub fn knob(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    spec: KnobSpec<'_>,
    make: impl FnOnce(f64) -> Action,
) -> Response {
    let centre = rect.center();
    let (response, moved) = interact(cx, rect, rect, id, &spec.value, Axis::Rotary);
    let action = make(moved.unwrap_or(spec.value.position));
    cx.record(action.id(), rect, ControlKind::Value, spec.value.enabled);
    if moved.is_some() {
        cx.out.emit(action);
    }

    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    // The arc sits on the edge of the control and the cap 7px inside it.
    let arc_radius = spec.size / 2.0 - 1.0;
    let cap_radius = arc_radius - 7.0;
    let arc_stroke = |ink| Stroke::new(2.0, ink);
    let ink = if spec.value.enabled {
        color.text_primary
    } else {
        color.text_disabled
    };
    painter.add(PathShape::line(
        arc_points(centre, arc_radius, knob_angle(0.0), knob_angle(1.0)),
        arc_stroke(color.line_key),
    ));
    let from = if spec.centre_detent { 0.5 } else { 0.0 };
    if !spec.hide_arc && spec.value.enabled && (spec.value.position - from).abs() > 1e-4 {
        painter.add(PathShape::line(
            arc_points(
                centre,
                arc_radius,
                knob_angle(from),
                knob_angle(spec.value.position),
            ),
            arc_stroke(ink),
        ));
    }
    if spec.centre_detent {
        let top = centre - egui::vec2(0.0, arc_radius);
        painter.line_segment(
            [top - egui::vec2(0.0, 2.0), top + egui::vec2(0.0, 3.0)],
            paint::hairline(cx.theme, color.line_control),
        );
    }
    let fill = if response.dragged() {
        color.surface_pressed
    } else if response.hovered() && spec.value.enabled {
        color.surface_hover
    } else {
        color.surface_control
    };
    painter.circle(
        centre,
        cap_radius,
        fill,
        paint::hairline(cx.theme, color.line_key),
    );
    let angle = knob_angle(spec.value.position);
    let direction = egui::vec2(angle.cos(), angle.sin());
    painter.line_segment(
        [
            centre + direction * (cap_radius * 0.45),
            centre + direction * (cap_radius - 2.0),
        ],
        Stroke::new(2.0, ink),
    );
    if response.has_focus() {
        painter.circle_stroke(
            centre,
            cap_radius + tokens.space.s1 + tokens.stroke.focus / 2.0,
            Stroke::new(tokens.stroke.focus, color.focus_ring),
        );
    }
    response
}

pub struct FaderSpec<'a> {
    pub value: ValueSpec<'a>,
    /// Position of the longer tick beside the track: unity for a channel fader, the centre
    /// for a tempo fader.
    pub mark: f64,
}

/// Size of the cap of a vertical fader.
const CAP: egui::Vec2 = egui::vec2(26.0, 16.0);
/// Size of the cap of the crossfader.
const CROSS_CAP: egui::Vec2 = egui::vec2(16.0, 26.0);
const SLOT_WIDTH: f32 = 4.0;
const TICK_SPACING: f32 = 12.0;

fn paint_cap(cx: &Cx<'_>, cap: Rect, vertical: bool, enabled: bool) {
    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect(
        cap,
        paint::radius(tokens.radius.md),
        color.surface_pressed,
        paint::hairline(cx.theme, color.line_control),
        StrokeKind::Inside,
    );
    let ink = if enabled {
        color.text_primary
    } else {
        color.text_disabled
    };
    let line = if vertical {
        Rect::from_center_size(cap.center(), egui::vec2(cap.width() - 8.0, 2.0))
    } else {
        Rect::from_center_size(cap.center(), egui::vec2(2.0, cap.height() - 8.0))
    };
    painter.rect_filled(line, egui::CornerRadius::ZERO, ink);
}

/// Draws a vertical fader. `rect` is as wide as the cap and the ticks and as tall as the
/// track; the cap travels inside it. Emits `make(position)` when it is moved.
pub fn fader(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    spec: FaderSpec<'_>,
    make: impl FnOnce(f64) -> Action,
) -> Response {
    // The centre of the cap travels along this line, so the cap stays inside `rect`.
    let travel = rect.shrink2(egui::vec2(0.0, CAP.y / 2.0));
    let (response, moved) = interact(cx, rect, travel, id, &spec.value, Axis::Vertical);
    let action = make(moved.unwrap_or(spec.value.position));
    cx.record(action.id(), rect, ControlKind::Value, spec.value.enabled);
    if moved.is_some() {
        cx.out.emit(action);
    }

    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let slot_x = rect.left() + CAP.x / 2.0;
    let slot = Rect::from_min_max(
        egui::pos2(slot_x - SLOT_WIDTH / 2.0, rect.top()),
        egui::pos2(slot_x + SLOT_WIDTH / 2.0, rect.bottom()),
    );
    painter.rect(
        slot,
        paint::radius(tokens.radius.sm),
        color.bg_app,
        paint::hairline(cx.theme, color.line_key),
        StrokeKind::Inside,
    );
    let ticks_x = slot.right() + 10.0;
    let mut y = rect.top();
    while y <= rect.bottom() + 0.5 {
        painter.hline(
            ticks_x..=ticks_x + 6.0,
            y,
            paint::hairline(cx.theme, color.line_key),
        );
        y += TICK_SPACING;
    }
    let y_of = |position: f64| travel.bottom() - position.clamp(0.0, 1.0) as f32 * travel.height();
    painter.hline(
        ticks_x - 4.0..=ticks_x + 6.0,
        y_of(spec.mark),
        paint::hairline(cx.theme, color.line_control),
    );
    let cap = Rect::from_center_size(egui::pos2(slot_x, y_of(spec.value.position)), CAP);
    paint_cap(cx, cap, true, spec.value.enabled);
    if response.has_focus() {
        paint::focus_ring(painter, cx.theme, cap, tokens.radius.md);
    }
    response
}

/// Width a vertical fader needs: the cap, a gap and the ticks.
pub const FADER_WIDTH: f32 = 34.0;

/// Draws the crossfader along `rect` and emits `make(position)` when it is moved.
pub fn crossfader(
    cx: &mut Cx<'_>,
    rect: Rect,
    id: Id,
    spec: ValueSpec<'_>,
    make: impl FnOnce(f64) -> Action,
) -> Response {
    let travel = rect.shrink2(egui::vec2(CROSS_CAP.x / 2.0, 0.0));
    let (response, moved) = interact(cx, rect, travel, id, &spec, Axis::Horizontal);
    let action = make(moved.unwrap_or(spec.position));
    cx.record(action.id(), rect, ControlKind::Value, spec.enabled);
    if moved.is_some() {
        cx.out.emit(action);
    }

    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let slot = Rect::from_center_size(rect.center(), egui::vec2(rect.width(), SLOT_WIDTH));
    painter.rect(
        slot,
        paint::radius(tokens.radius.sm),
        color.bg_app,
        paint::hairline(cx.theme, color.line_key),
        StrokeKind::Inside,
    );
    painter.vline(
        rect.center().x,
        slot.bottom() + 6.0..=slot.bottom() + 12.0,
        paint::hairline(cx.theme, color.line_control),
    );
    let x = travel.left() + spec.position.clamp(0.0, 1.0) as f32 * travel.width();
    let cap = Rect::from_center_size(egui::pos2(x, rect.center().y), CROSS_CAP);
    paint_cap(cx, cap, false, spec.enabled);
    if response.has_focus() {
        paint::focus_ring(painter, cx.theme, cap, tokens.radius.md);
    }
    response
}

/// A control with a name for assistive technology and no value of its own, such as the
/// clip indicator. Used by widgets outside this module that need the same role.
pub fn button_info(response: &Response, enabled: bool, name: &str) {
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, name));
}
