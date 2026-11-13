//! Keeps third-party material that must not enter the repository out of it (AGENTS.md,
//! "Clean-room").
//!
//! The patterns are stored in pieces and the test inputs are assembled at run time, so this
//! file passes its own check.

use std::path::Path;

use crate::failure::Failure;
use crate::repo;

const ATTRIBUTION_PATH: &str = "ATTRIBUTION.md";

/// The names of the reference product and of its vendor, which no tracked file may contain.
/// The vendor name is two words that may be joined by a space, a hyphen, an underscore or a
/// dot, as in a domain.
const PRODUCT: [&str; 2] = ["trak", "tor"];
const VENDOR: [&str; 2] = ["native", "instruments"];

/// Start of the sentence that grants a file under one of the licences below.
const GRANT_PREFIX: &str = "terms of the gnu";
const COPYLEFT_LICENCES: [&str; 4] = [
    "general public license",
    "affero general public license",
    "lesser general public license",
    "library general public license",
];
const SPDX_TAG: &str = "spdx-license-identifier:";
const SPDX_COPYLEFT: &str = "gpl";

const AUDIO_EXTENSIONS: [&str; 10] = [
    "aac", "aif", "aiff", "flac", "m4a", "mp3", "oga", "ogg", "opus", "wav",
];
/// Mapping and collection formats of the reference product.
const FOREIGN_EXTENSIONS: [&str; 2] = ["tsi", "nml"];
const FIXTURES_DIR: &str = "tests/fixtures/";

pub fn run(root: &Path) -> Result<(), Failure> {
    let attribution = repo::read_text(root, ATTRIBUTION_PATH)?;
    let files = repo::tracked_files(root)?;
    let mut findings = Vec::new();
    for path in &files {
        let Some(bytes) = repo::read_tracked(root, path)? else {
            continue;
        };
        findings.extend(file_findings(path, &bytes, &attribution));
    }
    Failure::from_findings("cleanroom", findings)?;
    println!("cleanroom: {} tracked file(s) checked", files.len());
    Ok(())
}

fn file_findings(path: &str, bytes: &[u8], attribution: &str) -> Vec<String> {
    let mut findings = Vec::new();
    if is_foreign_format(path) {
        findings.push(format!(
            "{path}: file format of the reference product outside `{FIXTURES_DIR}`"
        ));
    }
    if is_audio(path, bytes) && !attribution.contains(&format!("`{path}`")) {
        findings.push(format!(
            "{path}: audio file without a row in {ATTRIBUTION_PATH}"
        ));
    }
    if names_reference(path) {
        findings.push(format!(
            "{path}: path names the reference product or its vendor"
        ));
    }
    let text = String::from_utf8_lossy(bytes);
    findings.extend(
        text.lines()
            .enumerate()
            .filter(|(_, line)| names_reference(line))
            .map(|(index, _)| {
                format!(
                    "{path}:{}: names the reference product or its vendor",
                    index + 1
                )
            }),
    );
    if has_copyleft_header(&text) {
        findings.push(format!("{path}: copyleft licence header"));
    }
    findings
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map_or_else(String::new, |(_, extension)| extension.to_ascii_lowercase())
}

fn is_foreign_format(path: &str) -> bool {
    let inside_fixtures =
        path.starts_with(FIXTURES_DIR) || path.contains(&format!("/{FIXTURES_DIR}"));
    FOREIGN_EXTENSIONS.contains(&extension(path).as_str()) && !inside_fixtures
}

fn is_audio(path: &str, bytes: &[u8]) -> bool {
    AUDIO_EXTENSIONS.contains(&extension(path).as_str()) || has_audio_signature(bytes)
}

fn has_audio_signature(bytes: &[u8]) -> bool {
    matches!(
        bytes,
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'A',
            b'V',
            b'E',
            ..
        ] | [b'F', b'O', b'R', b'M', _, _, _, _, b'A', b'I', b'F', ..]
            | [_, _, _, _, b'f', b't', b'y', b'p', b'M', b'4', b'A', ..]
            | [b'f', b'L', b'a', b'C', ..]
            | [b'O', b'g', b'g', b'S', ..]
            | [b'I', b'D', b'3', ..]
    )
}

fn names_reference(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains(&PRODUCT.concat()) || names_vendor(&lower)
}

/// The first word of the vendor name must start a word, so that a longer word ending in it
/// and followed by the second word is not a finding.
fn names_vendor(lower: &str) -> bool {
    let [first, second] = VENDOR;
    lower.match_indices(first).any(|(at, _)| {
        let starts_word = lower
            .get(..at)
            .and_then(|before| before.chars().next_back())
            .is_none_or(|c| !c.is_alphanumeric());
        let rest = lower.get(at + first.len()..).unwrap_or_default();
        let rest =
            rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.'));
        starts_word && rest.starts_with(second)
    })
}

/// Licence headers wrap across comment lines, so the grant sentence is searched in the
/// text reduced to lowercase words.
fn has_copyleft_header(text: &str) -> bool {
    let words = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    let granted = COPYLEFT_LICENCES
        .iter()
        .any(|licence| words.contains(&format!("{GRANT_PREFIX} {licence}")));
    let tagged = text.lines().map(str::to_lowercase).any(|line| {
        line.split_once(SPDX_TAG)
            .is_some_and(|(_, expression)| expression.contains(SPDX_COPYLEFT))
    });
    granted || tagged
}

#[cfg(test)]
mod tests {
    use super::{PRODUCT, VENDOR, file_findings, has_copyleft_header, names_reference};

    const NO_ATTRIBUTION: &str = "";

    fn product() -> String {
        let [first, second] = PRODUCT;
        format!("{}{second}", first.to_uppercase())
    }

    fn vendor(separator: &str) -> String {
        let [first, second] = VENDOR;
        format!("{first}{separator}{second}")
    }

    #[test]
    fn flags_the_product_name_in_any_case() {
        assert!(names_reference(&format!("Notes on {}.", product())));
        assert!(names_reference(&product().to_lowercase()));
        assert!(names_reference(&product().to_uppercase()));
    }

    #[test]
    fn flags_the_vendor_name_with_any_separator() {
        for separator in [" ", "-", "_", ".", "  "] {
            let line = format!(
                "// Copyright 2024 {} GmbH",
                vendor(separator).to_uppercase()
            );
            assert!(names_reference(&line), "{line}");
        }
        let url = format!("<https://docs.{}.com/>", vendor("-"));
        assert!(names_reference(&url));
    }

    #[test]
    fn accepts_words_that_only_contain_the_vendor_name() {
        let [first, second] = VENDOR;
        assert!(!names_reference(&format!("alter{first} {second}")));
        assert!(!names_reference(&format!("{first} code and {second}")));
        assert!(!names_reference("The reference product and its vendor."));
        assert!(!names_reference("Copyright (c) The Platter contributors"));
    }

    #[test]
    fn flags_the_names_in_paths_and_text() {
        let in_path = format!("docs/{}.md", product().to_lowercase());
        assert_eq!(file_findings(&in_path, b"", NO_ATTRIBUTION).len(), 1);
        let text = format!("line one\nmade by {}\n", vendor(" "));
        let findings = file_findings("docs/notes.md", text.as_bytes(), NO_ATTRIBUTION);
        assert_eq!(
            findings,
            vec!["docs/notes.md:2: names the reference product or its vendor"]
        );
    }

    #[test]
    fn flags_a_wrapped_copyleft_grant() {
        let family = ["General", "Public", "License"].join(" ");
        let header = format!(
            "// it under the terms of the GNU\n// {family} as published by\n// the foundation"
        );
        assert!(has_copyleft_header(&header));
        let affero = format!("under the terms of the GNU Affero {family}");
        assert!(has_copyleft_header(&affero));
    }

    #[test]
    fn flags_a_copyleft_spdx_tag() {
        let tag = ["SPDX-License-Identifier", " AGPL-3.0-only"].join(":");
        assert!(has_copyleft_header(&format!("// {tag}\nfn main() {{}}")));
    }

    #[test]
    fn accepts_permissive_licences_and_plain_mentions() {
        let tag = ["SPDX-License-Identifier", " MIT OR Apache-2.0"].join(":");
        assert!(!has_copyleft_header(&tag));
        assert!(!has_copyleft_header(
            "No dependency may be under GPL, AGPL or LGPL."
        ));
    }

    #[test]
    fn requires_attribution_for_audio_files() {
        let riff = b"RIFF\x24\x00\x00\x00WAVEfmt ";
        assert_eq!(
            file_findings("assets/click.wav", riff, NO_ATTRIBUTION).len(),
            1
        );
        assert_eq!(
            file_findings("assets/renamed.bin", riff, NO_ATTRIBUTION).len(),
            1
        );
        assert_eq!(
            file_findings("assets/loop.MP3", b"", NO_ATTRIBUTION).len(),
            1
        );
        let attribution = "| `assets/click.wav` | synthesised | CC0-1.0 |";
        assert!(file_findings("assets/click.wav", riff, attribution).is_empty());
    }

    #[test]
    fn rejects_foreign_formats_outside_fixtures() {
        assert_eq!(
            file_findings("mappings/deck.tsi", b"", NO_ATTRIBUTION).len(),
            1
        );
        assert_eq!(
            file_findings("collection.NML", b"", NO_ATTRIBUTION).len(),
            1
        );
        assert!(file_findings("tests/fixtures/empty.nml", b"", NO_ATTRIBUTION).is_empty());
        let nested = "crates/library/tests/fixtures/empty.nml";
        assert!(file_findings(nested, b"", NO_ATTRIBUTION).is_empty());
    }

    #[test]
    fn accepts_ordinary_source_files() {
        let source = b"//! Three-band equaliser.\nfn main() {}\n";
        assert!(file_findings("crates/dsp/src/eq.rs", source, NO_ATTRIBUTION).is_empty());
    }
}
