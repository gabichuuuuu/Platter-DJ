# 0012. Translations: TOML files and a lookup by key, no library

Status: accepted (M2).

## Context

The interface is translatable from M2, with English and Spanish complete and a language
choice in the preferences. The task is to pick a translation library after checking its
licence and maintenance, or to do without one. A library would be a dependency outside the
approved list, which means asking the maintainer first.

## Options

1. **Fluent** (`fluent`, `fluent-bundle`; Apache-2.0 OR MIT, maintained by the Fluent
   project). Messages with plural rules, gender and nested selections, in its own file format.
2. **`rust-i18n`** (MIT, maintained). A macro that loads YAML, JSON or TOML at compile time
   and a global current locale.
3. **Project code.** One TOML file per language, compiled in, flattened to keys; a lookup by
   key and a replacement of named placeholders.

## Choice

Option 3. What the interface of M2 needs from a translation layer is small: about 220 short
strings, some with one or two values in them. Nothing in them depends on a plural or a
gender: counts are shown as "13 audio files" and "0 dropouts since start", which read
correctly in both languages for any number. `toml` is already a dependency, so the files need
no new parser. The lookup with its file reader is about 100 lines.

Fluent solves problems these strings do not have and brings its own syntax and several crates.
`rust-i18n` keeps the current language in a global, which is state the interface would hold
outside the view it draws from; and its macro hides from the tests which keys exist. Neither is
on the approved list. If a later milestone needs plural rules, the lookup is one function
behind which a library can sit.

## Rules that follow

- No panel writes a text. A test draws every state of the interface with marked translations
  and fails on any text that is neither marked, nor data of the scene, nor a number.
- Both files hold the same keys with the same placeholders; a test compares them.
- A key without a text shows as the key itself. A test draws every state in both languages
  and fails if one appears.
- Numbers are written by `format.rs` and are the same in every language, with a point as the
  decimal separator; units and the percent sign come from the translation, so Spanish writes
  "48 kHz" and "18 %".
- Names of keys in shortcuts are translated one by one; letters, digits and function keys show
  as they are.

## How it was checked

`crates/ui-egui/src/i18n.rs` and `crates/ui-egui/tests/panels.rs`
(`no_panel_paints_a_text_that_is_not_translated`, `every_text_a_panel_asks_for_exists`,
`every_command_has_a_label_in_both_languages`).
