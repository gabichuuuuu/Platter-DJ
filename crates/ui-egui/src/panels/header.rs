//! The status header across the top of the window, and the notice below it when no audio
//! device is playing.
//!
//! Components: `StatusHeader` and `Notice` in `docs/design/DESIGN.md`; placement from
//! `docs/design/screens/M2-main.html` and `Startup-states.html`.

use egui::{Align2, CornerRadius, Rect, Sense};

use crate::command::{AppCommand, SettingsPage};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::icons::Glyph;
use crate::model::{AudioProblem, AudioState, OutputKind, View};
use crate::paint;
use crate::shell::ViewState;
use crate::widgets::key::{self, KeySpec, Press};
use crate::widgets::meter::{self, MeterSpec};
use crate::widgets::value::button_info;

/// Load above which the reading turns to the warning colour.
const HIGH_LOAD: f32 = 0.7;
const MASTER_METER_SEGMENTS: usize = 24;

fn kind_key(kind: OutputKind) -> &'static str {
    match kind {
        OutputKind::BuiltIn => "header.audio_built_in",
        OutputKind::Usb => "header.audio_usb",
        OutputKind::Bluetooth => "header.audio_bluetooth",
        OutputKind::Other => "header.audio_other",
    }
}

/// The title and the explanation of an audio problem.
pub fn problem_keys(problem: AudioProblem) -> (&'static str, &'static str) {
    match problem {
        AudioProblem::NoDevice => ("notice.no_audio_title", "notice.no_audio_text"),
        AudioProblem::DeviceMissing => {
            ("notice.device_missing_title", "notice.device_missing_text")
        }
        AudioProblem::Unsupported => ("notice.unsupported_title", "notice.unsupported_text"),
        AudioProblem::Busy => ("notice.busy_title", "notice.busy_text"),
        AudioProblem::Failed => ("notice.failed_title", "notice.failed_text"),
    }
}

pub fn header(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    let middle = rect.center().y;
    let label = tokens.text.label;
    let readout = tokens.text.readout;
    let gap = tokens.space.s4;

    let mark = Rect::from_min_size(
        egui::pos2(rect.left() + gap, middle - 10.0),
        egui::vec2(20.0, 20.0),
    );
    theme.mark.paint(painter, mark, color.text_primary);
    let name = Rect::from_min_max(
        egui::pos2(mark.right() + tokens.space.s3, rect.top()),
        rect.max,
    );
    let name = paint::text(
        painter,
        theme,
        name,
        Align2::LEFT_CENTER,
        cx.t("app.name"),
        tokens.text.title,
        color.text_primary,
    );
    let divider = |x: f32| {
        painter.vline(
            x,
            middle - 10.0..=middle + 10.0,
            paint::hairline(theme, color.line_divider),
        );
    };
    divider(name.right() + gap);

    // The indicators, left to right. Each returns where the next one starts.
    let mut x = name.right() + 2.0 * gap + 1.0;
    let line = |x: f32| Rect::from_min_max(egui::pos2(x, rect.top()), rect.max);
    let pair = |x: f32, name: &str, value: &str, ink| {
        let mut right = x;
        if !name.is_empty() {
            let drawn = paint::text(
                painter,
                theme,
                line(x),
                Align2::LEFT_CENTER,
                name,
                label,
                color.text_secondary,
            );
            right = drawn.right() + tokens.space.s2 + tokens.space.s1;
        }
        paint::text(
            painter,
            theme,
            line(right),
            Align2::LEFT_CENTER,
            value,
            readout,
            ink,
        )
        .right()
    };
    let none = cx.t("header.none");
    match &view.audio.state {
        AudioState::Running(stream) => {
            painter.circle_filled(egui::pos2(x + 4.0, middle), 4.0, color.status_ok);
            let kind = cx.t(kind_key(stream.kind));
            let drawn = paint::text(
                painter,
                theme,
                line(x + 8.0 + tokens.space.s3),
                Align2::LEFT_CENTER,
                kind,
                label,
                color.text_primary,
            );
            x = drawn.right() + gap;
            let rate = cx.tf(
                "header.rate_value",
                &[("value", &format::kilohertz(stream.sample_rate))],
            );
            x = pair(x, "", &rate, color.text_primary) + gap;
            let latency = cx.tf(
                "header.latency_value",
                &[("value", &format!("{:.1}", stream.latency_ms))],
            );
            x = pair(x, cx.t("header.latency"), &latency, color.text_primary) + gap;
            let percent = (stream.load * 100.0).round().clamp(0.0, 999.0);
            let load = cx.tf("header.load_value", &[("value", &format!("{percent:.0}"))]);
            let ink = if stream.load > HIGH_LOAD {
                color.status_warn
            } else {
                color.text_primary
            };
            x = pair(x, cx.t("header.load"), &load, ink) + gap;
            dropouts(cx, x, rect, stream.dropouts);
        }
        AudioState::Stopped(problem) => {
            let icon = Rect::from_min_size(egui::pos2(x, middle - 8.0), egui::vec2(16.0, 16.0));
            theme
                .icons
                .paint(painter, Glyph::Error, icon, color.status_error);
            let (title, _) = problem_keys(*problem);
            let drawn = paint::text(
                painter,
                theme,
                line(icon.right() + tokens.space.s3),
                Align2::LEFT_CENTER,
                cx.t(title),
                label,
                color.text_primary,
            );
            x = drawn.right() + gap;
            for name in ["header.rate", "header.latency", "header.load"] {
                x = pair(x, cx.t(name), none, color.text_primary) + gap;
            }
        }
    }

    // The right end, right to left: preferences, clock, master level.
    let settings = Rect::from_min_size(
        egui::pos2(rect.right() - gap - 28.0, middle - 12.0),
        egui::vec2(28.0, 24.0),
    );
    key::key(
        cx,
        settings,
        id("header-settings"),
        KeySpec::icon(Glyph::Settings, 14.0, cx.t("header.settings")),
        Press::Click(
            AppCommand::SettingsOpen {
                page: SettingsPage::Audio,
            }
            .into(),
        ),
    );
    let painter = cx.painter();
    let mut right = settings.left() - gap;
    if let Some(clock) = view.clock {
        let text = format!("{:02}:{:02}", clock.hour, clock.minute);
        let area = Rect::from_min_max(rect.min, egui::pos2(right, rect.bottom()));
        let drawn = paint::text(
            painter,
            theme,
            area,
            Align2::RIGHT_CENTER,
            &text,
            readout,
            color.text_primary,
        );
        right = drawn.left() - gap;
        divider(right);
        right -= gap;
    }
    let meter_rect = Rect::from_min_size(
        egui::pos2(right - 96.0, middle - 5.0),
        egui::vec2(96.0, 10.0),
    );
    meter::meter(
        cx,
        meter_rect,
        &MeterSpec {
            levels: view.engine.master_levels,
            memory: state.meters[2],
            segments: MASTER_METER_SEGMENTS,
            vertical: false,
        },
    );
    let area = Rect::from_min_max(
        rect.min,
        egui::pos2(meter_rect.left() - tokens.space.s3 - 3.0, rect.bottom()),
    );
    paint::text(
        painter,
        theme,
        area,
        Align2::RIGHT_CENTER,
        cx.t("header.master"),
        label,
        color.text_secondary,
    );
}

/// The dropout count: a reading that resets the count when it is operated.
fn dropouts(cx: &mut Cx<'_>, x: f32, header: Rect, count: u64) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let line = Rect::from_min_max(egui::pos2(x, header.top()), header.max);
    let name = paint::text(
        painter,
        theme,
        line,
        Align2::LEFT_CENTER,
        cx.t("header.dropouts"),
        tokens.text.label,
        color.text_secondary,
    );
    let ink = if count > 0 {
        color.status_warn
    } else {
        color.text_primary
    };
    let value = Rect::from_min_max(
        egui::pos2(
            name.right() + tokens.space.s2 + tokens.space.s1,
            header.top(),
        ),
        header.max,
    );
    let value = paint::text(
        painter,
        theme,
        value,
        Align2::LEFT_CENTER,
        &count.to_string(),
        tokens.text.readout,
        ink,
    );
    let hit = name
        .union(value)
        .expand2(egui::vec2(tokens.space.s2, tokens.space.s2));
    let action = AppCommand::AudioDropoutsReset;
    cx.record(action.id(), hit, ControlKind::Button, true);
    let response = cx
        .ui
        .interact(hit, id("header-dropouts"), cx.sense(true, Sense::click()));
    let label = cx.tf("header.dropouts_name", &[("count", &count.to_string())]);
    button_info(&response, true, &label);
    if response.has_focus() {
        cx.out.claim(&[egui::Key::Space, egui::Key::Enter]);
        paint::focus_ring(painter, theme, hit, tokens.radius.md);
    }
    if response.clicked() {
        cx.out.emit(action);
    }
}

/// The notice under the header when no device plays: what happened, what it means, and a
/// key that opens the audio preferences.
pub fn audio_notice(cx: &mut Cx<'_>, rect: Rect, problem: AudioProblem) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect_filled(rect, CornerRadius::ZERO, color.surface_panel);
    let bar = Rect::from_min_size(rect.min, egui::vec2(tokens.stroke.led, rect.height()));
    painter.rect_filled(bar, CornerRadius::ZERO, color.status_error);
    let middle = rect.center().y;
    let icon = Rect::from_min_size(
        egui::pos2(rect.left() + 15.0, middle - 9.0),
        egui::vec2(18.0, 18.0),
    );
    theme
        .icons
        .paint(painter, Glyph::Error, icon, color.status_error);
    let (title, text) = problem_keys(problem);
    let label = cx.t("notice.open_audio_settings");
    let width = key::width_for(cx, label, 0.0);
    let button = Rect::from_min_size(
        egui::pos2(
            rect.right() - tokens.space.s4 - width,
            middle - tokens.size.button_h / 2.0,
        ),
        egui::vec2(width, tokens.size.button_h),
    );
    let line = Rect::from_min_max(
        egui::pos2(icon.right() + tokens.space.s4, rect.top()),
        rect.max,
    );
    let drawn = paint::text(
        painter,
        theme,
        line,
        Align2::LEFT_CENTER,
        cx.t(title),
        tokens.text.title,
        color.text_primary,
    );
    let rest = Rect::from_min_max(
        egui::pos2(drawn.right() + tokens.space.s4, rect.top()),
        egui::pos2(button.left() - tokens.space.s4, rect.bottom()),
    );
    paint::text_elided(
        painter,
        theme,
        rest,
        cx.t(text),
        tokens.text.body,
        color.text_secondary,
    );
    key::key(
        cx,
        button,
        id("notice-audio-settings"),
        KeySpec::text(label),
        Press::Click(
            AppCommand::SettingsOpen {
                page: SettingsPage::Audio,
            }
            .into(),
        ),
    );
}
