//! The status header across the top of the window, and the notice below it when no audio
//! device is playing.
//!
//! Components: `StatusHeader`, `MasterClock` and `Notice` in `docs/design/DESIGN.md`;
//! placement from `docs/design/screens/M2-main.html`, `Startup-states.html` and, for the
//! master clock, `First-version.html`.

use egui::{Align2, CornerRadius, Rect, Sense};
use platter_core::Bpm;
use platter_engine::{ClockMode, ClockSnapshot, Command};

use crate::command::{AppCommand, MeterId, SettingsPage};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::icons::Glyph;
use crate::model::{AudioProblem, AudioState, MidiView, OutputKind, View};
use crate::paint;
use crate::shell::ViewState;
use crate::widgets::choice::{self, Choice};
use crate::widgets::field::{self, Typing};
use crate::widgets::key::{self, KeySpec, Press};
use crate::widgets::meter::{self, MeterSpec};
use crate::widgets::value::{self, Steps, ValueSpec, button_info};

/// Load above which the reading turns to the warning colour.
const HIGH_LOAD: f32 = 0.7;
const MASTER_METER_SEGMENTS: usize = 24;
const CLOCK_KEY_HEIGHT: f32 = 24.0;
const CLOCK_ICON_WIDTH: f32 = 28.0;
const CLOCK_FIELD_WIDTH: f32 = 64.0;
/// Beats per minute the arrows of the tempo field add or take away.
const CLOCK_FIELD_STEP: f64 = 0.01;
/// Beats per minute the step keys add or take away.
const CLOCK_STEP: f64 = 1.0;
/// How much the bend keys change the tempo of the clock while held.
const CLOCK_BEND: f64 = 0.04;

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
            x = dropouts(cx, x, rect, stream.dropouts) + gap;
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
    if view.midi.named > 0 {
        x = midi(cx, x, rect, view.midi) + gap;
    }
    if view.engine.recorder.recording {
        x = super::recorder::indicator(cx, x, rect) + gap;
    }

    // The right end, right to left: preferences, full screen, clock, master level.
    let settings = Rect::from_min_size(
        egui::pos2(rect.right() - gap - 28.0, middle - 12.0),
        egui::vec2(28.0, 24.0),
    );
    let fullscreen = settings.translate(egui::vec2(-28.0 - tokens.space.s2, 0.0));
    let mut spec = KeySpec::icon(Glyph::Fullscreen, 14.0, cx.t("header.fullscreen"));
    spec.lit = view
        .settings
        .fullscreen
        .then_some(tokens.color.text_primary);
    key::key(
        cx,
        fullscreen,
        id("header-fullscreen"),
        spec,
        Press::Click(AppCommand::FullscreenToggle.into()),
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
    let mut right = fullscreen.left() - gap;
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
            memory: state.meters[MeterId::Master.index()],
            segments: MASTER_METER_SEGMENTS,
            vertical: false,
        },
    );
    let area = Rect::from_min_max(
        rect.min,
        egui::pos2(meter_rect.left() - tokens.space.s3 - 3.0, rect.bottom()),
    );
    let master = paint::text(
        painter,
        theme,
        area,
        Align2::RIGHT_CENTER,
        cx.t("header.master"),
        label,
        color.text_secondary,
    );
    let span = Rect::from_x_y_ranges(x..=master.left() - gap, rect.y_range());
    master_clock(cx, span, &view.engine.clock);
}

/// A piece of the master clock, by what decides its width.
enum Piece<'a> {
    Label(&'a str),
    Field,
    Modes([&'a str; 2]),
    Key(&'a str),
    Icon,
}

/// The master clock, centred in `span`: the tempo of the clock, Auto and Master, Tap, the
/// step and bend keys, Snap and Quantize. Behaviour note: `docs/behavior/sync.md`.
fn master_clock(cx: &mut Cx<'_>, span: Rect, clock: &ClockSnapshot) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let near = tokens.space.s2;
    let apart = tokens.space.s3;
    let modes = [
        texts.get("header.clock_auto"),
        texts.get("header.clock_master"),
    ];
    // Each piece with the space before it.
    let pieces = [
        (0.0, Piece::Label(texts.get("header.tempo"))),
        (apart, Piece::Field),
        (near, Piece::Icon),
        (apart, Piece::Modes(modes)),
        (apart, Piece::Key(texts.get("header.tap"))),
        (near, Piece::Icon),
        (near, Piece::Icon),
        (apart, Piece::Icon),
        (near, Piece::Icon),
        (apart, Piece::Key(texts.get("header.snap"))),
        (near, Piece::Key(texts.get("header.quantize"))),
    ];
    let painter = cx.painter();
    // As wide as `choice::segmented` makes each segment.
    let segment = |label: &str| {
        (paint::text_width(painter, theme, label, tokens.text.button) + 2.0 * tokens.space.s4)
            .ceil()
    };
    let widths = pieces.each_ref().map(|(_, piece)| match piece {
        Piece::Label(text) => paint::text_width(painter, theme, text, tokens.text.label),
        Piece::Field => CLOCK_FIELD_WIDTH,
        Piece::Modes(labels) => labels.iter().map(|label| segment(label)).sum::<f32>() + 2.0,
        Piece::Key(label) => key::width_for(cx, label, CLOCK_ICON_WIDTH),
        Piece::Icon => CLOCK_ICON_WIDTH,
    });
    let total: f32 = pieces
        .iter()
        .zip(&widths)
        .map(|((before, _), width)| before + width)
        .sum();
    let mut x = span.left() + ((span.width() - total) / 2.0).max(0.0);
    let top = span.center().y - CLOCK_KEY_HEIGHT / 2.0;
    let mut rects = [Rect::NOTHING; 11];
    for ((rect, (before, _)), width) in rects.iter_mut().zip(&pieces).zip(widths) {
        x += before;
        *rect = Rect::from_min_size(egui::pos2(x, top), egui::vec2(width, CLOCK_KEY_HEIGHT));
        x += width;
    }
    let [
        label,
        field,
        metronome,
        modes_rect,
        tap,
        down,
        up,
        slower,
        faster,
        snap,
        quantize,
    ] = rects;

    // The tempo is the clock's own only while the clock is the tempo master and follows no
    // received clock (`docs/behavior/midi-clock.md`, *Following*).
    let own = clock.master.is_none() && !clock.external;
    paint::text(
        painter,
        theme,
        label,
        Align2::LEFT_CENTER,
        texts.get("header.tempo"),
        tokens.text.label,
        color.text_secondary,
    );
    let range = Bpm::MAX - Bpm::MIN;
    let position = ((clock.bpm - Bpm::MIN) / range).clamp(0.0, 1.0);
    // A double click, or Enter on the field, types the tempo.
    let typed = if own {
        field::typing(
            cx,
            field,
            id("clock-tempo"),
            &format::bpm(clock.bpm),
            texts.get("header.tempo_name"),
            "clock.tempo.set",
        )
    } else {
        Typing::Idle
    };
    match typed {
        Typing::Idle => {
            value::field(
                cx,
                field,
                id("clock-tempo"),
                ValueSpec {
                    name: texts.get("header.tempo_name"),
                    position,
                    default: position,
                    steps: Steps {
                        small: CLOCK_FIELD_STEP / range,
                        fine: CLOCK_FIELD_STEP / 10.0 / range,
                        large: 1.0 / range,
                    },
                    enabled: own,
                },
                &format::bpm(clock.bpm),
                |position| {
                    Command::ClockTempoSet {
                        bpm: Bpm::MIN + position * range,
                    }
                    .into()
                },
            );
        }
        Typing::Open => {}
        Typing::Confirmed(text) => {
            if let Some(bpm) = format::typed_bpm(&text) {
                cx.out.emit(Command::ClockTempoSet { bpm });
            }
        }
    }
    // The metronome clicks on every beat of the clock in the headphones.
    let spec = KeySpec::icon(Glyph::Metronome, 16.0, texts.get("header.metronome_name"))
        .lit(clock.metronome.then_some(color.text_primary));
    key::key(
        cx,
        metronome,
        id("clock-metronome"),
        spec,
        Press::Click(Command::ClockMetronomeToggle.into()),
    );
    // A clock that follows a received one is the tempo master: the mode does not change.
    let choices = modes.map(|label| Choice {
        label,
        enabled: !clock.external,
    });
    choice::segmented(
        cx,
        modes_rect,
        id("clock-mode"),
        &choices,
        Some(usize::from(!clock.auto)),
        |index| {
            let mode = if index == 0 {
                ClockMode::Auto
            } else {
                ClockMode::Master
            };
            Command::ClockModeSet { mode }.into()
        },
    );
    key::key(
        cx,
        tap,
        id("clock-tap"),
        KeySpec::text(texts.get("header.tap")).enabled(own),
        Press::Click(Command::ClockTap.into()),
    );
    let steps = [
        (down, Glyph::Minus, "header.tempo_down_name", -CLOCK_STEP),
        (up, Glyph::Plus, "header.tempo_up_name", CLOCK_STEP),
    ];
    for (index, (rect, glyph, name, amount)) in steps.into_iter().enumerate() {
        key::key(
            cx,
            rect,
            id(("clock-step", index)),
            KeySpec::icon(glyph, 14.0, texts.get(name)).enabled(own),
            Press::Click(Command::ClockTempoStep { amount }.into()),
        );
    }
    let bends = [
        (
            slower,
            Glyph::ChevronLeft,
            "header.bend_slower_name",
            -CLOCK_BEND,
        ),
        (
            faster,
            Glyph::ChevronRight,
            "header.bend_faster_name",
            CLOCK_BEND,
        ),
    ];
    for (index, (rect, glyph, name, amount)) in bends.into_iter().enumerate() {
        key::key(
            cx,
            rect,
            id(("clock-bend", index)),
            KeySpec::icon(glyph, 14.0, texts.get(name)).enabled(own),
            Press::Hold {
                down: Command::ClockBend { amount }.into(),
                up: Command::ClockBend { amount: 0.0 }.into(),
            },
        );
    }
    let lit = color.text_primary;
    key::key(
        cx,
        snap,
        id("clock-snap"),
        KeySpec::text(texts.get("header.snap")).lit(clock.snap.then_some(lit)),
        Press::Click(Command::GlobalSnapToggle.into()),
    );
    key::key(
        cx,
        quantize,
        id("clock-quantize"),
        KeySpec::text(texts.get("header.quantize")).lit(clock.quantize.then_some(lit)),
        Press::Click(Command::GlobalQuantizeToggle.into()),
    );
}

/// The dropout count: a reading that resets the count when it is operated. Returns where it
/// ends.
/// The MIDI indicator: a dot for how many of the controllers named are open, the label that
/// lights while messages come, and the count. Returns where the next indicator starts.
fn midi(cx: &Cx<'_>, x: f32, header: Rect, midi: MidiView) -> f32 {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    let middle = header.center().y;
    let dot = if midi.open == 0 {
        color.status_error
    } else if midi.open < midi.named {
        color.status_warn
    } else {
        color.status_ok
    };
    painter.circle_filled(egui::pos2(x + 4.0, middle), 4.0, dot);
    let ink = if midi.active {
        color.text_primary
    } else {
        color.text_secondary
    };
    let line = |x: f32| Rect::from_min_max(egui::pos2(x, header.top()), header.max);
    let name = paint::text(
        painter,
        theme,
        line(x + 8.0 + tokens.space.s3),
        Align2::LEFT_CENTER,
        cx.t("header.midi"),
        tokens.text.label,
        ink,
    );
    let count = cx.tf(
        "header.midi_value",
        &[
            ("open", &midi.open.to_string()),
            ("named", &midi.named.to_string()),
        ],
    );
    paint::text(
        painter,
        theme,
        line(name.right() + tokens.space.s2 + tokens.space.s1),
        Align2::LEFT_CENTER,
        &count,
        tokens.text.readout,
        color.text_primary,
    )
    .right()
}

fn dropouts(cx: &mut Cx<'_>, x: f32, header: Rect, count: u64) -> f32 {
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
    hit.right()
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
