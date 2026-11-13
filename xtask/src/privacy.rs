//! Keeps traces of the development machine out of the repository (AGENTS.md, "No trace of
//! the development machine").
//!
//! The names of the current user, machine and domain are read from the environment at run
//! time and never written here. The test inputs are invented and assembled at run time, so
//! this file passes its own check.

use std::ffi::OsStr;
use std::path::Path;

use crate::failure::Failure;
use crate::repo;

/// Variables that name the user, the machine or its domain on the supported systems.
const IDENTITY_VARIABLES: [&str; 7] = [
    "USERNAME",
    "USER",
    "LOGNAME",
    "COMPUTERNAME",
    "HOSTNAME",
    "USERDOMAIN",
    "USERDNSDOMAIN",
];

const UNIX_HOME_ROOTS: [&str; 2] = ["/home/", "/Users/"];

const FIELD_SEPARATOR: char = '\u{1f}';
const RECORD_SEPARATOR: char = '\u{1e}';
const LOG_ARGS: [&str; 3] = [
    "log",
    "--all",
    "--format=%h%x1f%an%x1f%ae%x1f%cn%x1f%ce%x1f%B%x1e",
];
const COMMIT_FIELDS: [&str; 5] = [
    "author name",
    "author email",
    "committer name",
    "committer email",
    "message",
];

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// Text, EXIF and modification-time chunks.
const PNG_METADATA_CHUNKS: [[u8; 4]; 5] = [*b"tEXt", *b"zTXt", *b"iTXt", *b"eXIf", *b"tIME"];
const SVG_METADATA_MARKS: [&str; 3] = ["<metadata", "sodipodi:", "inkscape:"];
const UNCHECKED_IMAGE_EXTENSIONS: [&str; 8] =
    ["avif", "bmp", "gif", "heic", "ico", "tif", "tiff", "webp"];

/// Names that identify the machine running the check, with the variable each came from.
struct LocalIdentity {
    names: Vec<(&'static str, String)>,
}

impl LocalIdentity {
    /// In CI the environment describes the runner, not a development machine, and its
    /// generic names would match ordinary words of the documentation.
    fn from_env() -> Self {
        if std::env::var("CI").is_ok_and(|value| value == "true") {
            println!("privacy: CI environment, the local identity check is skipped");
            return Self { names: Vec::new() };
        }
        let values = IDENTITY_VARIABLES
            .iter()
            .filter_map(|variable| Some((*variable, std::env::var(variable).ok()?)));
        Self::new(values)
    }

    fn new(values: impl IntoIterator<Item = (&'static str, String)>) -> Self {
        let mut names: Vec<(&'static str, String)> = Vec::new();
        for (variable, value) in values {
            let value = value.trim().to_lowercase();
            if !value.is_empty() && names.iter().all(|(_, known)| *known != value) {
                names.push((variable, value));
            }
        }
        Self { names }
    }

    /// The variable whose value appears in the text as a whole word, if any.
    fn found_in(&self, text: &str) -> Option<&'static str> {
        let lower = text.to_lowercase();
        self.names
            .iter()
            .find(|(_, name)| contains_word(&lower, name))
            .map(|(variable, _)| *variable)
    }
}

fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(at, _)| {
        let before = text.get(..at).and_then(|head| head.chars().next_back());
        let after = text
            .get(at + word.len()..)
            .and_then(|tail| tail.chars().next());
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

pub fn run(root: &Path) -> Result<(), Failure> {
    let identity = LocalIdentity::from_env();
    let files = repo::tracked_files(root)?;
    let mut findings = Vec::new();
    for path in &files {
        let Some(bytes) = repo::read_tracked(root, path)? else {
            continue;
        };
        findings.extend(file_findings(path, &bytes, &identity));
    }
    let log = repo::capture(root, OsStr::new("git"), &LOG_ARGS)?;
    let log = String::from_utf8_lossy(&log);
    findings.extend(history_findings(&log, &identity));
    Failure::from_findings("privacy", findings)?;
    println!(
        "privacy: {} tracked file(s) and {} commit(s) checked",
        files.len(),
        commits(&log).count()
    );
    Ok(())
}

fn file_findings(path: &str, bytes: &[u8], identity: &LocalIdentity) -> Vec<String> {
    let mut findings = Vec::new();
    if let Some(problem) = text_problem(path, identity) {
        findings.push(format!("{path}: the file name {problem}"));
    }
    let text = String::from_utf8_lossy(bytes);
    findings.extend(text.lines().enumerate().filter_map(|(index, line)| {
        text_problem(line, identity).map(|problem| format!("{path}:{}: {problem}", index + 1))
    }));
    if let Some(problem) = image_problem(path, bytes) {
        findings.push(format!("{path}: {problem}"));
    }
    findings
}

fn commits(log: &str) -> impl Iterator<Item = &str> {
    log.split(RECORD_SEPARATOR)
        .map(str::trim)
        .filter(|record| !record.is_empty())
}

fn history_findings(log: &str, identity: &LocalIdentity) -> Vec<String> {
    let mut findings = Vec::new();
    for record in commits(log) {
        let mut fields = record.split(FIELD_SEPARATOR);
        let id = fields.next().unwrap_or_default();
        for (label, value) in COMMIT_FIELDS.iter().zip(fields) {
            if let Some(problem) = value.lines().find_map(|line| text_problem(line, identity)) {
                findings.push(format!("commit {id}: the {label} {problem}"));
            }
            if label.ends_with("email") && is_generated_email(value) {
                findings.push(format!(
                    "commit {id}: the {label} was generated by the system"
                ));
            }
        }
    }
    findings
}

/// Git falls back to `user@host.(none)` or a local host name when no identity is set.
fn is_generated_email(email: &str) -> bool {
    let lower = email.trim().to_ascii_lowercase();
    let host = lower.rsplit_once('@').map_or("", |(_, host)| host);
    lower.contains("(none)") || host.ends_with(".local") || host.ends_with(".localdomain")
}

fn text_problem(line: &str, identity: &LocalIdentity) -> Option<String> {
    if has_windows_home(line) {
        return Some("contains an absolute Windows user path".to_owned());
    }
    if has_unix_home(line) {
        return Some("contains an absolute Unix user path".to_owned());
    }
    identity
        .found_in(line)
        .map(|variable| format!("names the current user, machine or domain (value of {variable})"))
}

fn is_separator(byte: u8) -> bool {
    matches!(byte, b'\\' | b'/')
}

/// A drive, the users folder and the first character of a name, with single, doubled or
/// forward separators: `X:\Users\<name>`.
fn has_windows_home(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.match_indices("users").any(|(at, word)| {
        let before = lower.as_bytes().get(..at).unwrap_or_default();
        let after = lower.as_bytes().get(at + word.len()..).unwrap_or_default();
        ends_with_drive(before) && starts_with_name(after)
    })
}

fn ends_with_drive(before: &[u8]) -> bool {
    let mut rest = before;
    while let [head @ .., last] = rest
        && is_separator(*last)
    {
        rest = head;
    }
    rest.len() < before.len() && matches!(rest, [.., letter, b':'] if letter.is_ascii_alphabetic())
}

fn starts_with_name(after: &[u8]) -> bool {
    let mut rest = after;
    while let [first, tail @ ..] = rest
        && is_separator(*first)
    {
        rest = tail;
    }
    rest.len() < after.len() && rest.first().is_some_and(u8::is_ascii_alphanumeric)
}

/// A home root followed by the first character of a name: `/home/<name>`.
fn has_unix_home(line: &str) -> bool {
    let bytes = line.as_bytes();
    UNIX_HOME_ROOTS.iter().any(|root| {
        line.match_indices(root).any(|(at, _)| {
            let before = bytes.get(..at).unwrap_or_default();
            let after = bytes.get(at + root.len()..).unwrap_or_default();
            starts_a_path(before) && after.first().is_some_and(u8::is_ascii_alphanumeric)
        })
    })
}

/// The root starts a path unless it continues a longer path or a URL. A single letter
/// before it is a drive in the spelling some Unix-like shells use on Windows.
fn starts_a_path(before: &[u8]) -> bool {
    match before {
        [] => true,
        [.., b'/', drive] if drive.is_ascii_alphabetic() => true,
        [.., last] => !(last.is_ascii_alphanumeric() || matches!(last, b'.' | b'_' | b'-' | b'~')),
    }
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map_or_else(String::new, |(_, extension)| extension.to_ascii_lowercase())
}

fn image_problem(path: &str, bytes: &[u8]) -> Option<&'static str> {
    match extension(path).as_str() {
        "png" => png_problem(bytes),
        "jpg" | "jpeg" => jpeg_problem(bytes),
        "svg" => svg_problem(bytes),
        other if UNCHECKED_IMAGE_EXTENSIONS.contains(&other) => {
            Some("image format the metadata check cannot read; use PNG, JPEG or SVG")
        }
        _ => None,
    }
}

/// A chunk is a 4-byte big-endian length, a 4-byte type, the data and a 4-byte CRC.
fn png_problem(bytes: &[u8]) -> Option<&'static str> {
    let Some(mut rest) = bytes.strip_prefix(&PNG_SIGNATURE) else {
        return Some("is not a PNG file despite its extension");
    };
    while let [l0, l1, l2, l3, t0, t1, t2, t3, tail @ ..] = rest {
        if PNG_METADATA_CHUNKS.contains(&[*t0, *t1, *t2, *t3]) {
            return Some("carries PNG metadata (text, EXIF or modification time)");
        }
        let length = usize::try_from(u32::from_be_bytes([*l0, *l1, *l2, *l3])).ok();
        let next = length.and_then(|length| length.checked_add(4));
        let Some(next) = next.and_then(|skip| tail.get(skip..)) else {
            return Some("is a truncated PNG file");
        };
        rest = next;
    }
    None
}

/// Before the image data, a segment is a marker (`FF xx`) and a 2-byte big-endian length
/// that counts itself.
fn jpeg_problem(bytes: &[u8]) -> Option<&'static str> {
    let Some(mut rest) = bytes.strip_prefix(&[0xFF, 0xD8]) else {
        return Some("is not a JPEG file despite its extension");
    };
    while let [0xFF, marker, high, low, ..] = rest {
        match *marker {
            // Start of scan: the image data follows and holds no further segments.
            0xDA => return None,
            0xE1 | 0xED | 0xFE => {
                return Some("carries JPEG metadata (EXIF, XMP, IPTC or a comment)");
            }
            _ => {}
        }
        let length = usize::from(u16::from_be_bytes([*high, *low]));
        let Some(next) = rest.get(2 + length..) else {
            return Some("is a truncated JPEG file");
        };
        rest = next;
    }
    Some("is a JPEG file without image data")
}

fn svg_problem(bytes: &[u8]) -> Option<&'static str> {
    let text = String::from_utf8_lossy(bytes);
    SVG_METADATA_MARKS
        .iter()
        .any(|mark| text.contains(mark))
        .then_some("carries SVG metadata or editor attributes")
}

#[cfg(test)]
mod tests {
    use super::{
        FIELD_SEPARATOR, LocalIdentity, PNG_SIGNATURE, RECORD_SEPARATOR, file_findings,
        history_findings, image_problem, text_problem,
    };

    const NOBODY: LocalIdentity = LocalIdentity { names: Vec::new() };

    fn invented_identity() -> LocalIdentity {
        LocalIdentity::new([
            ("USERNAME", "quietfox".to_owned()),
            ("COMPUTERNAME", "LAB-BENCH-07".to_owned()),
            ("USERDOMAIN", "LAB-BENCH-07".to_owned()),
            ("HOSTNAME", "  ".to_owned()),
        ])
    }

    /// A user path, assembled here so that the source of this test holds none.
    fn user_path(prefix: &str, separator: &str, root: &str) -> String {
        [prefix, root, "quietfox", "project"].join(separator)
    }

    #[test]
    fn flags_windows_user_paths_in_every_spelling() {
        for separator in ["\\", "\\\\", "/"] {
            let line = format!("opened {}", user_path("D:", separator, "Users"));
            assert!(text_problem(&line, &NOBODY).is_some(), "{line}");
        }
        let lowercase = user_path("c:", "\\", "users");
        assert!(text_problem(&lowercase, &NOBODY).is_some());
    }

    #[test]
    fn flags_unix_user_paths() {
        for root in ["home", "Users"] {
            let line = format!("cache={}", user_path("", "/", root));
            assert!(text_problem(&line, &NOBODY).is_some(), "{line}");
        }
        let shell_drive = user_path("/c", "/", "Users");
        assert!(text_problem(&shell_drive, &NOBODY).is_some());
    }

    #[test]
    fn accepts_placeholders_urls_and_relative_paths() {
        let placeholder = ["X:", "Users", "<name>"].join("\\");
        let url = ["https://example.org", "home", "news"].join("/");
        let relative = ["docs", "home", "index.md"].join("/");
        for line in [
            placeholder.as_str(),
            url.as_str(),
            relative.as_str(),
            "the users table",
        ] {
            assert_eq!(text_problem(line, &NOBODY), None, "{line}");
        }
    }

    #[test]
    fn flags_the_local_names_as_whole_words_only() {
        let identity = invented_identity();
        assert!(text_problem("built by QuietFox on monday", &identity).is_some());
        assert!(text_problem("host: lab-bench-07", &identity).is_some());
        assert_eq!(text_problem("the quietfoxes of the north", &identity), None);
        assert_eq!(text_problem("nothing to see", &identity), None);
    }

    #[test]
    fn reports_the_variable_and_not_the_value() {
        let problem = text_problem("owner quietfox", &invented_identity()).unwrap_or_default();
        assert!(problem.contains("USERNAME"));
        assert!(!problem.contains("quietfox"));
    }

    #[test]
    fn checks_file_names_and_every_line() {
        let identity = invented_identity();
        let found = file_findings("notes/quietfox.txt", b"one\nlab-bench-07\n", &identity);
        assert_eq!(found.len(), 2);
        assert!(found[1].starts_with("notes/quietfox.txt:2:"));
    }

    fn log_record(fields: [&str; 6]) -> String {
        format!(
            "{}{RECORD_SEPARATOR}\n",
            fields.join(&FIELD_SEPARATOR.to_string())
        )
    }

    #[test]
    fn accepts_a_history_with_a_proper_identity() {
        let address = "1234+pat@users.noreply.example.org";
        let log = log_record([
            "abc1234",
            "Pat",
            address,
            "Pat",
            address,
            "feat: add gate\n\nM0",
        ]);
        assert!(history_findings(&log, &invented_identity()).is_empty());
        assert!(history_findings("", &invented_identity()).is_empty());
    }

    #[test]
    fn flags_generated_git_identities() {
        for host in ["lab.local", "lab.localdomain", "lab.(none)"] {
            let address = format!("pat@{host}");
            let log = log_record(["abc1234", "Pat", &address, "Pat", "pat@example.org", "fix"]);
            let found = history_findings(&log, &NOBODY);
            assert_eq!(found.len(), 1, "{host}");
            assert!(found[0].contains("author email"));
        }
    }

    #[test]
    fn flags_local_traces_in_commit_fields() {
        let message = format!("fix: build\n\nfailed in {}", user_path("", "/", "home"));
        let log = log_record([
            "abc1234",
            "quietfox",
            "pat@example.org",
            "Pat",
            "pat@lab-bench-07.example.org",
            &message,
        ]);
        let found = history_findings(&log, &invented_identity());
        assert_eq!(found.len(), 3);
        assert!(found[0].contains("author name"));
        assert!(found[1].contains("committer email"));
        assert!(found[2].contains("message"));
    }

    fn png(chunks: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        for (kind, data) in chunks {
            let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
            bytes.extend(length.to_be_bytes());
            bytes.extend(*kind);
            bytes.extend(*data);
            bytes.extend([0; 4]);
        }
        bytes
    }

    #[test]
    fn accepts_a_png_without_metadata() {
        let image = png(&[(b"IHDR", &[0; 13]), (b"IDAT", &[1, 2, 3]), (b"IEND", &[])]);
        assert_eq!(image_problem("shot.png", &image), None);
    }

    #[test]
    fn flags_png_metadata_and_broken_files() {
        let tagged = png(&[
            (b"IHDR", &[0; 13]),
            (b"tEXt", b"Author\0someone"),
            (b"IEND", &[]),
        ]);
        assert!(image_problem("shot.PNG", &tagged).is_some());
        let mut truncated = png(&[(b"IHDR", &[0; 13]), (b"IDAT", &[1, 2, 3])]);
        truncated.truncate(truncated.len() - 5);
        assert!(image_problem("shot.png", &truncated).is_some());
        assert!(image_problem("shot.png", b"plain text").is_some());
    }

    #[test]
    fn checks_jpeg_segments_up_to_the_image_data() {
        let clean = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46, 0xFF, 0xDA, 0x00, 0x02,
        ];
        assert_eq!(image_problem("photo.jpg", &clean), None);
        let exif = [
            0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x04, 0x45, 0x78, 0xFF, 0xDA, 0x00, 0x02,
        ];
        assert!(image_problem("photo.jpeg", &exif).is_some());
        let cut = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x40, 0x4A];
        assert!(image_problem("photo.jpg", &cut).is_some());
    }

    #[test]
    fn checks_svg_and_rejects_formats_it_cannot_read() {
        assert_eq!(
            image_problem("mark.svg", b"<svg><path d=\"M0 0\"/></svg>"),
            None
        );
        assert!(image_problem("mark.svg", b"<svg><metadata>x</metadata></svg>").is_some());
        assert!(image_problem("shot.webp", b"RIFF").is_some());
        assert_eq!(image_problem("notes.md", b"# notes"), None);
    }
}
