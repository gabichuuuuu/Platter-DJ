//! The mixer: a channel per deck, the master section, the headphone controls and the
//! crossfader.
//!
//! Components: `Knob`, `Fader`, `Crossfader` and `Meter` in `docs/design/DESIGN.md`;
//! placement from `docs/design/screens/M2-main.html`. Behaviour notes:
//! `docs/behavior/mixer.md` and `docs/behavior/master.md`.

use egui::{Align2, Color32, CornerRadius, Id, Pos2, Rect, Role, Sense, Vec2, WidgetInfo};
use platter_core::DeckId;
use platter_dsp::eq::band_gain;
use platter_engine::{
    Band, Command, CrossfaderDirection, CrossfaderSide, EffectKind, FADER_UNITY, FxUnit, MixerMode,
};

use crate::command::DeckLayout;
use crate::command::{Action, AppCommand, MeterId};
use crate::cx::{ControlKind, Cx, id};
use crate::format;
use crate::icons::Glyph;
use crate::model::View;
use crate::paint;
use crate::panels::waveform::deck_color;
use crate::shell::ViewState;
use crate::tokens::{TextStyle, Tokens};
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::key::{self, Face, KeySpec, Press};
use crate::widgets::meter::{self, MeterSpec, scale_position};
use crate::widgets::value::{
    self, CHANNEL_FADER, CHANNEL_FADER_WIDTH, FaderSpec, KnobSpec, Steps, ValueSpec,
};

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

/// What goes under a knob: its name, or its value while it is touched or has the keyboard.
struct KnobLabel {
    in_use: bool,
    text: String,
    style: TextStyle,
    ink: Color32,
    /// The row under the knob, centred on it.
    rect: Rect,
}

impl KnobLabel {
    fn paint(&self, cx: &Cx<'_>, rect: Rect) {
        paint::text(
            cx.painter(),
            cx.theme,
            rect,
            Align2::CENTER_CENTER,
            &self.text,
            self.style,
            self.ink,
        );
    }
}

/// A knob without its label. `origin` is the top left corner of the knob.
fn knob(
    cx: &mut Cx<'_>,
    origin: egui::Pos2,
    id: Id,
    cell: KnobCell<'_>,
    make: impl FnOnce(f64) -> Action,
) -> KnobLabel {
    let tokens = &cx.theme.tokens;
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
    let in_use = cell.enabled && (response.hovered() || response.dragged() || response.has_focus());
    let (text, style, ink) = if in_use {
        (cell.reading, tokens.text.readout, color.text_primary)
    } else if cell.enabled {
        (name.to_owned(), tokens.text.label, color.text_secondary)
    } else {
        (name.to_owned(), tokens.text.label, color.text_disabled)
    };
    KnobLabel {
        in_use,
        text,
        style,
        ink,
        rect: Rect::from_min_size(
            egui::pos2(rect.center().x - 40.0, rect.bottom() + 1.0),
            egui::vec2(80.0, 14.0),
        ),
    }
}

/// A knob with its label below.
fn knob_cell(
    cx: &mut Cx<'_>,
    origin: egui::Pos2,
    id: Id,
    cell: KnobCell<'_>,
    make: impl FnOnce(f64) -> Action,
) {
    let label = knob(cx, origin, id, cell, make);
    label.paint(cx, label.rect);
}

fn db_reading(cx: &Cx<'_>, db: f64) -> String {
    if db.is_finite() && db > KNOB_FLOOR_DB - 0.01 {
        cx.texts
            .format("mixer.db_value", &[("value", &format::decibels(db))])
    } else {
        cx.texts.get("mixer.silence_value").to_owned()
    }
}

/// A meter with its clip indicator above and, while it shows, the CLIP word in `label_at`.
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

/// Distance between the left edges of channel A and channel B. Both channels have the same
/// layout, side by side, with the master and the headphones in a column to their right.
const CHANNEL_STEP: f32 = 156.0;
/// Left edge of the master column, where its divider runs, and the centre of its controls.
const MASTER_LEFT: f32 = 324.0;
const MASTER_CENTRE: f32 = 356.0;
/// Sizes the reference screen gives the knobs that are not gain or equaliser knobs.
const FILTER_KNOB: f32 = 44.0;
const SMALL_KNOB: f32 = 24.0;
const MASTER_KNOB: f32 = 36.0;
/// Tops of the high, mid and low knobs; their kill keys and the channel meter follow them.
const EQ_TOPS: [f32; 3] = [95.0, 153.0, 211.0];
/// The two assign keys of a channel, under its fader.
const ASSIGN_WIDTH: f32 = 26.0;
const ASSIGN_STEP: f32 = 28.0;
/// The keys beside the crossfader.
const FADE_STEP_WIDTH: f32 = 24.0;
const FADE_AUTO_WIDTH: f32 = 44.0;

/// Where the controls of a channel sit, from the top left corner of the channel. Knob
/// positions are the top left corners of the knobs.
struct ChannelLayout {
    badge: Vec2,
    /// The channel is named beside its badge.
    title: bool,
    gain: Vec2,
    /// Diameter of the gain and equaliser knobs.
    knob: f32,
    pre_listen: Rect,
    /// The key that restores the autogain value, beside the gain knob, when the channel has one.
    auto: Option<Rect>,
    /// The mixer effect knob and the keys of effect units 1 and 2, when the channel has them.
    effects: Option<ChannelEffects>,
    key: Vec2,
    eq: [Vec2; 3],
    /// Where each kill key sits from the top left corner of its knob.
    kill: Vec2,
    meter: Rect,
    filter: Vec2,
    filter_size: f32,
    pan: Vec2,
    /// The left assign key, the step to the right one, and the width of each.
    assign: Vec2,
    assign_step: f32,
    assign_width: f32,
    fader: Vec2,
    fader_height: f32,
}

impl ChannelLayout {
    /// Two channels side by side, as `docs/design/screens/M2-main.html` places them.
    fn two(tokens: &Tokens) -> Self {
        let knob = tokens.size.knob;
        Self {
            badge: egui::vec2(50.0, 12.0),
            title: true,
            gain: egui::vec2(16.0, 35.0),
            knob,
            pre_listen: Rect::from_min_size(egui::pos2(108.0, 143.0), egui::vec2(54.0, 22.0)),
            auto: None,
            effects: None,
            key: egui::vec2(24.0, 272.0),
            eq: EQ_TOPS.map(|top| egui::vec2(16.0, top)),
            kill: egui::vec2(47.0, 19.0),
            // The meter runs beside the equaliser, from the top of the high knob to the name
            // of the low one; its clip cell takes the first 6px.
            meter: Rect::from_min_max(
                egui::pos2(94.0, EQ_TOPS[0] + 6.0),
                egui::pos2(104.0, EQ_TOPS[2] + knob + 15.0),
            ),
            filter: egui::vec2(113.0, 35.0),
            filter_size: FILTER_KNOB,
            pan: egui::vec2(123.0, 99.0),
            assign: egui::vec2(108.0, 294.0),
            assign_step: ASSIGN_STEP,
            assign_width: ASSIGN_WIDTH,
            fader: egui::vec2(112.0, 170.0),
            fader_height: tokens.size.fader_track,
        }
    }

    /// One of four narrow channels, as `docs/design/screens/M4-effects.html` places them.
    fn four() -> Self {
        Self {
            badge: egui::vec2(29.0, 0.0),
            title: false,
            gain: egui::vec2(6.0, 23.0),
            knob: 30.0,
            pre_listen: Rect::from_min_size(egui::pos2(9.0, 371.0), egui::vec2(58.0, 20.0)),
            auto: Some(Rect::from_min_size(
                egui::pos2(40.0, 37.0),
                egui::vec2(31.0, 18.0),
            )),
            effects: Some(ChannelEffects {
                knob: egui::pos2(12.0, 277.0),
                keys: egui::pos2(49.0, 280.0),
            }),
            key: egui::vec2(8.0, 327.0),
            eq: [73.0, 121.0, 169.0].map(|top| egui::vec2(2.0, top)),
            kill: egui::vec2(34.0, 14.0),
            meter: Rect::from_min_size(egui::pos2(69.0, 79.0), egui::vec2(6.0, 135.0)),
            filter: egui::vec2(19.0, 219.0),
            filter_size: 38.0,
            pan: egui::vec2(44.0, 327.0),
            assign: egui::vec2(16.0, 497.0),
            assign_step: 22.0,
            assign_width: 20.0,
            fader: egui::vec2(15.0, 396.0),
            fader_height: 96.0,
        }
    }
}

/// Where the effect controls of a narrow channel sit, from its top left corner.
struct ChannelEffects {
    /// The top left corner of the 30 point mixer effect knob; its name goes under it.
    knob: Pos2,
    /// The top left corner of the key of unit 1; the key of unit 2 is under it.
    keys: Pos2,
}

const MIXER_EFFECT_KNOB: f32 = 30.0;
const ASSIGN_KEY: Vec2 = egui::vec2(20.0, 18.0);

/// The mixer effect knob with the name of its effect under it, which opens the list of the
/// mixer effects, and the keys that send the channel to effect units 1 and 2.
fn channel_effects(
    cx: &mut Cx<'_>,
    origin: Pos2,
    layout: &ChannelEffects,
    deck: DeckId,
    view: &View<'_>,
) {
    let texts = cx.texts;
    let tokens = &cx.theme.tokens;
    let letter = deck.letter().to_string();
    let index = deck.index();
    let mixer = view.engine.effects.mixer[index];
    let effect = texts.get(crate::panels::effects::effect_key(Some(mixer.effect)));
    let name = texts.format(
        "effects.mixer_name",
        &[("effect", effect), ("deck", &letter)],
    );
    let knob_rect = Rect::from_min_size(
        origin + layout.knob.to_vec2(),
        egui::vec2(MIXER_EFFECT_KNOB, MIXER_EFFECT_KNOB),
    );
    value::knob(
        cx,
        knob_rect,
        id(("mixer-effect", index)),
        KnobSpec {
            value: ValueSpec {
                name: &name,
                position: mixer.amount,
                default: 0.0,
                steps: Steps::PERCENT,
                enabled: true,
            },
            size: MIXER_EFFECT_KNOB,
            centre_detent: false,
            hide_arc: false,
        },
        |position| Command::MixerEffectSet { deck, position }.into(),
    );
    // The name of the effect, a key that looks like a label, opens the list.
    let labels: Vec<&str> = EffectKind::MIXER
        .iter()
        .map(|kind| texts.get(crate::panels::effects::effect_key(Some(*kind))))
        .collect();
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let select_name = texts.format("effects.mixer_select_name", &[("deck", &letter)]);
    let label_rect = Rect::from_min_size(
        egui::pos2(knob_rect.center().x - 20.0, knob_rect.bottom() + 1.0),
        egui::vec2(40.0, 14.0),
    );
    choice::label_select(
        cx,
        label_rect,
        id(("mixer-effect-select", index)),
        SelectSpec {
            name: &select_name,
            choices: &choices,
            current: EffectKind::MIXER
                .iter()
                .position(|kind| *kind == mixer.effect),
            enabled: true,
        },
        |choice| {
            let effect = EffectKind::MIXER
                .get(choice)
                .copied()
                .unwrap_or(EffectKind::Reverb);
            Command::MixerEffectSelect { deck, effect }.into()
        },
    );
    for (row, unit) in FxUnit::ALL.into_iter().take(2).enumerate() {
        let number = unit.number().to_string();
        let name = texts.format(
            "effects.assign_name",
            &[("deck", &letter), ("unit", &number)],
        );
        let mut spec = KeySpec::text(&number);
        spec.name = &name;
        spec.text = Some(tokens.text.label);
        spec.lit = view
            .engine
            .effects
            .is_assigned(deck, unit)
            .then_some(tokens.color.text_primary);
        key::key(
            cx,
            Rect::from_min_size(
                origin + layout.keys.to_vec2() + egui::vec2(0.0, 21.0 * row as f32),
                ASSIGN_KEY,
            ),
            id(("mixer-fx-assign", index, row)),
            spec,
            Press::Click(Command::MixerFxAssignToggle { deck, unit }.into()),
        );
    }
}

fn channel(
    cx: &mut Cx<'_>,
    origin: Pos2,
    layout: &ChannelLayout,
    deck: DeckId,
    (view, state): (&View<'_>, &ViewState),
) {
    if let Some(effects) = &layout.effects {
        channel_effects(cx, origin, effects, deck, view);
    }
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let at = |offset: Vec2| origin + offset;
    let snapshot = &view.engine.decks[deck.index()];
    let controls = &snapshot.controls;
    let letter = deck.letter().to_string();
    let knob = layout.knob;
    let index = deck.index();

    let badge = Rect::from_min_size(at(layout.badge), egui::vec2(18.0, 18.0));
    paint::badge(
        cx.painter(),
        theme,
        badge,
        deck.letter(),
        deck_color(theme, deck),
    );
    if layout.title {
        let title = texts.format("mixer.channel", &[("deck", &letter)]);
        let title_rect = Rect::from_min_size(
            badge.right_top() + egui::vec2(6.0, 0.0),
            egui::vec2(90.0, 18.0),
        );
        paint::text(
            cx.painter(),
            theme,
            title_rect,
            Align2::LEFT_CENTER,
            &title,
            tokens.text.label,
            color.text_secondary,
        );
    }

    knob_cell(
        cx,
        at(layout.gain),
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
    if let Some(auto) = layout.auto {
        // Lit while the gain is the autogain value of the track.
        let value = snapshot.autogain_db;
        let at_value = value.is_some_and(|db| (db - controls.gain_db).abs() < 0.05);
        let name = texts.format("mixer.auto_name", &[("deck", &letter)]);
        let mut spec = KeySpec::text(texts.get("mixer.fade_auto"))
            .enabled(value.is_some())
            .lit(at_value.then_some(color.text_primary));
        spec.name = &name;
        spec.text = Some(tokens.text.label);
        spec.keeps_led = true;
        key::key(
            cx,
            auto.translate(origin.to_vec2()),
            id(("mixer-auto", index)),
            spec,
            Press::Click(Command::MixerGainRestore { deck }.into()),
        );
    }
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
        layout.pre_listen.translate(origin.to_vec2()),
        id(("mixer-pre-listen", index)),
        spec,
        Press::Click(Command::MixerCueToggle { deck }.into()),
    );

    // The key of the deck in semitones, ±12; turning it turns key lock on.
    let shift = snapshot.key_shift;
    let reading = if shift.abs() < 0.5 {
        "0".to_owned()
    } else {
        format!("{shift:+.0}")
    };
    knob_cell(
        cx,
        at(layout.key),
        id(("mixer-key", index)),
        KnobCell {
            name: "mixer.key",
            reading,
            position: (shift + 12.0) / 24.0,
            default: 0.5,
            centre_detent: true,
            hide_arc: false,
            enabled: snapshot.track.is_some(),
            size: SMALL_KNOB,
        },
        |position| {
            Command::DeckKeySet {
                deck,
                semitones: (position * 24.0 - 12.0).round(),
            }
            .into()
        },
    );

    // In external mixer mode the hardware mixer does the rest: the channel keeps its gain,
    // its key and its pre-listen, as the design system collapses it.
    if view.engine.mixer.mode == MixerMode::External {
        return;
    }

    let bands = [
        (Band::High, "mixer.high", layout.eq[0]),
        (Band::Mid, "mixer.mid", layout.eq[1]),
        (Band::Low, "mixer.low", layout.eq[2]),
    ];
    for (band, name, spot) in bands {
        let position = controls.eq[band as usize];
        let killed = controls.eq_kill[band as usize];
        let reading = if killed || position <= 0.0 {
            texts.get("mixer.kill_value").to_owned()
        } else {
            db_reading(cx, 20.0 * f64::from(band_gain(position as f32)).log10())
        };
        knob_cell(
            cx,
            at(spot),
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
        let width = key::width_for(cx, kill, 28.0).clamp(28.0, 30.0);
        let rect = Rect::from_min_size(at(spot + layout.kill), egui::vec2(width, 18.0));
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

    let meter_rect = layout.meter.translate(origin.to_vec2());
    let label = Rect::from_center_size(
        egui::pos2(meter_rect.center().x, meter_rect.bottom() + 9.0),
        egui::vec2(28.0, 14.0),
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
        at(layout.filter),
        id(("mixer-filter", index)),
        KnobCell {
            name: "mixer.filter",
            reading,
            position: (filter + 1.0) / 2.0,
            default: 0.5,
            centre_detent: true,
            hide_arc: false,
            enabled: true,
            size: layout.filter_size,
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
        at(layout.pan),
        id(("mixer-pan", index)),
        KnobCell {
            name: "mixer.pan",
            reading,
            position: (pan + 1.0) / 2.0,
            default: 0.5,
            centre_detent: true,
            hide_arc: false,
            enabled: true,
            size: SMALL_KNOB,
        },
        |position| {
            Command::MixerPanSet {
                deck,
                position: position * 2.0 - 1.0,
            }
            .into()
        },
    );

    // The side of the crossfader the channel goes to: L, R, or neither (thru).
    let side = view.engine.mixer.crossfader_sides[index];
    let sides = [
        (
            CrossfaderSide::Left,
            "mixer.assign_left",
            "mixer.assign_left_name",
        ),
        (
            CrossfaderSide::Right,
            "mixer.assign_right",
            "mixer.assign_right_name",
        ),
    ];
    for (place, (this, label, name)) in sides.into_iter().enumerate() {
        let label = texts.get(label);
        let name = texts.format(name, &[("deck", &letter)]);
        let rect = Rect::from_min_size(
            at(layout.assign + egui::vec2(place as f32 * layout.assign_step, 0.0)),
            egui::vec2(layout.assign_width, 18.0),
        );
        let spec = KeySpec {
            face: Face::Text(label),
            name: &name,
            lit: (side == this).then_some(color.text_primary),
            held: None,
            enabled: true,
            keeps_led: false,
            text: Some(tokens.text.label),
        };
        // A lit side goes back to neither.
        let next = if side == this {
            CrossfaderSide::Thru
        } else {
            this
        };
        key::key(
            cx,
            rect,
            id(("mixer-assign", index, place)),
            spec,
            Press::Click(Command::MixerCrossfaderAssign { deck, side: next }.into()),
        );
    }

    let name = texts.format("mixer.fader", &[("deck", &letter)]);
    let track = Rect::from_min_size(
        at(layout.fader),
        egui::vec2(CHANNEL_FADER_WIDTH, layout.fader_height),
    );
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
            look: CHANNEL_FADER,
        },
        |position| Command::MixerFaderSet { deck, position }.into(),
    );
}

/// Where the parts of the master column sit, from the top left corner of the mixer.
struct MasterLayout {
    meter: Rect,
    level: Vec2,
    /// The line between the level and the headphones, its top.
    divider: f32,
    phones: f32,
    mix: Vec2,
    vol: Vec2,
    phones_knob: f32,
    /// The two headphone knobs sit side by side, so their values share one row.
    side_by_side: bool,
}

impl MasterLayout {
    /// The column of the two-deck mixer, as `docs/design/screens/M2-main.html` places it.
    const TWO: Self = Self {
        meter: Rect::from_min_max(egui::pos2(361.0, 38.0), egui::pos2(373.0, 156.0)),
        level: egui::vec2(MASTER_CENTRE - MASTER_KNOB / 2.0, 162.0),
        divider: 221.0,
        phones: 230.0,
        mix: egui::vec2(330.0, 250.0),
        vol: egui::vec2(359.0, 250.0),
        phones_knob: SMALL_KNOB,
        side_by_side: true,
    };

    /// The column of the four-deck mixer, as `docs/design/screens/M4-effects.html` places it.
    const FOUR: Self = Self {
        meter: Rect::from_min_max(egui::pos2(362.0, 32.0), egui::pos2(374.0, 142.0)),
        level: egui::vec2(MASTER_CENTRE - MASTER_KNOB / 2.0 + 2.0, 150.0),
        divider: 245.0,
        phones: 256.0,
        mix: egui::vec2(340.0, 278.0),
        vol: egui::vec2(340.0, 337.0),
        phones_knob: MASTER_KNOB,
        side_by_side: false,
    };
}

fn master(
    cx: &mut Cx<'_>,
    panel: Rect,
    layout: &MasterLayout,
    (view, state): (&View<'_>, &ViewState),
) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let at = |x: f32, y: f32| panel.min + egui::vec2(x, y);
    let mixer = &view.engine.mixer;
    let centre = MASTER_CENTRE;
    let painter = cx.painter();
    painter.vline(
        panel.left() + MASTER_LEFT,
        panel.top() + 12.0..=panel.top() + 306.0,
        paint::hairline(theme, color.line_divider),
    );
    // While the master clips, the CLIP word takes the place of the heading, right above
    // the clip cell: the column has no other room for it.
    let heading = Rect::from_center_size(at(centre, 19.0), egui::vec2(60.0, 14.0));
    if !state.meters[MeterId::Master.index()].is_clipped() {
        paint::text(
            painter,
            theme,
            heading,
            Align2::CENTER_CENTER,
            texts.get("mixer.master"),
            tokens.text.label,
            color.text_secondary,
        );
    }

    let meter_rect = layout.meter.translate(panel.min.to_vec2());
    // 0 and 6 dB sit closer than a line of text on this short meter: each figure keeps at
    // least a line below the one above it.
    let mut below = f32::NEG_INFINITY;
    for mark in SCALE_MARKS {
        let y = meter_rect.bottom() - scale_position(-mark) * meter_rect.height();
        let y = y
            .clamp(meter_rect.top() + 6.0, meter_rect.bottom() - 6.0)
            .max(below);
        below = y + tokens.text.readout.line_height - 3.0;
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
    meter_with_clip(
        cx,
        meter_rect,
        MeterId::Master,
        MeterSpec {
            levels: view.engine.master_levels,
            memory: state.meters[MeterId::Master.index()],
            segments: MASTER_SEGMENTS,
            vertical: true,
        },
        heading,
    );

    knob_cell(
        cx,
        panel.min + layout.level,
        id("mixer-master-level"),
        KnobCell {
            name: "mixer.level",
            reading: db_reading(cx, mixer.master_gain_db),
            position: gain_position(mixer.master_gain_db),
            default: gain_position(0.0),
            centre_detent: false,
            hide_arc: false,
            enabled: true,
            size: MASTER_KNOB,
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
        panel.left() + centre - 22.0..=panel.left() + centre + 22.0,
        panel.top() + layout.divider,
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
        14.0 + tokens.space.s1 + paint::text_width(painter, theme, phones, tokens.text.label);
    let left = panel.left() + centre - width / 2.0;
    let icon = Rect::from_min_size(
        egui::pos2(left, panel.top() + layout.phones),
        egui::vec2(14.0, 14.0),
    );
    theme.icons.paint(painter, Glyph::Headphones, icon, ink);
    let label = Rect::from_min_size(
        egui::pos2(icon.right() + tokens.space.s1, icon.top()),
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
    let mix_label = knob(
        cx,
        panel.min + layout.mix,
        id("mixer-phones-mix"),
        KnobCell {
            name: "mixer.mix",
            reading,
            position: mix,
            default: 0.0,
            centre_detent: false,
            hide_arc: false,
            enabled: headphones,
            size: layout.phones_knob,
        },
        |position| Command::MixerCueMixSet { position }.into(),
    );
    let vol_label = knob(
        cx,
        panel.min + layout.vol,
        id("mixer-phones-volume"),
        KnobCell {
            name: "mixer.vol",
            reading: db_reading(cx, mixer.headphone_gain_db),
            position: gain_position(mixer.headphone_gain_db),
            default: gain_position(0.0),
            centre_detent: false,
            hide_arc: false,
            enabled: headphones,
            size: layout.phones_knob,
        },
        |position| {
            Command::MixerCueVolumeSet {
                db: gain_db(position),
            }
            .into()
        },
    );
    // The two knobs sit closer than their values are wide: the value of the one in use
    // takes the whole row of the column, right of its divider, and hides both names.
    let row = Rect::from_x_y_ranges(
        panel.left() + MASTER_LEFT + 1.0..=panel.right() - tokens.space.s1,
        mix_label.rect.y_range(),
    );
    if let Some(label) = [&mix_label, &vol_label]
        .into_iter()
        .find(|label| layout.side_by_side && label.in_use)
    {
        label.paint(cx, row);
    } else {
        mix_label.paint(cx, mix_label.rect);
        vol_label.paint(cx, vol_label.rect);
    }
}

/// The crossfader row: the track, what marks its two ends, and the step and automatic fade keys
/// outside them. Positions from the top left corner of the mixer.
struct CrossfaderRow {
    track: Vec2,
    /// Where the marks of the two sides go: the badges of decks A and B with two decks, the L
    /// and R of the assign keys with four.
    ends: [Vec2; 2],
    deck_badges: bool,
    steps: [Vec2; 2],
    autos: [Vec2; 2],
}

/// The channels of the four-deck layout, left to right: `docs/design/DESIGN.md`, *Four-deck
/// layout (M4)*.
const FOUR_ORDER: [DeckId; DeckId::COUNT] = [DeckId::C, DeckId::A, DeckId::B, DeckId::D];
/// Distance between the left edges of two channels of the four-deck layout.
const FOUR_STEP: f32 = 79.0;
const FOUR_PADDING: f32 = 10.0;

pub fn mixer(cx: &mut Cx<'_>, rect: Rect, view: &View<'_>, state: &ViewState) {
    let theme = cx.theme;
    let tokens = &theme.tokens;
    cx.painter()
        .rect_filled(rect, CornerRadius::ZERO, tokens.color.surface_panel);
    let row = match view.settings.deck_layout {
        DeckLayout::Two => {
            let layout = ChannelLayout::two(tokens);
            for deck in DeckLayout::Two.decks() {
                let origin = rect.min + egui::vec2(deck.index() as f32 * CHANNEL_STEP, 0.0);
                channel(cx, origin, &layout, *deck, (view, state));
            }
            CrossfaderRow {
                track: egui::vec2(118.0, 315.0),
                ends: [egui::vec2(88.0, 320.0), egui::vec2(290.0, 320.0)],
                deck_badges: true,
                steps: [egui::vec2(8.0, 317.0), egui::vec2(364.0, 317.0)],
                autos: [egui::vec2(36.0, 317.0), egui::vec2(314.0, 317.0)],
            }
        }
        DeckLayout::Four => {
            let layout = ChannelLayout::four();
            for (slot, deck) in FOUR_ORDER.into_iter().enumerate() {
                let origin =
                    rect.min + egui::vec2(FOUR_PADDING + slot as f32 * FOUR_STEP, FOUR_PADDING);
                channel(cx, origin, &layout, deck, (view, state));
            }
            CrossfaderRow {
                track: egui::vec2(117.0, 541.0),
                ends: [egui::vec2(98.0, 546.0), egui::vec2(281.0, 546.0)],
                deck_badges: false,
                steps: [egui::vec2(13.0, 543.0), egui::vec2(355.0, 543.0)],
                autos: [egui::vec2(49.0, 543.0), egui::vec2(301.0, 543.0)],
            }
        }
    };
    let layout = match view.settings.deck_layout {
        DeckLayout::Two => &MasterLayout::TWO,
        DeckLayout::Four => &MasterLayout::FOUR,
    };
    master(cx, rect, layout, (view, state));

    if view.engine.mixer.mode == MixerMode::External {
        return;
    }
    crossfader(cx, rect, &row, view);
}

fn crossfader(cx: &mut Cx<'_>, rect: Rect, row: &CrossfaderRow, view: &View<'_>) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let at = |offset: Vec2| rect.min + offset;
    let track = Rect::from_min_size(
        at(row.track),
        egui::vec2(tokens.size.crossfader_track, 28.0),
    );
    let marks = [
        (DeckId::A, "mixer.assign_left"),
        (DeckId::B, "mixer.assign_right"),
    ];
    for ((deck, side), end) in marks.into_iter().zip(row.ends) {
        let mark = Rect::from_min_size(at(end), egui::vec2(18.0, 18.0));
        if row.deck_badges {
            paint::badge(
                cx.painter(),
                theme,
                mark,
                deck.letter(),
                deck_color(theme, deck),
            );
        } else {
            paint::text(
                cx.painter(),
                theme,
                mark,
                Align2::CENTER_CENTER,
                texts.get(side),
                tokens.text.label,
                tokens.color.text_secondary,
            );
        }
    }
    let name = texts.get("mixer.crossfader");
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

    // Outside each end: a step towards that side, then a fade to it by itself.
    let fades = [
        (
            CrossfaderDirection::Left,
            Glyph::ChevronLeft,
            "mixer.fade_step_a",
            "mixer.fade_auto_a",
        ),
        (
            CrossfaderDirection::Right,
            Glyph::ChevronRight,
            "mixer.fade_step_b",
            "mixer.fade_auto_b",
        ),
    ];
    for (place, (direction, glyph, step_name, auto_name)) in fades.into_iter().enumerate() {
        let name = texts.get(step_name);
        key::key(
            cx,
            Rect::from_min_size(at(row.steps[place]), egui::vec2(FADE_STEP_WIDTH, 24.0)),
            id(("mixer-fade-step", place)),
            KeySpec::icon(glyph, 14.0, name),
            Press::Click(Command::MixerCrossfaderStep { direction }.into()),
        );
        let label = texts.get("mixer.fade_auto");
        let mut spec = KeySpec::text(label);
        spec.name = texts.get(auto_name);
        spec.text = Some(tokens.text.label);
        key::key(
            cx,
            Rect::from_min_size(at(row.autos[place]), egui::vec2(FADE_AUTO_WIDTH, 24.0)),
            id(("mixer-fade-auto", place)),
            spec,
            Press::Click(Command::MixerCrossfaderAutoFade { direction }.into()),
        );
    }
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
