//! Texts of the interface in each language.
//!
//! Every string a panel shows comes from here by key; a test fails if a panel paints text
//! that did not. The translations are TOML files compiled into the binary, one per
//! language, with nested tables that are flattened to keys such as `deck.cue`. A text may
//! hold placeholders in braces, filled when it is shown: `"{count} audio files"`.
//! `docs/decisions/0012-translations.md` records why no translation library is used.

use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Language {
    #[default]
    English,
    Spanish,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::Spanish];

    /// The code that names the language in the settings file.
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
    }

    fn source(self) -> &'static str {
        match self {
            Self::English => include_str!("../locales/en.toml"),
            Self::Spanish => include_str!("../locales/es.toml"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextError {
    Syntax {
        language: Language,
        message: String,
    },
    /// A value that is neither a text nor a table of texts.
    NotText {
        language: Language,
        key: String,
    },
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { language, message } => {
                write!(
                    f,
                    "the texts of `{}` cannot be read: {message}",
                    language.code()
                )
            }
            Self::NotText { language, key } => {
                write!(
                    f,
                    "`{key}` in the texts of `{}` is not a text",
                    language.code()
                )
            }
        }
    }
}

impl std::error::Error for TextError {}

/// The texts of one language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texts {
    language: Language,
    texts: BTreeMap<String, String>,
}

/// Nesting of the tables in a translation file. The files use two levels.
const MAX_DEPTH: usize = 4;

fn flatten(
    table: &toml::Table,
    prefix: &str,
    depth: usize,
    language: Language,
    out: &mut BTreeMap<String, String>,
) -> Result<(), TextError> {
    for (name, value) in table {
        let key = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::String(text) => {
                out.insert(key, text.clone());
            }
            toml::Value::Table(inner) if depth < MAX_DEPTH => {
                flatten(inner, &key, depth + 1, language, out)?;
            }
            _ => return Err(TextError::NotText { language, key }),
        }
    }
    Ok(())
}

impl Texts {
    pub fn load(language: Language) -> Result<Self, TextError> {
        let root: toml::Table = language
            .source()
            .parse()
            .map_err(|error: toml::de::Error| TextError::Syntax {
                language,
                message: error.message().to_owned(),
            })?;
        let mut texts = BTreeMap::new();
        flatten(&root, "", 0, language, &mut texts)?;
        Ok(Self { language, texts })
    }

    pub fn language(&self) -> Language {
        self.language
    }

    /// The text of `key`. A key without a text shows as the key itself, which is visible
    /// and harmless; the tests keep both languages complete.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.texts.get(key).map_or(key, String::as_str)
    }

    /// The text of `key` with each `{name}` replaced by its value.
    pub fn format(&self, key: &str, values: &[(&str, &str)]) -> String {
        let mut text = self.get(key).to_owned();
        for (name, value) in values {
            text = text.replace(&format!("{{{name}}}"), value);
        }
        text
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.texts.keys().map(String::as_str)
    }

    /// The same texts with a marker around each, so a test can tell a translated text from
    /// one written in the code.
    pub fn marked(mut self, open: char, close: char) -> Self {
        for text in self.texts.values_mut() {
            *text = format!("{open}{text}{close}");
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Language, Texts};

    /// The placeholders of a text, in order of appearance.
    fn placeholders(text: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut rest = text;
        while let Some((_, after)) = rest.split_once('{') {
            let Some((name, tail)) = after.split_once('}') else {
                break;
            };
            found.insert(name.to_owned());
            rest = tail;
        }
        found
    }

    #[test]
    fn every_key_exists_in_both_languages_with_the_same_placeholders() {
        let english = Texts::load(Language::English).unwrap();
        let spanish = Texts::load(Language::Spanish).unwrap();
        let keys = |texts: &Texts| texts.keys().map(str::to_owned).collect::<BTreeSet<_>>();
        assert_eq!(keys(&english), keys(&spanish));
        assert!(keys(&english).len() > 100);
        for key in english.keys() {
            assert!(!english.get(key).trim().is_empty(), "{key}");
            assert!(!spanish.get(key).trim().is_empty(), "{key}");
            assert_eq!(
                placeholders(english.get(key)),
                placeholders(spanish.get(key)),
                "{key}"
            );
        }
    }

    #[test]
    fn placeholders_are_filled_and_unknown_keys_show_themselves() {
        let texts = Texts::load(Language::English).unwrap();
        assert_eq!(
            texts.format("browser.file_count", &[("count", "13")]),
            "13 audio files"
        );
        assert_eq!(texts.get("no.such.key"), "no.such.key");
        let marked = texts.marked('<', '>');
        assert_eq!(marked.get("deck.cue"), "<CUE>");
    }

    #[test]
    fn languages_are_found_by_their_code() {
        assert_eq!(Language::from_code("es"), Some(Language::Spanish));
        assert_eq!(Language::from_code("en"), Some(Language::English));
        assert_eq!(Language::from_code("fr"), None);
    }
}
