//! Keeps third-party material that must not enter the repository out of it (AGENTS.md,
//! "Clean-room").
//!
//! The names and the file extensions that must stay out are stored as hashes and the test
//! inputs are stand-ins, so this file neither contains them nor fails its own check.

use std::path::Path;

use crate::failure::Failure;
use crate::repo;

const ATTRIBUTION_PATH: &str = "ATTRIBUTION.md";

/// The names of the documented applications and of their vendors, and their mapping and
/// collection formats, which no tracked file may contain.
const BANNED: Banned = Banned {
    product: Hashed {
        len: 7,
        hash: 0x295e_4e6a_d6f0_c3b0,
    },
    vendor: [
        Hashed {
            len: 6,
            hash: 0x4b70_1c18_7218_deca,
        },
        Hashed {
            len: 11,
            hash: 0x2321_4655_f673_8d39,
        },
    ],
    extensions: [
        Hashed {
            len: 3,
            hash: 0x5706_cd19_4470_4f7b,
        },
        Hashed {
            len: 3,
            hash: 0x2154_da19_258a_3a68,
        },
    ],
};

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
const FIXTURES_DIR: &str = "tests/fixtures/";

/// A lowercase ASCII word known only by its length and its 64-bit FNV-1a hash.
#[derive(Clone, Copy)]
struct Hashed {
    len: usize,
    hash: u64,
}

impl Hashed {
    fn matches(self, text: &[u8]) -> bool {
        text.len() == self.len && fnv1a(text) == self.hash
    }

    fn starts(self, text: &[u8]) -> bool {
        text.get(..self.len).is_some_and(|head| self.matches(head))
    }
}

/// The vendor name is two words that may be joined by a space, a hyphen, an underscore or a
/// dot, as in a domain.
struct Banned {
    product: Hashed,
    vendor: [Hashed; 2],
    extensions: [Hashed; 2],
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub fn run(root: &Path) -> Result<(), Failure> {
    let attribution = repo::read_text(root, ATTRIBUTION_PATH)?;
    let files = repo::tracked_files(root)?;
    let mut findings = Vec::new();
    for path in &files {
        let Some(bytes) = repo::read_tracked(root, path)? else {
            continue;
        };
        findings.extend(file_findings(path, &bytes, &attribution, &BANNED));
    }
    Failure::from_findings("cleanroom", findings)?;
    println!("cleanroom: {} tracked file(s) checked", files.len());
    Ok(())
}

fn file_findings(path: &str, bytes: &[u8], attribution: &str, banned: &Banned) -> Vec<String> {
    let mut findings = Vec::new();
    if is_foreign_format(path, banned) {
        findings.push(format!(
            "{path}: file format of a documented application outside `{FIXTURES_DIR}`"
        ));
    }
    if is_audio(path, bytes) && !attribution.contains(&format!("`{path}`")) {
        findings.push(format!(
            "{path}: audio file without a row in {ATTRIBUTION_PATH}"
        ));
    }
    if names_reference(path, banned) {
        findings.push(format!(
            "{path}: path names a documented application or its vendor"
        ));
    }
    let text = String::from_utf8_lossy(bytes);
    findings.extend(
        text.lines()
            .enumerate()
            .filter(|(_, line)| names_reference(line, banned))
            .map(|(index, _)| {
                format!(
                    "{path}:{}: names a documented application or its vendor",
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

fn is_foreign_format(path: &str, banned: &Banned) -> bool {
    let inside_fixtures =
        path.starts_with(FIXTURES_DIR) || path.contains(&format!("/{FIXTURES_DIR}"));
    let extension = extension(path);
    banned
        .extensions
        .iter()
        .any(|foreign| foreign.matches(extension.as_bytes()))
        && !inside_fixtures
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

fn names_reference(text: &str, banned: &Banned) -> bool {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let names_product = (0..bytes.len()).any(|at| {
        bytes
            .get(at..)
            .is_some_and(|rest| banned.product.starts(rest))
    });
    names_product || names_vendor(&lower, banned.vendor)
}

/// The first word of the vendor name must start a word, so that a longer word ending in it
/// and followed by the second word is not a finding.
fn names_vendor(lower: &str, [first, second]: [Hashed; 2]) -> bool {
    let bytes = lower.as_bytes();
    (0..bytes.len()).any(|at| {
        let at_first = bytes.get(at..).is_some_and(|rest| first.starts(rest));
        if !at_first {
            return false;
        }
        let starts_word = lower
            .get(..at)
            .and_then(|before| before.chars().next_back())
            .is_none_or(|c| !c.is_alphanumeric());
        let rest = lower.get(at + first.len..).unwrap_or_default();
        let rest =
            rest.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '_' | '.'));
        starts_word && second.starts(rest.as_bytes())
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
    use super::{Banned, Hashed, file_findings, fnv1a, has_copyleft_header, names_reference};

    const NO_ATTRIBUTION: &str = "";
    const PRODUCT: &str = "zorbix";
    const VENDOR: [&str; 2] = ["kranz", "audiowerk"];

    fn hashed(word: &str) -> Hashed {
        Hashed {
            len: word.len(),
            hash: fnv1a(word.as_bytes()),
        }
    }

    /// Stand-ins for the real names and formats, which this file must not contain.
    fn banned() -> Banned {
        let [first, second] = VENDOR;
        Banned {
            product: hashed(PRODUCT),
            vendor: [hashed(first), hashed(second)],
            extensions: [hashed("zzq"), hashed("qqz")],
        }
    }

    fn findings(path: &str, bytes: &[u8], attribution: &str) -> Vec<String> {
        file_findings(path, bytes, attribution, &banned())
    }

    fn names(text: &str) -> bool {
        names_reference(text, &banned())
    }

    fn product() -> String {
        let mut letters = PRODUCT.chars();
        letters.next().map_or_else(String::new, |first| {
            format!("{}{}", first.to_ascii_uppercase(), letters.as_str())
        })
    }

    fn vendor(separator: &str) -> String {
        let [first, second] = VENDOR;
        format!("{first}{separator}{second}")
    }

    #[test]
    fn hashes_the_published_test_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn flags_the_product_name_in_any_case() {
        assert!(names(&format!("Notes on {}.", product())));
        assert!(names(&product().to_lowercase()));
        assert!(names(&product().to_uppercase()));
        assert!(names(&format!("my{}pro", product())));
    }

    #[test]
    fn flags_the_vendor_name_with_any_separator() {
        for separator in [" ", "-", "_", ".", "  "] {
            let line = format!(
                "// Copyright 2024 {} GmbH",
                vendor(separator).to_uppercase()
            );
            assert!(names(&line), "{line}");
        }
        let url = format!("<https://docs.{}.com/>", vendor("-"));
        assert!(names(&url));
    }

    #[test]
    fn accepts_words_that_only_contain_the_vendor_name() {
        let [first, second] = VENDOR;
        assert!(!names(&format!("alter{first} {second}")));
        assert!(!names(&format!("{first} code and {second}")));
        assert!(!names("A documented application and its vendor."));
        assert!(!names("Copyright (c) The Platter contributors"));
    }

    #[test]
    fn flags_the_names_in_paths_and_text() {
        let in_path = format!("docs/{}.md", product().to_lowercase());
        assert_eq!(findings(&in_path, b"", NO_ATTRIBUTION).len(), 1);
        let text = format!("line one\nmade by {}\n", vendor(" "));
        assert_eq!(
            findings("docs/notes.md", text.as_bytes(), NO_ATTRIBUTION),
            vec!["docs/notes.md:2: names a documented application or its vendor"]
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
        assert_eq!(findings("assets/click.wav", riff, NO_ATTRIBUTION).len(), 1);
        assert_eq!(
            findings("assets/renamed.bin", riff, NO_ATTRIBUTION).len(),
            1
        );
        assert_eq!(findings("assets/loop.MP3", b"", NO_ATTRIBUTION).len(), 1);
        let attribution = "| `assets/click.wav` | synthesised | CC0-1.0 |";
        assert!(findings("assets/click.wav", riff, attribution).is_empty());
    }

    #[test]
    fn rejects_foreign_formats_outside_fixtures() {
        assert_eq!(findings("mappings/deck.zzq", b"", NO_ATTRIBUTION).len(), 1);
        assert_eq!(findings("collection.QQZ", b"", NO_ATTRIBUTION).len(), 1);
        assert!(findings("tests/fixtures/empty.qqz", b"", NO_ATTRIBUTION).is_empty());
        let nested = "crates/library/tests/fixtures/empty.qqz";
        assert!(findings(nested, b"", NO_ATTRIBUTION).is_empty());
    }

    #[test]
    fn accepts_ordinary_source_files() {
        let source = b"//! Three-band equaliser.\nfn main() {}\n";
        assert!(findings("crates/dsp/src/eq.rs", source, NO_ATTRIBUTION).is_empty());
    }
}
