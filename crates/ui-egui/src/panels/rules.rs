//! The dialog of the rules of a smart playlist: one row per rule, each with its field and its
//! value, then *Add rule*, *Cancel* and *Save*.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Playlists*; the rules themselves are in
//! `docs/behavior/library.md`, *Smart playlists*. The layout follows the section *Smart
//! playlist rules* of `docs/design/DESIGN.md`, in the pattern of `Track-details.html`.

use egui::{Align2, CornerRadius, Rect, StrokeKind};
use platter_library::{MAX_COLOUR, MAX_KEY, MAX_RULES, NodeId, RangeField, Rule};

use crate::command::{Action, AppCommand, RuleField};
use crate::cx::{Cx, id};
use crate::icons::Glyph;
use crate::keys::{self, KeyNotation};
use crate::paint;
use crate::widgets::choice::{self, Choice, SelectSpec};
use crate::widgets::field;
use crate::widgets::key::{self, KeySpec, Press};

/// Width of the dialog.
pub const RULES_WIDTH: f32 = 560.0;
const PAD_X: f32 = 20.0;
const PAD_Y: f32 = 16.0;
const TITLE_HEIGHT: f32 = 26.0;
const CAPTION_HEIGHT: f32 = 20.0;
const ROW_HEIGHT: f32 = 32.0;
const ROW_GAP: f32 = 8.0;
const FIELD_WIDTH: f32 = 140.0;
const REMOVE_SIDE: f32 = 28.0;
const FOOTER_HEIGHT: f32 = 56.0;
/// Seconds in a day, for the dates of import and of last play.
const DAY: i64 = 86_400;

/// One rule as the dialog edits it: its field, and its value as typed or chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleRow {
    pub field: RuleField,
    /// The words, or the lower bound of a range.
    pub from: String,
    /// The upper bound of a range.
    pub to: String,
    /// The colour (0 for the first), the key, or *Yes* (0) and *No* (1).
    pub choice: usize,
}

impl RuleRow {
    pub fn new(field: RuleField) -> Self {
        Self {
            field,
            from: String::new(),
            to: String::new(),
            choice: 0,
        }
    }

    /// The row of a rule kept in the library.
    pub fn of(rule: &Rule) -> Self {
        let bound = |field: RangeField, value: Option<f64>| {
            value.map_or_else(String::new, |value| match field {
                RangeField::Imported | RangeField::LastPlayed => date_text(value as i64),
                _ => value.to_string(),
            })
        };
        match rule {
            Rule::Words { field, words } => Self {
                from: words.clone(),
                ..Self::new(RuleField::of_tag(*field))
            },
            Rule::Range { field, low, high } => {
                // The upper date is the last second of its day: the field shows that day.
                Self {
                    from: bound(*field, *low),
                    to: bound(*field, *high),
                    ..Self::new(RuleField::of_range(*field))
                }
            }
            Rule::Colour(colour) => Self {
                choice: usize::from(colour.saturating_sub(1)),
                ..Self::new(RuleField::Colour)
            },
            Rule::Key(key) => Self {
                choice: usize::from(*key),
                ..Self::new(RuleField::Key)
            },
            Rule::Missing(set) => Self {
                choice: usize::from(!*set),
                ..Self::new(RuleField::Missing)
            },
            Rule::Analysed(set) => Self {
                choice: usize::from(!*set),
                ..Self::new(RuleField::Analysed)
            },
        }
    }

    /// The rule the row holds, or none when its value means nothing.
    pub fn rule(&self) -> Option<Rule> {
        let rule = match self.field {
            RuleField::Title | RuleField::Artist | RuleField::Album | RuleField::Genre => {
                Rule::Words {
                    field: self.field.tag()?,
                    words: self.from.trim().to_owned(),
                }
            }
            RuleField::Colour => Rule::Colour(u8::try_from(self.choice + 1).ok()?),
            RuleField::Key => Rule::Key(u8::try_from(self.choice).ok()?),
            RuleField::Missing => Rule::Missing(self.choice == 0),
            RuleField::Analysed => Rule::Analysed(self.choice == 0),
            _ => {
                let field = self.field.range()?;
                let dates = matches!(field, RangeField::Imported | RangeField::LastPlayed);
                let bound = |text: &str, end_of_day: bool| -> Option<Option<f64>> {
                    let text = text.trim();
                    if text.is_empty() {
                        return Some(None);
                    }
                    if dates {
                        let day = parse_date(text)?;
                        let second = if end_of_day { day + DAY - 1 } else { day };
                        Some(Some(second as f64))
                    } else {
                        text.parse::<f64>().ok().map(Some)
                    }
                };
                Rule::Range {
                    field,
                    low: bound(&self.from, false)?,
                    high: bound(&self.to, true)?,
                }
            }
        };
        rule.validate().ok().map(|()| rule)
    }
}

/// What the dialog edits: the rules of one smart playlist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesForm {
    pub smart: NodeId,
    pub name: String,
    pub rows: Vec<RuleRow>,
}

impl RulesForm {
    /// The rules of every row, or none while a row holds a value that means nothing.
    pub fn rules(&self) -> Option<Vec<Rule>> {
        self.rows.iter().map(RuleRow::rule).collect()
    }
}

/// Size of the dialog for `rows` rules; 16 fit in the window at its design size.
pub fn rules_size(rows: usize) -> egui::Vec2 {
    let rows = rows as f32;
    egui::vec2(
        RULES_WIDTH,
        2.0 * PAD_Y
            + TITLE_HEIGHT
            + CAPTION_HEIGHT
            + 16.0
            + rows * (ROW_HEIGHT + ROW_GAP)
            + ROW_HEIGHT
            + FOOTER_HEIGHT,
    )
}

/// A date as the dialog writes it: year, month and day in UTC.
pub fn date_text(seconds: i64) -> String {
    let (year, month, day, ..) = platter_library::Timestamp(seconds).utc();
    format!("{year:04}-{month:02}-{day:02}")
}

/// The first second, in UTC, of a date written as `YYYY-MM-DD`. The count of days comes
/// from the algorithm `days_from_civil` of Howard Hinnant,
/// <https://howardhinnant.github.io/date_algorithms.html>.
pub fn parse_date(text: &str) -> Option<i64> {
    let mut parts = text.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let length = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=length).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some((era * 146_097 + day_of_era - 719_468) * DAY)
}

/// Draws the dialog in `rect` for `form`, whose fields take what is typed.
pub fn rules(cx: &mut Cx<'_>, rect: Rect, form: &mut RulesForm, notation: KeyNotation) {
    let theme = cx.theme;
    let texts = cx.texts;
    let tokens = &theme.tokens;
    let color = &tokens.color;
    let painter = cx.painter();
    painter.rect(
        rect.expand(1.0),
        CornerRadius::ZERO,
        color.surface_panel,
        paint::hairline(theme, color.line_key),
        StrokeKind::Outside,
    );
    let inner = rect.shrink2(egui::vec2(PAD_X, PAD_Y));
    let line = |top: f32, height: f32| {
        Rect::from_min_size(
            egui::pos2(inner.left(), top),
            egui::vec2(inner.width(), height),
        )
    };
    let mut top = inner.top();
    let title = texts.format("rules.title", &[("name", &form.name)]);
    paint::text_elided(
        painter,
        theme,
        line(top, TITLE_HEIGHT),
        &title,
        tokens.text.title,
        color.text_primary,
    );
    top += TITLE_HEIGHT;
    paint::text(
        painter,
        theme,
        line(top, CAPTION_HEIGHT),
        Align2::LEFT_CENTER,
        texts.get("rules.every"),
        tokens.text.body,
        color.text_secondary,
    );
    top += CAPTION_HEIGHT + 16.0;

    let field_labels: Vec<&str> = RuleField::ALL
        .iter()
        .map(|field| texts.get(field.label()))
        .collect();
    let field_choices: Vec<Choice<'_>> = field_labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    for (index, row) in form.rows.iter_mut().enumerate() {
        let area = line(top, ROW_HEIGHT);
        let select = Rect::from_min_size(area.min, egui::vec2(FIELD_WIDTH, ROW_HEIGHT));
        choice::select(
            cx,
            select,
            id(("rules-field", index)),
            SelectSpec {
                name: texts.get("rules.field"),
                choices: &field_choices,
                current: RuleField::ALL.iter().position(|field| *field == row.field),
                enabled: true,
            },
            |at| {
                Action::App(AppCommand::BrowserSmartRuleField {
                    row: index,
                    field: RuleField::ALL.get(at).copied().unwrap_or(RuleField::Genre),
                })
            },
        );
        let remove = Rect::from_min_size(
            egui::pos2(
                area.right() - REMOVE_SIDE,
                area.center().y - REMOVE_SIDE / 2.0,
            ),
            egui::vec2(REMOVE_SIDE, REMOVE_SIDE),
        );
        key::key(
            cx,
            remove,
            id(("rules-remove", index)),
            KeySpec::icon(Glyph::Close, 14.0, texts.get("rules.remove")),
            Press::Click(Action::App(AppCommand::BrowserSmartRuleRemove {
                row: index,
            })),
        );
        let value = Rect::from_min_max(
            egui::pos2(select.right() + tokens.space.s3, area.top()),
            egui::pos2(remove.left() - tokens.space.s3, area.bottom()),
        );
        let warn = row.rule().is_none();
        value_of(cx, value, index, row, notation);
        if warn {
            cx.painter().rect_stroke(
                value.expand(2.0),
                paint::radius(tokens.radius.md),
                paint::hairline(theme, color.status_warn),
                StrokeKind::Outside,
            );
        }
        top += ROW_HEIGHT + ROW_GAP;
    }
    let add = texts.get("rules.add");
    let width = key::width_for(cx, add, 88.0);
    key::key(
        cx,
        Rect::from_min_size(egui::pos2(inner.left(), top), egui::vec2(width, ROW_HEIGHT)),
        id("rules-add"),
        KeySpec::text(add).enabled(form.rows.len() < MAX_RULES),
        Press::Click(Action::App(AppCommand::BrowserSmartRuleAdd)),
    );
    footer(cx, rect, form);
}

/// The value of a row: its words, its range, its colour, its key, or *Yes* and *No*.
fn value_of(cx: &mut Cx<'_>, rect: Rect, index: usize, row: &mut RuleRow, notation: KeyNotation) {
    let texts = cx.texts;
    let tokens = &cx.theme.tokens;
    let choose = move |at: usize| {
        Action::App(AppCommand::BrowserSmartRuleChoice {
            row: index,
            value: at,
        })
    };
    match row.field {
        RuleField::Title | RuleField::Artist | RuleField::Album | RuleField::Genre => {
            field::text(
                cx,
                rect,
                id(("rules-words", index)),
                &mut row.from,
                texts.get("rules.words"),
                "browser.smart.rules.set",
            );
        }
        RuleField::Colour => {
            let labels: Vec<String> = (1..=MAX_COLOUR)
                .map(|colour| {
                    texts.format("browser.colour_number", &[("number", &colour.to_string())])
                })
                .collect();
            select_of(cx, rect, index, &labels, row.choice, choose);
        }
        RuleField::Key => {
            let words = (
                texts.get("browser.key_major"),
                texts.get("browser.key_minor"),
            );
            let labels: Vec<String> = (0..=MAX_KEY)
                .map(|key| keys::key_text(key, notation, words))
                .collect();
            select_of(cx, rect, index, &labels, row.choice, choose);
        }
        RuleField::Missing | RuleField::Analysed => {
            let choices = [
                Choice {
                    label: texts.get("rules.yes"),
                    enabled: true,
                },
                Choice {
                    label: texts.get("rules.no"),
                    enabled: true,
                },
            ];
            choice::segmented(
                cx,
                rect,
                id(("rules-flag", index)),
                &choices,
                Some(row.choice.min(1)),
                choose,
            );
        }
        _ => {
            let unit = row.field.unit().map_or("", |unit| texts.get(unit));
            let between = texts.get("rules.to");
            let painter = cx.painter();
            let unit_width =
                paint::text_width(painter, cx.theme, unit, tokens.text.body) + tokens.space.s3;
            let between_width = paint::text_width(painter, cx.theme, between, tokens.text.body)
                + 2.0 * tokens.space.s3;
            let width = ((rect.width() - unit_width - between_width) / 2.0).max(0.0);
            let from = Rect::from_min_size(rect.min, egui::vec2(width, rect.height()));
            let to = Rect::from_min_size(
                egui::pos2(from.right() + between_width, rect.top()),
                egui::vec2(width, rect.height()),
            );
            field::text(
                cx,
                from,
                id(("rules-from", index)),
                &mut row.from,
                texts.get("rules.from"),
                "browser.smart.rules.set",
            );
            field::text(
                cx,
                to,
                id(("rules-to", index)),
                &mut row.to,
                texts.get("rules.to"),
                "browser.smart.rules.set",
            );
            let painter = cx.painter();
            paint::text(
                painter,
                cx.theme,
                Rect::from_min_max(
                    egui::pos2(from.right(), rect.top()),
                    egui::pos2(to.left(), rect.bottom()),
                ),
                Align2::CENTER_CENTER,
                between,
                tokens.text.body,
                tokens.color.text_secondary,
            );
            paint::text(
                painter,
                cx.theme,
                Rect::from_min_max(egui::pos2(to.right(), rect.top()), rect.max),
                Align2::RIGHT_CENTER,
                unit,
                tokens.text.body,
                tokens.color.text_secondary,
            );
        }
    }
}

fn select_of(
    cx: &mut Cx<'_>,
    rect: Rect,
    index: usize,
    labels: &[String],
    current: usize,
    make: impl Fn(usize) -> Action,
) {
    let choices: Vec<Choice<'_>> = labels
        .iter()
        .map(|label| Choice {
            label,
            enabled: true,
        })
        .collect();
    choice::select(
        cx,
        rect,
        id(("rules-choice", index)),
        SelectSpec {
            name: cx.texts.get("rules.value"),
            choices: &choices,
            current: Some(current),
            enabled: true,
        },
        make,
    );
}

/// The foot: *Cancel* and *Save*; *Save* waits until every row holds a rule.
fn footer(cx: &mut Cx<'_>, rect: Rect, form: &RulesForm) {
    let texts = cx.texts;
    let tokens = &cx.theme.tokens;
    let middle = rect.bottom() - FOOTER_HEIGHT / 2.0;
    let rules = form.rules();
    let save = Action::App(AppCommand::BrowserSmartRulesSet {
        smart: form.smart,
        rules: rules.clone().unwrap_or_default(),
    });
    let keys = [
        (texts.get("rules.save"), save, rules.is_some()),
        (
            texts.get("rules.cancel"),
            Action::App(AppCommand::BrowserSmartRulesClose),
            true,
        ),
    ];
    let mut right = rect.right() - PAD_X;
    for (index, (label, action, enabled)) in keys.into_iter().enumerate() {
        let width = key::width_for(cx, label, 88.0);
        let at = Rect::from_min_size(
            egui::pos2(right - width, middle - tokens.size.button_h / 2.0),
            egui::vec2(width, tokens.size.button_h),
        );
        right = at.left() - tokens.space.s3;
        key::key(
            cx,
            at,
            id(("rules-key", index)),
            KeySpec::text(label).enabled(enabled),
            Press::Click(action),
        );
    }
}

#[cfg(test)]
mod tests {
    use platter_library::TagField;

    use super::*;

    #[test]
    fn dates_read_back_and_the_upper_one_takes_its_whole_day() {
        for (text, seconds) in [
            ("1970-01-01", 0),
            ("2000-02-29", 951_782_400),
            ("2026-11-20", 1_795_132_800),
        ] {
            assert_eq!(parse_date(text), Some(seconds), "{text}");
            assert_eq!(date_text(seconds), text);
            assert_eq!(date_text(seconds + DAY - 1), text);
        }
        for text in [
            "2026-02-29",
            "2026-13-01",
            "2026-1",
            "x",
            "2026-01-01-01",
            "0-01-01",
        ] {
            assert_eq!(parse_date(text), None, "{text}");
        }
        let row = RuleRow {
            from: "2026-11-01".into(),
            to: "2026-11-20".into(),
            ..RuleRow::new(RuleField::Played)
        };
        let rule = row.rule().unwrap();
        assert_eq!(
            rule,
            Rule::Range {
                field: RangeField::LastPlayed,
                low: Some(1_793_491_200.0),
                high: Some(1_795_132_800.0 + 86_399.0),
            }
        );
        assert_eq!(RuleRow::of(&rule), row);
    }

    #[test]
    fn every_rule_comes_back_from_its_row_and_a_bad_row_holds_none() {
        let rules = [
            Rule::Words {
                field: TagField::Genre,
                words: "deep house".into(),
            },
            Rule::Range {
                field: RangeField::Tempo,
                low: Some(120.0),
                high: None,
            },
            Rule::Colour(8),
            Rule::Key(23),
            Rule::Missing(false),
            Rule::Analysed(true),
        ];
        for rule in rules {
            assert_eq!(RuleRow::of(&rule).rule(), Some(rule));
        }
        for row in [
            RuleRow::new(RuleField::Title),
            RuleRow::new(RuleField::Tempo),
            RuleRow {
                from: "fast".into(),
                ..RuleRow::new(RuleField::Tempo)
            },
            RuleRow {
                from: "130".into(),
                to: "120".into(),
                ..RuleRow::new(RuleField::Tempo)
            },
            RuleRow {
                from: "yesterday".into(),
                ..RuleRow::new(RuleField::Imported)
            },
        ] {
            assert_eq!(row.rule(), None, "{row:?}");
        }
    }
}
