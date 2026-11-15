//! Text as the library keeps it and compares it.

use crate::MAX_TAG_CHARS;

/// A tag as stored: none when empty, cut at [`MAX_TAG_CHARS`] characters.
pub fn tag(value: Option<&str>) -> Option<String> {
    let value = value?;
    (!value.is_empty()).then(|| value.chars().take(MAX_TAG_CHARS).collect())
}

/// The form text is compared in. SQLite folds the case of English letters only, so the
/// library folds it here, for every alphabet.
pub fn fold(value: &str) -> String {
    value.to_lowercase()
}

/// A pattern of `LIKE ... ESCAPE '\'` that finds `word` anywhere, with its wildcards taken as
/// plain characters.
pub fn contains(word: &str) -> String {
    let mut pattern = String::with_capacity(word.len() + 2);
    pattern.push('%');
    for character in fold(word).chars() {
        if matches!(character, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

/// A number that makes sense as a tempo, a length or a rate: finite and above zero.
pub fn positive(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_covers_every_alphabet() {
        assert_eq!(fold("ÉCOLE Ωμέγα"), "école ωμέγα");
    }

    #[test]
    fn wildcards_are_plain_characters() {
        assert_eq!(contains("50%_off\\"), "%50\\%\\_off\\\\%");
    }

    #[test]
    fn tags_are_cut_and_empty_ones_dropped() {
        assert_eq!(tag(Some("")), None);
        let long = "é".repeat(MAX_TAG_CHARS + 5);
        assert_eq!(
            tag(Some(&long)).map(|t| t.chars().count()),
            Some(MAX_TAG_CHARS)
        );
        assert_eq!(positive(Some(f64::NAN)), None);
        assert_eq!(positive(Some(-1.0)), None);
        assert_eq!(positive(Some(128.0)), Some(128.0));
    }
}
