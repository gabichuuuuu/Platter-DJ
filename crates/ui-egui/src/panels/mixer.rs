//! The mixer: a channel per deck, the master section, the headphone controls and the
//! crossfader.
//!
//! Components: `Knob`, `Fader`, `Crossfader` and `Meter` in `docs/design/DESIGN.md`;
//! placement from `docs/design/screens/M2-main.html`. Behaviour notes:
//! `docs/behavior/mixer.md` and `docs/behavior/master.md`.

use egui::{Align2, CornerRadius, Id, Rect, Role, Sense, WidgetInfo};
use platter_core::DeckId;
use platter_dsp::eq::band_gain;
use platter_engine::{Band, Command, FADER_UNITY};

use crate::command::{Action, AppCommand, MeterId};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::icons::Glyph;
use crate::model::View;
use crate::paint;
use crate::panels::waveform::deck_color;
use crate::shell::ViewState;
use crate::widgets::key::{self, Face, KeySpec, Press};
use crate::widgets::meter::{self, MeterSpec, scale_position};
use crate::widgets::value::{self, FADER_WIDTH, FaderSpec, KnobSpec, Steps, ValueSpec};

/// Gain at the bottom of a gain knob, just above silence, and at its top.
const KNOB_FLOOR_DB: f64 = -36.0;
const KNOB_TOP_DB: f64 = 12.0;
/// Decibels below full scale the master meter labels.
const SCALE_MARKS: [f32; 5] = [0.0, 6.0, 12.0, 24.0, 48.0];
const CHANNEL_SEGMENTS: usize = 26;
const MASTER_SEGMENTS: usize = 30;

/// Position of a gain knob for a gain in decibels.
fn gain_position(db: f64) -> f64 {
    ((db - KNOB_FLOOR_DB) / (KNOB_TOP_DB - KNOB_FLOOR_DB)).clamp(0.0, 1.0)
}

/// Gain in decibels of a knob position; the very bottom is silence.
fn gain_db(position: f64) -> f64 {
    if position <= 0.0 {
        f64::NEG_INFINITY
    } else {
        KNOB_FLOOR_DB + position * (KNOB_TOP_DB - KNOB_FLOOR_DB)
    }
}

struct KnobCell<'a> {
    /// Translation key of the name under the knob.
    name: &'a str,
    /// The value in words, shown in place of the name while the knob is in use.
    reading: String,
    position: f64,
    default: f64,
    centre_detent: bool,
    hide_arc: bool,
    enabled: bool,
    size: f32,
}

/// A knob with its name below. `origin` is the top left corner of the knob.
fn knob_cell(
    cx: &mut Cx<'_>,
    origin: egui::Pos2,
    id: Id,
    cell: KnobCell<'_>,
    make: impl FnOnce(f64) -> Action,
) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let name = cx.texts.get(cell.name);
    let spoken = format!("{name}, {}", cell.reading);
    let rect = Rect::from_min_size(origin, egui::vec2(cell.size, cell.size));
    let response = value::knob(
        cx,
        rect,
        id,
        KnobSpec {
            value: ValueSpec {
                name: &spoken,
                position: cell.position,
                default: cell.default,
                steps: Steps::PERCENT,
                enabled: cell.enabled,
            },
            size: cell.size,
            centre_detent: cell.centre_detent,
            hide_arc: cell.hide_arc,
        },
        make,
    );
    // The name, or the value while the knob is touched or has the keyboard.
    let in_use = cell.enabled && (response.hovered() || response.dragged() || response.has_focus());
    let (text, style, ink) = if in_use {
        (
            cell.reading.as_str(),
            tokens.text.readout,
            color.text_primary,
        )
    } else if cell.enabled {
        (name, tokens.text.label, color.text_secondary)
    } else {
        (name, tokens.text.label, color.text_disabled)
    };
    let label = Rect::from_min_size(
        egui::pos2(rect.center().x - 40.0, rect.bottom() + 1.0),
        egui::vec2(80.0, 14.0),
    );
    paint::text(
        cx.painter(),
        theme,
        label,
        Align2::CENTER_CENTER,
        text,
        style,
        ink,
    );
}

fn db_reading(cx: &Cx<'_>, db: f64) -> String {
    if db.is_finite() && db > KNOB_FLOOR_DB - 0.01 {
        cx.texts
            .format("mixer.db_value", &[("value", &format::decibels(db))])
    } else {
        cx.texts.get("mixer.silence_value").to_owned()
    }
}

/// A meter with its clip indicator above and, while it shows, the word below or beside it.
fn meter_with_clip(cx: &mut Cx<'_>, rect: Rect, which: MeterId, spec: MeterSpec, label_at: Rect) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let clipped = spec.memory.is_clipped();
    meter::meter(cx, rect, &spec);
    let painter = cx.painter();
    let cell = Rect::from_min_size(
        rect.left_top() - egui::vec2(0.0, 6.0),
        egui::vec2(rect.width(), 4.0),
    );
    let ink = if clipped {
        color.meter_clip
    } else {
        color.surface_control
    };
    painter.rect_filled(cell, paint::radius(tokens.radius.sm), ink);
    let clip = cx.texts.get("mixer.clip");
    if clipped {
        paint::text(
            painter,
            theme,
            label_at,
            Align2::CENTER_CENTER,
            clip,
            tokens.text.readout,
            color.meter_clip,
        );
    }
    // The whole meter resets the indicator; it is only a control while there is one to reset.
    let hit = rect.union(cell);
    let action = AppCommand::MixerClipReset { meter: which };
    cx.record(action.id(), hit, ControlKind::Button, clipped);
    let response = cx.ui.interact(
        hit,
        id(("meter", which.index())),
        cx.sense(clipped, Sense::click()),
    );
    let name = cx.texts.get("mixer.level");
    response.widget_info(|| WidgetInfo::labeled(Role::Meter, true, name));
    if response.has_focus() {
        cx.out.claim(&[egui::Key::Space, egui::Key::Enter]);
        paint::focus_ring(painter, theme, hit, tokens.radius.sm);
    }
    if response.clicked() {
        cx.out.emit(action);
    }
}

/// Where the parts of a channel sit, as offsets from the left of the mixer panel. Channel B
/// is the mirror image of channel A.
struct ChannelLayout {
    /// Left of the gain and equaliser knobs, and of their kill keys.
    knobs: f32,
    kills: f32,
    /// Left of the filter and pan knobs and of the pre-listen key.
    side: f32,
    fader: f32,
    meter: f32,
    badge: f32,
}

const CHANNEL_A: ChannelLayout = ChannelLayout {
    knobs: 16.0,
    kills: 64.0,
    side: 108.0,
    fader: 103.0,
    meter: 143.0,
    badge: 45.0,
};

const CHANNEL_B: ChannelLayout = ChannelLayout {
    knobs: 310.0,
    kills: 358.0,
    side: 253.0,
    fader: 248.0,
    meter: 288.0,
    badge: 281.0,
};

fn channel(cx: &mut Cx<'_>, panel: Rect, deck: DeckId, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let layout = match deck {
        DeckId::A => CHANNEL_A,
        DeckId::B => CHANNEL_B,
    };
    let at = |x: f32, y: f32| panel.min + egui::vec2(x, y);
    let snapshot = &view.engine.decks[deck.index()];
    let controls = &snapshot.controls;
    let letter = deck.letter().to_string();
    let knob = tokens.size.knob;
    let index = deck.index();

    let badge = Rect::from_min_size(at(layout.badge, 12.0), egui::vec2(18.0, 18.0));
    paint::badge(
        cx.painter(),
        theme,
        badge,
        deck.letter(),
        deck_color(theme, deck),
    );
    let title = texts.format("mixer.channel", &[("deck", &letter)]);
    let title_rect = Rect::from_min_size(at(layout.badge + 24.0, 12.0), egui::vec2(90.0, 18.0));
    paint::text(
        cx.painter(),
        theme,
        title_rect,
        Align2::LEFT_CENTER,
        &title,
        tokens.text.label,
        color.text_secondary,
    );

    knob_cell(
        cx,
        at(layout.knobs, 36.0),
        id(("mixer-gain", index)),
        KnobCell {
            name: "mixer.gain",
            reading: db_reading(cx, controls.gain_db),
            position: gain_position(controls.gain_db),
            default: gain_position(0.0),
            centre_detent: false,
            hide_arc: false,
            enabled: true,
            size: knob,
        },
        |position| {
            Command::MixerGainSet {
                deck,
                db: gain_db(position),
            }
            .into()
        },
    );
    let bands = [
        (Band::High, "mixer.high", 97.0),
        (Band::Mid, "mixer.mid", 158.0),
        (Band::Low, "mixer.low", 219.0),
    ];
    for (band, name, top) in bands {
        let position = controls.eq[band as usize];
        let killed = controls.eq_kill[band as usize];
        let reading = if killed || position <= 0.0 {
            texts.get("mixer.kill_value").to_owned()
        } else {
            db_reading(cx, 20.0 * f64::from(band_gain(position as f32)).log10())
        };
        knob_cell(
            cx,
            at(layout.knobs, top),
            id(("mixer-eq", index, band as usize)),
            KnobCell {
                name,
                reading,
                position,
                default: 0.5,
                centre_detent: true,
                hide_arc: killed,
                enabled: true,
                size: knob,
            },
            |position| {
                Command::MixerEqSet {
                    deck,
                    band,
                    position,
                }
                .into()
            },
        );
        let kill = texts.get("mixer.kill");
        let width = key::width_for(cx, kill, 28.0).clamp(28.0, 34.0);
        let rect = Rect::from_min_size(at(layout.kills, top + 19.0), egui::vec2(width, 18.0));
        let name = texts.format(
            "mixer.kill_name",
            &[("band", texts.get(name)), ("deck", &letter)],
        );
        let spec = KeySpec {
            face: Face::Text(kill),
            name: &name,
            lit: killed.then_some(color.text_primary),
            held: None,
            enabled: true,
            keeps_led: false,
            text: Some(tokens.text.label),
        };
        key::key(
            cx,
            rect,
            id(("mixer-kill", index, band as usize)),
            spec,
            Press::Click(Command::MixerEqKillToggle { deck, band }.into()),
        );
    }

    let filter = controls.filter;
    let reading = if filter.abs() < 0.005 {
        texts.get("mixer.filter_off").to_owned()
    } else {
        let key = if filter < 0.0 {
            "mixer.filter_low_pass"
        } else {
            "mixer.filter_high_pass"
        };
        texts.format(key, &[("value", &format!("{:.0}", filter.abs() * 100.0))])
    };
    knob_cell(
        cx,
        at(layout.side, 36.0),
        id(("mixer-filter", index)),
        KnobCell {
            name: "mixer.filter",
            reading,
            position: (filter + 1.0) / 2.0,
            default: 0.5,
            centre_detent: true,
            hide_arc: false,
            enabled: true,
            size: knob,
        },
        |position| {
            Command::MixerFilterSet {
                deck,
                position: position * 2.0 - 1.0,
            }
            .into()
        },
    );
    let pan = controls.pan;
    let reading = if pan.abs() < 0.005 {
        texts.get("mixer.pan_centre").to_owned()
    } else {
        let key = if pan < 0.0 {
            "mixer.pan_left"
        } else {
            "mixer.pan_right"
        };
        texts.format(key, &[("value", &format!("{:.0}", pan.abs() * 100.0))])
    };
    knob_cell(
        cx,
        at(layout.side, 99.0),
        id(("mixer-pan", index)),
        KnobCell {
            name: "mixer.pan",
            reading,
            position: (pan + 1.0) / 2.0,
            default: 0.5,
            centre_detent: true,
            hide_arc: false,
            enabled: true,
            size: knob,
        },
        |position| {
            Command::MixerPanSet {
                deck,
                position: position * 2.0 - 1.0,
            }
            .into()
        },
    );

    let headphones = view.audio.has_headphones();
    let name = texts.format("mixer.pre_listen", &[("deck", &letter)]);
    let spec = KeySpec {
        face: Face::Icon(Glyph::Headphones, 16.0),
        name: &name,
        lit: controls.pre_listen.then_some(deck_color(theme, deck)),
        held: None,
        enabled: headphones,
        keeps_led: false,
        text: None,
    };
    key::key(
        cx,
        Rect::from_min_size(at(layout.side, 162.0), egui::vec2(40.0, 24.0)),
        id(("mixer-pre-listen", index)),
        spec,
        Press::Click(Command::MixerCueToggle { deck }.into()),
    );

    let name = texts.format("mixer.fader", &[("deck", &letter)]);
    let track = Rect::from_min_size(at(layout.fader, 196.0), egui::vec2(FADER_WIDTH, 108.0));
    value::fader(
        cx,
        track,
        id(("mixer-fader", index)),
        FaderSpec {
            value: ValueSpec {
                name: &name,
                position: controls.fader,
                default: FADER_UNITY,
                steps: Steps::PERCENT,
                enabled: true,
            },
            mark: FADER_UNITY,
        },
        |position| Command::MixerFaderSet { deck, position }.into(),
    );
    let meter_rect = Rect::from_min_size(at(layout.meter, 196.0), egui::vec2(10.0, 108.0));
    let label = Rect::from_center_size(
        egui::pos2(meter_rect.center().x, meter_rect.bottom() + 9.0),
        egui::vec2(40.0, 14.0),
    );
    meter_with_clip(
        cx,
        meter_rect,
        MeterId::Deck(deck),
        MeterSpec {
            levels: snapshot.levels,
            memory: state.meters[index],
            segments: CHANNEL_SEGMENTS,
            vertical: true,
        },
        label,
    );
}

fn master(cx: &mut Cx<'_>, panel: Rect, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let at = |x: f32, y: f32| panel.min + egui::vec2(x, y);
    let mixer = &view.engine.mixer;
    let centre = 200.0;
    let painter = cx.painter();
    let heading = Rect::from_center_size(at(centre, 19.0), egui::vec2(88.0, 14.0));
    paint::text(
        painter,
        theme,
        heading,
        Align2::CENTER_CENTER,
        texts.get("mixer.master"),
        tokens.text.label,
        color.text_secondary,
    );

    let meter_rect = Rect::from_min_size(at(205.0, 36.0), egui::vec2(12.0, 124.0));
    for mark in SCALE_MARKS {
        let y = meter_rect.bottom() - scale_position(-mark) * meter_rect.height();
        let y = y.clamp(meter_rect.top() + 6.0, meter_rect.bottom() - 6.0);
        let label = Rect::from_min_max(
            egui::pos2(meter_rect.left() - 30.0, y - 7.0),
            egui::pos2(meter_rect.left() - tokens.space.s2 - 2.0, y + 7.0),
        );
        paint::text(
            painter,
            theme,
            label,
            Align2::RIGHT_CENTER,
            &format!("{mark:.0}"),
            tokens.text.readout,
            color.text_secondary,
        );
    }
    let clip_label = Rect::from_min_size(at(219.0, 28.0), egui::vec2(28.0, 14.0));
    meter_with_clip(
        cx,
        meter_rect,
        MeterId::Master,
        MeterSpec {
            levels: view.engine.master_levels,
            memory: state.meters[2],
            segments: MASTER_SEGMENTS,
            vertical: true,
        },
        clip_label,
    );

    knob_cell(
        cx,
        at(centre - 20.0, 170.0),
        id("mixer-master-level"),
        KnobCell {
            name: "mixer.level",
            reading: db_reading(cx, mixer.master_gain_db),
            position: gain_position(mixer.master_gain_db),
            default: gain_position(0.0),
            centre_detent: false,
            hide_arc: false,
            enabled: true,
            size: tokens.size.knob,
        },
        |position| {
            Command::MasterGainSet {
                db: gain_db(position),
            }
            .into()
        },
    );

    let painter = cx.painter();
    painter.hline(
        panel.left() + centre - 20.0..=panel.left() + centre + 20.0,
        panel.top() + 235.0,
        paint::hairline(theme, color.line_divider),
    );
    let headphones = view.audio.has_headphones();
    let ink = if headphones {
        color.text_secondary
    } else {
        color.text_disabled
    };
    let phones = texts.get("mixer.phones");
    let width =
        14.0 + tokens.space.s2 + paint::text_width(painter, theme, phones, tokens.text.label);
    let left = panel.left() + centre - width / 2.0;
    let icon = Rect::from_min_size(
        egui::pos2(left, panel.top() + 246.0),
        egui::vec2(14.0, 14.0),
    );
    theme.icons.paint(painter, Glyph::Headphones, icon, ink);
    let label = Rect::from_min_size(
        egui::pos2(icon.right() + tokens.space.s2, icon.top()),
        egui::vec2(80.0, 14.0),
    );
    paint::text(
        painter,
        theme,
        label,
        Align2::LEFT_CENTER,
        phones,
        tokens.text.label,
        ink,
    );

    let mix = mixer.headphone_mix;
    let reading = if mix <= 0.005 {
        texts.get("mixer.mix_cue").to_owned()
    } else if mix >= 0.995 {
        texts.get("mixer.mix_master").to_owned()
    } else {
        texts.format(
            "mixer.mix_value",
            &[("value", &format!("{:.0}", mix * 100.0))],
        )
    };
    let small = 36.0;
    knob_cell(
        cx,
        at(centre - 40.0, 270.0),
        id("mixer-phones-mix"),
        KnobCell {
            name: "mixer.mix",
            reading,
            position: mix,
            default: 0.0,
            centre_detent: false,
            hide_arc: false,
            enabled: headphones,
            size: small,
        },
        |position| Command::MixerCueMixSet { position }.into(),
    );
    knob_cell(
        cx,
        at(centre + 4.0, 270.0),
        id("mixer-phones-volume"),
        KnobCell {
            name: "mixer.vol",
            reading: db_reading(cx, mixer.headphone_gain_db),
            position: gain_position(mixer.headphone_gain_db),
            default: gain_position(0.0),
            centre_detent: false,
            hide_arc: false,
            enabled: headphones,
            size: small,
        },
        |position| {
            Command::MixerCueVolumeSet {
                db: gain_db(position),
            }
            .into()
        },
    );
}

pub fn mixer(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    cx.painter()
        .rect_filled(rect, CornerRadius::ZERO, tokens.color.surface_panel);
    for deck in DeckId::ALL {
        channel(cx, rect, deck, view, state);
    }
    master(cx, rect, view, state);

    let at = |x: f32, y: f32| rect.min + egui::vec2(x, y);
    let track = Rect::from_min_size(
        at(118.0, 329.0),
        egui::vec2(tokens.size.crossfader_track, 28.0),
    );
    for (deck, x) in [(DeckId::A, 88.0), (DeckId::B, 290.0)] {
        let badge = Rect::from_min_size(at(x, 334.0), egui::vec2(18.0, 18.0));
        paint::badge(
            cx.painter(),
            theme,
            badge,
            deck.letter(),
            deck_color(theme, deck),
        );
    }
    let name = cx.texts.get("mixer.crossfader");
    value::crossfader(
        cx,
        track,
        id("mixer-crossfader"),
        ValueSpec {
            name,
            position: view.engine.mixer.crossfader,
            default: 0.5,
            steps: Steps::PERCENT,
            enabled: true,
        },
        |position| Command::MixerCrossfaderSet { position }.into(),
    );
}

#[cfg(test)]
mod tests {
    use super::{gain_db, gain_position};

    #[test]
    fn a_gain_knob_covers_silence_to_twelve_decibels_with_unity_at_three_quarters() {
        assert_eq!(gain_position(0.0), 0.75);
        assert_eq!(gain_position(12.0), 1.0);
        assert_eq!(gain_position(-90.0), 0.0);
        assert_eq!(gain_db(0.75), 0.0);
        assert_eq!(gain_db(1.0), 12.0);
        assert_eq!(gain_db(0.0), f64::NEG_INFINITY);
        assert!((gain_db(gain_position(-7.5)) + 7.5).abs() < 1e-9);
    }
}
