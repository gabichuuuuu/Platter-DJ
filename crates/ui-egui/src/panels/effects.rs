//! The effect row: effect units 1 and 2 over the deck columns, and the panel over the mixer
//! that holds the mix recorder (`super::recorder`).
//!
//! Components: `EffectUnit` and *Effect row* in `docs/design/DESIGN.md`; placement measured on
//! `docs/design/screens/M4-effects.html`. Behaviour note: `docs/behavior/effects.md`.

use egui::{Align2, Rect};
use platter_engine::{Command, EffectKind, FxUnit, UnitControls, UnitMode};

use crate::command::Action;
use crate::cx::{Cx, id};
use crate::model::View;
use crate::paint;
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::key::{self, KeySpec, Press};
use crate::widgets::value::{self, KnobSpec, Steps, ValueSpec};

/// The height of the effect row.
pub const ROW_HEIGHT: f32 = 72.0;
const PADDING: f32 = 12.0;
const KNOB: f32 = 36.0;
const SLOT_LEFT: f32 = 144.0;
const SLOT_STEP: f32 = 124.0;
const SELECTOR: egui::Vec2 = egui::vec2(74.0, 20.0);
const ON_KEY: egui::Vec2 = egui::vec2(74.0, 18.0);
const COLUMN: f32 = 52.0;

/// The effects a selector of a unit offers, `None` first.
const OFFERED: [Option<EffectKind>; 9] = [
    None,
    Some(EffectKind::Delay),
    Some(EffectKind::Reverb),
    Some(EffectKind::Flanger),
    Some(EffectKind::Phaser),
    Some(EffectKind::Filter),
    Some(EffectKind::Gater),
    Some(EffectKind::Slicer),
    Some(EffectKind::Bitcrusher),
];

/// The translation key of the name of an effect.
pub fn effect_key(kind: Option<EffectKind>) -> &'static str {
    match kind {
        None => "effects.none",
        Some(EffectKind::Delay) => "effects.delay",
        Some(EffectKind::Reverb) => "effects.reverb",
        Some(EffectKind::Flanger) => "effects.flanger",
        Some(EffectKind::Phaser) => "effects.phaser",
        Some(EffectKind::Filter) => "effects.filter",
        Some(EffectKind::Gater) => "effects.gater",
        Some(EffectKind::Slicer) => "effects.slicer",
        Some(EffectKind::Bitcrusher) => "effects.bitcrusher",
    }
}

/// The row: `left` and `right` hold units 1 and 2, `middle` is over the mixer.
pub fn row(cx: &mut Cx<'_>, [left, middle, right]: [Rect; 3], view: &View<'_>) {
    let theme = cx.theme;
    let fill = theme.tokens.color.surface_panel;
    for rect in [left, middle, right] {
        cx.painter().rect_filled(rect, 0.0, fill);
    }
    unit(cx, left, FxUnit::ALL[0], view);
    super::recorder::panel(cx, middle, &view.engine.recorder);
    unit(cx, right, FxUnit::ALL[1], view);
}

fn unit(cx: &mut Cx<'_>, panel: Rect, unit: FxUnit, view: &View<'_>) {
    let controls = view.engine.effects.unit(unit);
    let number = unit.number().to_string();
    let salt = unit.index();
    let at = |x: f32, y: f32| panel.min + egui::vec2(x, y);
    let theme = cx.theme;
    let tokens = &theme.tokens;
    let texts = cx.texts;

    // Header: the title over the choice of mode.
    let title = texts.format("effects.unit", &[("unit", &number)]);
    let assigned = view.engine.effects.decks_of(unit).next().is_some();
    paint::text(
        cx.painter(),
        theme,
        Rect::from_min_size(at(PADDING, 16.0), egui::vec2(64.0, 16.0)),
        Align2::LEFT_CENTER,
        &title,
        tokens.text.button,
        if assigned {
            tokens.color.text_primary
        } else {
            tokens.color.text_secondary
        },
    );
    let modes = [
        (UnitMode::Group, "effects.group", 37.0),
        (UnitMode::Single, "effects.single", 39.0),
    ];
    let mut left = PADDING;
    for (mode, label, width) in modes {
        let rect = Rect::from_min_size(at(left, 36.0), egui::vec2(width, 20.0));
        left += width;
        let text = texts.get(label);
        let name = texts.format("effects.mode_name", &[("mode", text), ("unit", &number)]);
        let mut spec = KeySpec::text(text);
        spec.name = &name;
        spec.text = Some(tokens.text.label);
        spec.lit = (controls.values.mode == mode).then_some(tokens.color.text_primary);
        key::key(
            cx,
            rect,
            id(("fx-mode", salt, mode.name())),
            spec,
            Press::Click(Command::FxUnitModeSet { unit, mode }.into()),
        );
    }

    // Dry / wet.
    labelled_knob(
        cx,
        at(84.0, 11.0),
        id(("fx-dry-wet", salt)),
        KnobCell {
            label: texts.get("effects.dry_wet").to_owned(),
            name: texts.format("effects.dry_wet_name", &[("unit", &number)]),
            position: f64::from(controls.values.dry_wet),
            default: f64::from(platter_dsp::effects::unit::DEFAULT_DRY_WET),
            enabled: true,
        },
        |position| Command::FxUnitDryWetSet { unit, position }.into(),
    );

    match controls.values.mode {
        UnitMode::Group => group(cx, panel, unit, controls),
        UnitMode::Single => single(cx, panel, unit, controls),
    }
}

/// A knob in a column of 52 points with its label under it.
struct KnobCell {
    label: String,
    name: String,
    position: f64,
    default: f64,
    enabled: bool,
}

fn labelled_knob(
    cx: &mut Cx<'_>,
    column: egui::Pos2,
    knob_id: egui::Id,
    cell: KnobCell,
    make: impl FnOnce(f64) -> Action,
) {
    let rect = Rect::from_min_size(
        column + egui::vec2((COLUMN - KNOB) / 2.0, 0.0),
        egui::vec2(KNOB, KNOB),
    );
    knob(cx, rect, knob_id, &cell, make);
    let tokens = &cx.theme.tokens;
    paint::text(
        cx.painter(),
        cx.theme,
        Rect::from_min_size(column + egui::vec2(0.0, 37.0), egui::vec2(COLUMN, 14.0)),
        Align2::CENTER_CENTER,
        &cell.label,
        tokens.text.label,
        if cell.enabled {
            tokens.color.text_secondary
        } else {
            tokens.color.text_disabled
        },
    );
}

fn knob(
    cx: &mut Cx<'_>,
    rect: Rect,
    knob_id: egui::Id,
    cell: &KnobCell,
    make: impl FnOnce(f64) -> Action,
) {
    value::knob(
        cx,
        rect,
        knob_id,
        KnobSpec {
            value: ValueSpec {
                name: &cell.name,
                position: cell.position,
                default: cell.default,
                steps: Steps::PERCENT,
                enabled: cell.enabled,
            },
            size: KNOB,
            centre_detent: false,
            hide_arc: false,
        },
        make,
    );
}

/// The selector of the effect of a place, and its on key under it.
fn selector(
    cx: &mut Cx<'_>,
    origin: egui::Pos2,
    salt: (usize, usize),
    (kind, on): (Option<EffectKind>, bool),
    names: (&str, &str),
    actions: (impl Fn(Option<EffectKind>) -> Action, Action),
) {
    let texts = cx.texts;
    let tokens = &cx.theme.tokens;
    let labels: Vec<&str> = OFFERED
        .iter()
        .map(|kind| texts.get(effect_key(*kind)))
        .collect();
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    let (pick, toggle) = actions;
    choice::small_select(
        cx,
        Rect::from_min_size(origin, SELECTOR),
        id(("fx-select", salt.0, salt.1)),
        SelectSpec {
            name: names.0,
            choices: &choices,
            current: OFFERED.iter().position(|offered| *offered == kind),
            enabled: true,
        },
        |index| pick(OFFERED.get(index).copied().flatten()),
    );
    let label = texts.get("effects.on");
    let mut spec = KeySpec::text(label);
    spec.name = names.1;
    spec.text = Some(tokens.text.label);
    spec.enabled = kind.is_some();
    spec.lit = on.then_some(tokens.color.text_primary);
    key::key(
        cx,
        Rect::from_min_size(origin + egui::vec2(0.0, 24.0), ON_KEY),
        id(("fx-on", salt.0, salt.1)),
        spec,
        Press::Click(toggle),
    );
}

fn group(cx: &mut Cx<'_>, panel: Rect, unit: FxUnit, controls: &UnitControls) {
    let texts = cx.texts;
    let number = unit.number().to_string();
    for (slot, (kind, values)) in controls
        .slots
        .iter()
        .zip(controls.values.slots.iter())
        .enumerate()
    {
        let left = panel.left() + SLOT_LEFT + SLOT_STEP * slot as f32;
        let effect = texts.get(effect_key(*kind));
        let slot_number = (slot + 1).to_string();
        let amount_name = texts.format(
            "effects.amount_name",
            &[("effect", effect), ("unit", &number)],
        );
        knob(
            cx,
            Rect::from_min_size(egui::pos2(left, panel.top() + 18.0), egui::vec2(KNOB, KNOB)),
            id(("fx-amount", unit.index(), slot)),
            &KnobCell {
                label: String::new(),
                name: amount_name,
                position: f64::from(values.amount),
                default: f64::from(platter_dsp::effects::DEFAULT_AMOUNT),
                enabled: kind.is_some(),
            },
            |position| {
                Command::FxSlotAmountSet {
                    unit,
                    slot,
                    position,
                }
                .into()
            },
        );
        let select_name = texts.format(
            "effects.select_name",
            &[("slot", &slot_number), ("unit", &number)],
        );
        let on_name = texts.format("effects.on_name", &[("effect", effect), ("unit", &number)]);
        selector(
            cx,
            egui::pos2(left + KNOB + 6.0, panel.top() + 15.0),
            (unit.index(), slot),
            (*kind, values.on),
            (&select_name, &on_name),
            (
                |effect| Command::FxSlotEffectSet { unit, slot, effect }.into(),
                Command::FxSlotToggle { unit, slot }.into(),
            ),
        );
    }
}

/// The names of the three knobs and the two keys of an effect in single mode; an empty name
/// for a control the effect does not use.
fn parameter_keys(kind: EffectKind) -> [&'static str; 5] {
    match kind {
        EffectKind::Delay => [
            "effects.tone",
            "effects.feedback",
            "effects.time",
            "effects.freeze",
            "effects.ping_pong",
        ],
        EffectKind::Reverb => [
            "effects.low_cut",
            "effects.high_cut",
            "effects.size",
            "effects.freeze",
            "",
        ],
        EffectKind::Flanger | EffectKind::Phaser => [
            "effects.spread",
            "effects.feedback",
            "effects.rate",
            "effects.down",
            "effects.free",
        ],
        EffectKind::Filter => [
            "effects.low_cut",
            "effects.resonance",
            "effects.high_cut",
            "effects.reject",
            "effects.one_knob",
        ],
        EffectKind::Gater => [
            "effects.noise",
            "effects.shape",
            "effects.rate",
            "effects.mute",
            "effects.stutter",
        ],
        EffectKind::Slicer => [
            "effects.mix",
            "effects.rotate",
            "effects.length",
            "effects.reverse",
            "effects.resync",
        ],
        EffectKind::Bitcrusher => [
            "effects.bits",
            "effects.rate",
            "effects.spread",
            "effects.smooth",
            "",
        ],
    }
}

fn single(cx: &mut Cx<'_>, panel: Rect, unit: FxUnit, controls: &UnitControls) {
    let texts = cx.texts;
    let tokens = &cx.theme.tokens;
    let number = unit.number().to_string();
    let kind = controls.single;
    let values = &controls.values.single;
    let effect = texts.get(effect_key(kind));
    let select_name = texts.format("effects.single_select_name", &[("unit", &number)]);
    let on_name = texts.format("effects.on_name", &[("effect", effect), ("unit", &number)]);
    selector(
        cx,
        egui::pos2(panel.left() + SLOT_LEFT, panel.top() + 15.0),
        (unit.index(), 9),
        (kind, values.on),
        (&select_name, &on_name),
        (
            |effect| Command::FxSingleEffectSet { unit, effect }.into(),
            Command::FxSingleToggle { unit }.into(),
        ),
    );
    let names = kind.map_or([""; 5], parameter_keys);
    let defaults = kind.map_or_else(Default::default, EffectKind::defaults);
    for (knob_index, key_name) in names.iter().take(3).enumerate() {
        let label = if key_name.is_empty() {
            String::new()
        } else {
            texts.get(key_name).to_owned()
        };
        let name = texts.format(
            "effects.parameter_name",
            &[("parameter", &label), ("unit", &number)],
        );
        labelled_knob(
            cx,
            egui::pos2(
                panel.left() + 226.0 + COLUMN * knob_index as f32,
                panel.top() + 11.0,
            ),
            id(("fx-knob", unit.index(), knob_index)),
            KnobCell {
                label,
                name,
                position: f64::from(values.controls.knobs[knob_index]),
                default: f64::from(defaults.knobs[knob_index]),
                enabled: kind.is_some(),
            },
            |position| {
                Command::FxSingleKnobSet {
                    unit,
                    knob: knob_index,
                    position,
                }
                .into()
            },
        );
    }
    for key_index in 0..2 {
        let used = kind.is_some() && !names[3 + key_index].is_empty();
        let label = if used {
            texts.get(names[3 + key_index])
        } else {
            ""
        };
        let name = texts.format(
            "effects.parameter_name",
            &[("parameter", label), ("unit", &number)],
        );
        let mut spec = KeySpec::text(label);
        spec.name = &name;
        spec.text = Some(tokens.text.label);
        spec.enabled = used;
        spec.lit = values.controls.keys[key_index].then_some(tokens.color.text_primary);
        key::key(
            cx,
            Rect::from_min_size(
                egui::pos2(
                    panel.left() + 390.0,
                    panel.top() + 15.0 + 22.0 * key_index as f32,
                ),
                egui::vec2(COLUMN, 20.0),
            ),
            id(("fx-key", unit.index(), key_index)),
            spec,
            Press::Click(
                Command::FxSingleKeyToggle {
                    unit,
                    key: key_index,
                }
                .into(),
            ),
        );
    }
    let label = texts.get("effects.reset");
    let name = texts.format("effects.reset_name", &[("unit", &number)]);
    let mut spec = KeySpec::text(label);
    spec.name = &name;
    spec.text = Some(tokens.text.label);
    spec.enabled = kind.is_some();
    key::key(
        cx,
        Rect::from_min_size(
            egui::pos2(panel.right() - PADDING - 40.0, panel.top() + 26.0),
            egui::vec2(40.0, 20.0),
        ),
        id(("fx-reset", unit.index())),
        spec,
        Press::Click(Command::FxUnitReset { unit }.into()),
    );
}
