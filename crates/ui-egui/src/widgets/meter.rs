//! The level meter: two columns of segments, a peak hold and a clip indicator.
//!
//! Component: `Meter` in `docs/design/DESIGN.md`. The meter reads levels from the state
//! snapshot; what it remembers between frames (the held peak, the clip indicator) is how a
//! reading is shown, not state of the engine.

use egui::{CornerRadius, Rect};

use crate::cx::Cx;

/// Bottom of the scale, in decibels below full scale.
const FLOOR_DB: f32 = -48.0;
/// Where the colour changes from the first to the second.
const WARN_DB: f32 = -6.0;
const PEAK_HOLD_SECONDS: f64 = 1.0;
pub const CLIP_HOLD_SECONDS: f64 = 2.0;

/// Decibels of a linear level; silence is far below the scale.
pub fn to_db(level: f32) -> f32 {
    if level > 0.0 && level.is_finite() {
        20.0 * level.log10()
    } else {
        f32::NEG_INFINITY
    }
}

/// Position of a level on the scale, from 0 at the floor to 1 at full scale.
pub fn scale_position(db: f32) -> f32 {
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// What a meter keeps between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeterMemory {
    /// Highest recent level of each side, and when it was reached.
    peaks: [(f32, f64); 2],
    /// When the signal last passed full scale; cleared by the hold time or by a click.
    clipped_at: Option<f64>,
}

impl MeterMemory {
    /// Takes in the levels of a frame at time `now`, in seconds.
    pub fn update(&mut self, levels: [f32; 2], now: f64) {
        for (peak, level) in self.peaks.iter_mut().zip(levels) {
            if level >= peak.0 || now - peak.1 > PEAK_HOLD_SECONDS {
                *peak = (level, now);
            }
        }
        if levels.iter().any(|level| *level > 1.0) {
            self.clipped_at = Some(now);
        } else if self
            .clipped_at
            .is_some_and(|at| now - at > CLIP_HOLD_SECONDS)
        {
            self.clipped_at = None;
        }
    }

    pub fn is_clipped(&self) -> bool {
        self.clipped_at.is_some()
    }

    pub fn reset_clip(&mut self) {
        self.clipped_at = None;
    }
}

pub struct MeterSpec {
    pub levels: [f32; 2],
    pub memory: MeterMemory,
    /// Number of segments along the meter.
    pub segments: usize,
    /// Columns side by side and segments stacked from the bottom; otherwise rows one above
    /// the other and segments from the left.
    pub vertical: bool,
}

/// Draws the two columns of a meter inside `rect`.
pub fn meter(cx: &Cx<'_>, rect: Rect, spec: &MeterSpec) {
    let tokens = &cx.theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let gap = 1.0;
    let between = tokens.space.s1;
    let segments = spec.segments.max(1);
    let (length, breadth) = if spec.vertical {
        (rect.height(), rect.width())
    } else {
        (rect.width(), rect.height())
    };
    let column = (breadth - between) / 2.0;
    let step = (length + gap) / segments as f32;
    // The segment that holds the level at the top of the warning range and beyond.
    let warn_from = (scale_position(WARN_DB) * segments as f32).floor() as usize;
    for (side, level) in spec.levels.iter().enumerate() {
        let lit = (scale_position(to_db(*level)) * segments as f32).round() as usize;
        let held = scale_position(to_db(spec.memory.peaks[side].0)) * segments as f32;
        let held = (held.round() as usize).checked_sub(1);
        for segment in 0..segments {
            let on = segment < lit || (Some(segment) == held && held.is_some_and(|held| held > 0));
            let ink = if !on {
                color.surface_control
            } else if segment >= warn_from {
                color.meter_warn
            } else {
                color.meter_ok
            };
            let across = side as f32 * (column + between);
            let along = segment as f32 * step;
            let cell = if spec.vertical {
                Rect::from_min_size(
                    egui::pos2(rect.left() + across, rect.bottom() - along - (step - gap)),
                    egui::vec2(column, step - gap),
                )
            } else {
                Rect::from_min_size(
                    egui::pos2(rect.left() + along, rect.top() + across),
                    egui::vec2(step - gap, column),
                )
            };
            painter.rect_filled(cell, CornerRadius::ZERO, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CLIP_HOLD_SECONDS, MeterMemory, scale_position, to_db};

    #[test]
    fn the_scale_runs_from_the_floor_to_full_scale() {
        assert_eq!(scale_position(to_db(1.0)), 1.0);
        assert_eq!(scale_position(to_db(0.0)), 0.0);
        assert_eq!(scale_position(to_db(4.0)), 1.0);
        assert!((scale_position(-24.0) - 0.5).abs() < 1e-6);
        assert_eq!(scale_position(to_db(f32::NAN)), 0.0);
    }

    #[test]
    fn the_clip_indicator_holds_and_then_clears() {
        let mut memory = MeterMemory::default();
        memory.update([0.5, 0.5], 0.0);
        assert!(!memory.is_clipped());
        memory.update([0.5, 1.2], 1.0);
        assert!(memory.is_clipped());
        memory.update([0.1, 0.1], 1.0 + CLIP_HOLD_SECONDS - 0.1);
        assert!(memory.is_clipped());
        memory.update([0.1, 0.1], 1.0 + CLIP_HOLD_SECONDS + 0.1);
        assert!(!memory.is_clipped());
        memory.update([2.0, 0.0], 5.0);
        memory.reset_clip();
        assert!(!memory.is_clipped());
    }

    #[test]
    fn the_peak_is_held_for_a_second() {
        let mut memory = MeterMemory::default();
        memory.update([0.8, 0.2], 0.0);
        memory.update([0.1, 0.1], 0.5);
        assert_eq!(memory.peaks[0].0, 0.8);
        memory.update([0.1, 0.1], 1.2);
        assert_eq!(memory.peaks[0].0, 0.1);
    }
}
