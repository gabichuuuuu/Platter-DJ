//! The effect units and the mixer effects in the settings file, under `[effects]`
//! (`docs/behavior/effects.md`). The on keys and the mixer effect knobs are not kept: Platter
//! starts with every effect off. A value that cannot be used is replaced by its default.

use platter_core::DeckId;
use platter_engine::{Controls, EffectControls, EffectKind, FxRouting, FxUnit, UnitMode};

/// The state as it is kept: every on key off, every mixer effect knob at zero.
pub fn kept(effects: &EffectControls) -> EffectControls {
    let mut kept = effects.clone();
    for unit in &mut kept.units {
        for slot in &mut unit.values.slots {
            slot.on = false;
        }
        unit.values.single.on = false;
    }
    for mixer in &mut kept.mixer {
        mixer.amount = 0.0;
    }
    kept
}

fn effect_name(effect: Option<EffectKind>) -> toml::Value {
    toml::Value::String(effect.map_or("none", EffectKind::name).to_owned())
}

fn numbers(values: &[f32]) -> toml::Value {
    toml::Value::Array(
        values
            .iter()
            .map(|value| toml::Value::Float(f64::from(*value)))
            .collect(),
    )
}

fn flags(values: &[bool]) -> toml::Value {
    toml::Value::Array(values.iter().map(|on| toml::Value::Boolean(*on)).collect())
}

fn controls_table(controls: &Controls) -> toml::Table {
    let mut table = toml::Table::new();
    table.insert("knobs".to_owned(), numbers(&controls.knobs));
    table.insert("keys".to_owned(), flags(&controls.keys));
    table
}

pub fn write(effects: &EffectControls) -> toml::Table {
    let effects = kept(effects);
    let units = effects
        .units
        .iter()
        .map(|unit| {
            let mut table = toml::Table::new();
            let values = &unit.values;
            let text = |text: &str| toml::Value::String(text.to_owned());
            table.insert("mode".to_owned(), text(values.mode.name()));
            table.insert(
                "dry_wet".to_owned(),
                toml::Value::Float(f64::from(values.dry_wet)),
            );
            table.insert("routing".to_owned(), text(unit.routing.name()));
            table.insert(
                "slots".to_owned(),
                toml::Value::Array(unit.slots.iter().map(|kind| effect_name(*kind)).collect()),
            );
            let amounts: Vec<f32> = values.slots.iter().map(|slot| slot.amount).collect();
            table.insert("amounts".to_owned(), numbers(&amounts));
            table.insert("single".to_owned(), effect_name(unit.single));
            table.extend(controls_table(&values.single.controls));
            let mut snapshots = toml::Table::new();
            for (kind, snapshot) in EffectKind::ALL.iter().zip(&unit.snapshots) {
                if let Some(controls) = snapshot {
                    snapshots.insert(
                        kind.name().to_owned(),
                        toml::Value::Table(controls_table(controls)),
                    );
                }
            }
            if !snapshots.is_empty() {
                table.insert("snapshots".to_owned(), toml::Value::Table(snapshots));
            }
            toml::Value::Table(table)
        })
        .collect();
    let channels = DeckId::ALL
        .iter()
        .map(|deck| {
            let mut table = toml::Table::new();
            let assigned = FxUnit::ALL
                .iter()
                .filter(|unit| effects.is_assigned(*deck, **unit))
                .map(|unit| toml::Value::Integer(i64::from(unit.number())))
                .collect();
            table.insert("units".to_owned(), toml::Value::Array(assigned));
            table.insert(
                "mixer_effect".to_owned(),
                toml::Value::String(effects.mixer[deck.index()].effect.name().to_owned()),
            );
            toml::Value::Table(table)
        })
        .collect();
    let mut table = toml::Table::new();
    table.insert("units".to_owned(), toml::Value::Array(units));
    table.insert("channels".to_owned(), toml::Value::Array(channels));
    table.insert(
        "keep_values".to_owned(),
        toml::Value::Boolean(effects.keep_values),
    );
    table
}

fn read_effect(value: Option<&toml::Value>) -> Option<EffectKind> {
    value
        .and_then(toml::Value::as_str)
        .and_then(|name| EffectKind::parse(name).ok())
}

/// A value between 0 and 1, or `None`.
fn read_unit_value(value: &toml::Value) -> Option<f32> {
    let number = value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))?;
    (0.0..=1.0).contains(&number).then_some(number as f32)
}

fn read_controls(table: &toml::Table, into: &mut Controls) {
    if let Some(knobs) = table.get("knobs").and_then(toml::Value::as_array) {
        for (knob, value) in into.knobs.iter_mut().zip(knobs) {
            if let Some(value) = read_unit_value(value) {
                *knob = value;
            }
        }
    }
    if let Some(keys) = table.get("keys").and_then(toml::Value::as_array) {
        for (key, value) in into.keys.iter_mut().zip(keys) {
            if let Some(on) = value.as_bool() {
                *key = on;
            }
        }
    }
}

pub fn read(table: Option<&toml::Table>) -> EffectControls {
    let mut effects = EffectControls::default();
    let Some(table) = table else {
        return effects;
    };
    if let Some(keep) = table.get("keep_values").and_then(toml::Value::as_bool) {
        effects.keep_values = keep;
    }
    let units = table.get("units").and_then(toml::Value::as_array);
    for (unit, value) in effects.units.iter_mut().zip(units.into_iter().flatten()) {
        let Some(source) = value.as_table() else {
            continue;
        };
        let text = |name: &str| source.get(name).and_then(toml::Value::as_str);
        if let Some(mode) = text("mode").and_then(|mode| UnitMode::parse(mode).ok()) {
            unit.values.mode = mode;
        }
        if let Some(dry_wet) = source.get("dry_wet").and_then(read_unit_value) {
            unit.values.dry_wet = dry_wet;
        }
        if let Some(routing) = text("routing").and_then(|name| FxRouting::parse(name).ok()) {
            unit.routing = routing;
        }
        if let Some(slots) = source.get("slots").and_then(toml::Value::as_array) {
            for (slot, value) in unit.slots.iter_mut().zip(slots) {
                *slot = read_effect(Some(value));
            }
        }
        if let Some(amounts) = source.get("amounts").and_then(toml::Value::as_array) {
            for (slot, value) in unit.values.slots.iter_mut().zip(amounts) {
                if let Some(amount) = read_unit_value(value) {
                    slot.amount = amount;
                }
            }
        }
        unit.single = read_effect(source.get("single"));
        if let Some(kind) = unit.single {
            unit.values.single.controls = kind.defaults();
            read_controls(source, &mut unit.values.single.controls);
        }
        if let Some(snapshots) = source.get("snapshots").and_then(toml::Value::as_table) {
            for (kind, snapshot) in EffectKind::ALL.iter().zip(unit.snapshots.iter_mut()) {
                if let Some(table) = snapshots.get(kind.name()).and_then(toml::Value::as_table) {
                    let mut controls = kind.defaults();
                    read_controls(table, &mut controls);
                    *snapshot = Some(controls);
                }
            }
        }
    }
    let channels = table.get("channels").and_then(toml::Value::as_array);
    for (deck, value) in DeckId::ALL.iter().zip(channels.into_iter().flatten()) {
        let Some(source) = value.as_table() else {
            continue;
        };
        if let Some(units) = source.get("units").and_then(toml::Value::as_array) {
            for number in units.iter().filter_map(toml::Value::as_integer) {
                if let Ok(unit) = FxUnit::from_number(number as f64) {
                    effects.assigned[deck.index()][unit.index()] = true;
                }
            }
        }
        if let Some(effect) =
            read_effect(source.get("mixer_effect")).filter(|kind| EffectKind::MIXER.contains(kind))
        {
            effects.mixer[deck.index()].effect = effect;
        }
    }
    effects
}

#[cfg(test)]
mod tests {
    use platter_core::DeckId;
    use platter_engine::{Controls, EffectControls, EffectKind, FxRouting, FxUnit, UnitMode};

    use super::{kept, read, write};

    #[test]
    fn the_effects_come_back_as_they_were_kept_with_every_effect_off() {
        let mut effects = EffectControls::default();
        let unit = &mut effects.units[1];
        unit.values.mode = UnitMode::Single;
        unit.values.dry_wet = 0.25;
        unit.routing = FxRouting::PostFader;
        unit.slots[2] = Some(EffectKind::Gater);
        unit.values.slots[2].amount = 0.75;
        unit.values.slots[2].on = true;
        unit.single = Some(EffectKind::Delay);
        unit.values.single.controls = Controls {
            knobs: [0.5, 0.25, 1.0],
            keys: [true, false],
        };
        unit.values.single.on = true;
        unit.snapshots[0] = Some(Controls {
            knobs: [0.125, 0.5, 0.75],
            keys: [false, true],
        });
        effects.assigned[DeckId::C.index()][FxUnit::ALL[1].index()] = true;
        effects.mixer[DeckId::D.index()].effect = EffectKind::Bitcrusher;
        effects.mixer[DeckId::D.index()].amount = 0.5;
        effects.keep_values = true;

        let table = write(&effects);
        let text = toml::to_string(&table).unwrap();
        let back = read(text.parse::<toml::Table>().ok().as_ref());
        assert_eq!(back, kept(&effects));
        assert!(!back.units[1].values.slots[2].on);
        assert!(!back.units[1].values.single.on);
        assert_eq!(back.mixer[DeckId::D.index()].amount, 0.0);
    }

    #[test]
    fn what_cannot_be_used_reads_as_the_default() {
        let text = r#"
            [[units]]
            mode = "triple"
            dry_wet = 3.0
            routing = "sideways"
            slots = ["wobble", 4]
            [[channels]]
            units = [0, 9, 2]
            mixer_effect = "flanger"
        "#;
        let table = text.parse::<toml::Table>().unwrap();
        let effects = read(Some(&table));
        let defaults = EffectControls::default();
        assert_eq!(effects.units[0], defaults.units[0]);
        assert_eq!(effects.mixer, defaults.mixer);
        assert_eq!(
            effects.assigned[DeckId::A.index()],
            [false, true, false, false]
        );
    }
}
