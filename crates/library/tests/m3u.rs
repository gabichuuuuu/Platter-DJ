//! Reading and writing playlists in the M3U format, on text and bytes made in the tests.
//! Behaviour note: `docs/behavior/library.md`, section *M3U files*.

#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use platter_library::{M3uEntry, M3uError, MAX_M3U_BYTES, MAX_M3U_ENTRIES, read_m3u, write_m3u};

fn folder() -> PathBuf {
    PathBuf::from("lists")
}

fn read(text: &str) -> platter_library::M3uReading {
    read_m3u(text.as_bytes(), &folder()).expect("small input")
}

fn plain(path: &Path) -> M3uEntry {
    M3uEntry {
        path: path.to_path_buf(),
        artist: None,
        title: None,
        seconds: None,
    }
}

#[test]
fn round_trip_keeps_paths_tags_and_whole_seconds() {
    let root = std::env::temp_dir().join("platter-m3u-round-trip");
    let entries = vec![
        M3uEntry {
            path: root.join("one.mp3"),
            artist: Some("Artist".to_owned()),
            title: Some("Title".to_owned()),
            seconds: Some(215.4),
        },
        M3uEntry {
            path: root.join("sub dir").join("two é.flac"),
            artist: None,
            title: Some("Only a title".to_owned()),
            seconds: None,
        },
        plain(&root.join("three.wav")),
    ];
    let text = write_m3u(&entries);
    assert!(text.starts_with("#EXTM3U\n"));
    assert!(!text.contains('\r'));

    let reading = read_m3u(text.as_bytes(), &folder()).expect("small input");
    assert_eq!(reading.skipped, 0);
    let mut expected = entries;
    expected[0].seconds = Some(215.0);
    assert_eq!(reading.entries, expected);
}

#[test]
fn relative_paths_resolve_under_the_folder_with_either_separator() {
    let reading = read("a/b.mp3\nc\\d.mp3\n.\\e.mp3\nf//g.mp3\n");
    let base = folder();
    assert_eq!(
        reading
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>(),
        vec![
            base.join("a").join("b.mp3"),
            base.join("c").join("d.mp3"),
            base.join("e.mp3"),
            base.join("f").join("g.mp3"),
        ]
    );
    assert_eq!(reading.skipped, 0);
}

#[test]
fn absolute_paths_are_kept_as_written() {
    let paths = [
        "/music/a.mp3",
        "C:\\Music\\a.mp3",
        "C:/Music/a.mp3",
        "\\\\server\\share\\a.mp3",
    ];
    let reading = read(&paths.join("\n"));
    let read_paths: Vec<_> = reading
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    let expected: Vec<_> = paths.iter().map(PathBuf::from).collect();
    assert_eq!(read_paths, expected);
}

#[test]
fn a_drive_without_a_separator_is_skipped() {
    let reading = read("C:a.mp3\nok.mp3\n");
    assert_eq!(reading.entries.len(), 1);
    assert_eq!(reading.skipped, 1);
}

#[test]
fn bom_and_crlf_and_bare_cr_are_accepted() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"#EXTM3U\r\n#EXTINF:100,A - B\r\none.mp3\rtwo.mp3\r\n\r\n");
    let reading = read_m3u(&bytes, &folder()).expect("small input");
    assert_eq!(reading.entries.len(), 2);
    assert_eq!(reading.entries[0].path, folder().join("one.mp3"));
    assert_eq!(reading.entries[0].artist.as_deref(), Some("A"));
    assert_eq!(reading.entries[0].title.as_deref(), Some("B"));
    assert_eq!(reading.entries[0].seconds, Some(100.0));
    assert_eq!(reading.entries[1], plain(&folder().join("two.mp3")));
}

#[test]
fn bytes_that_are_not_utf8_are_read_as_latin_1() {
    let mut bytes = b"#EXTINF:10,Caf".to_vec();
    bytes.push(0xE9);
    bytes.extend_from_slice(b"\ncaf");
    bytes.push(0xE9);
    bytes.extend_from_slice(b".mp3\n");
    let reading = read_m3u(&bytes, &folder()).expect("small input");
    assert_eq!(reading.entries.len(), 1);
    assert_eq!(reading.entries[0].title.as_deref(), Some("Café"));
    assert_eq!(reading.entries[0].path, folder().join("café.mp3"));
}

#[test]
fn utf8_text_is_read_as_utf8() {
    let reading = read("#EXTINF:10,Ünï - Tïtle\nünï.mp3\n");
    assert_eq!(reading.entries[0].artist.as_deref(), Some("Ünï"));
    assert_eq!(reading.entries[0].path, folder().join("ünï.mp3"));
}

#[test]
fn extinf_applies_only_to_the_next_entry() {
    let reading = read("#EXTINF:60,A - B\none.mp3\ntwo.mp3\n# a comment\n\nthree.mp3\n");
    assert_eq!(reading.entries.len(), 3);
    assert_eq!(reading.entries[0].title.as_deref(), Some("B"));
    assert_eq!(reading.entries[1].title, None);
    assert_eq!(reading.entries[1].seconds, None);
    assert_eq!(reading.entries[2].artist, None);
}

#[test]
fn extinf_waits_across_comments_and_blank_lines() {
    let reading = read("#EXTINF:60,A - B\n\n#EXTGRP:x\none.mp3\n");
    assert_eq!(reading.entries[0].seconds, Some(60.0));
}

#[test]
fn extinf_is_used_up_by_a_skipped_entry() {
    let reading = read("#EXTINF:60,A - B\nhttp://example.test/x.mp3\ntwo.mp3\n");
    assert_eq!(reading.skipped, 1);
    assert_eq!(reading.entries.len(), 1);
    assert_eq!(reading.entries[0].title, None);
}

#[test]
fn unknown_or_invalid_seconds_are_none() {
    let reading = read(
        "#EXTINF:-1,T\na.mp3\n#EXTINF:NaN,T\nb.mp3\n#EXTINF:-5,T\nc.mp3\n#EXTINF:0,T\nd.mp3\n\
         #EXTINF:inf,T\ne.mp3\n#EXTINF:abc,T\nf.mp3\n#EXTINF:12.5,T\ng.mp3\n#EXTINF:30\nh.mp3\n",
    );
    let seconds: Vec<_> = reading.entries.iter().map(|entry| entry.seconds).collect();
    assert_eq!(
        seconds,
        vec![None, None, None, None, None, None, Some(12.5), Some(30.0)]
    );
    assert_eq!(reading.entries[0].title.as_deref(), Some("T"));
    assert_eq!(reading.entries[7].title, None);
}

#[test]
fn a_display_without_a_separator_is_a_title() {
    let reading = read("#EXTINF:10,Just a title\na.mp3\n#EXTINF:10,Well-known-name\nb.mp3\n");
    assert_eq!(reading.entries[0].artist, None);
    assert_eq!(reading.entries[0].title.as_deref(), Some("Just a title"));
    assert_eq!(reading.entries[1].artist, None);
    assert_eq!(reading.entries[1].title.as_deref(), Some("Well-known-name"));
}

#[test]
fn the_display_splits_at_the_first_separator_only() {
    let reading =
        read("#EXTINF:10,  A  -  B - C  \na.mp3\n#EXTINF:10, - B\nb.mp3\n#EXTINF:10,\nc.mp3\n");
    assert_eq!(reading.entries[0].artist.as_deref(), Some("A"));
    assert_eq!(reading.entries[0].title.as_deref(), Some("B - C"));
    assert_eq!(reading.entries[1].artist, None);
    assert_eq!(reading.entries[1].title.as_deref(), Some("B"));
    assert_eq!(reading.entries[2].title, None);
    assert_eq!(reading.entries[2].seconds, Some(10.0));
}

#[test]
fn other_url_schemes_are_skipped_and_counted() {
    let reading = read(
        "http://example.test/a.mp3\nhttps://example.test/b.mp3\nrtsp://example.test/c\n\
         x+y.z-1://host/d\nok.mp3\n",
    );
    assert_eq!(reading.entries.len(), 1);
    assert_eq!(reading.skipped, 4);
}

#[test]
fn file_urls_are_decoded() {
    let reading = read(
        "file:///home/my%20music/caf%C3%A9.mp3\nfile:///C:/My%20Music/a.mp3\n\
         FILE://localhost/home/b.mp3\nfile:///D:/c.mp3\n",
    );
    let paths: Vec<_> = reading
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/home/my music/café.mp3"),
            PathBuf::from("C:/My Music/a.mp3"),
            PathBuf::from("/home/b.mp3"),
            PathBuf::from("D:/c.mp3"),
        ]
    );
    assert_eq!(reading.skipped, 0);
}

#[test]
fn a_file_url_with_another_host_is_a_unc_path() {
    let reading = read("file://server/share/a.mp3\n");
    assert_eq!(
        reading.entries[0].path,
        PathBuf::from("\\\\server/share/a.mp3")
    );
}

#[test]
fn broken_escapes_and_bytes_that_are_not_utf8_are_skipped() {
    let reading = read("file:///a%2.mp3\nfile:///a%zz.mp3\nfile:///a%\nfile:///a%FF.mp3\nok.mp3\n");
    assert_eq!(reading.entries.len(), 1);
    assert_eq!(reading.skipped, 4);
}

#[test]
fn empty_paths_and_nul_characters_are_skipped() {
    let reading = read("file://\n./\nbad\0name.mp3\nok.mp3\n");
    assert_eq!(reading.entries.len(), 1);
    assert_eq!(reading.skipped, 3);
}

#[test]
fn entry_lines_are_trimmed() {
    let reading = read("   a.mp3 \t\n");
    assert_eq!(reading.entries[0].path, folder().join("a.mp3"));
}

#[test]
fn an_oversize_file_is_refused() {
    let bytes = vec![b'a'; MAX_M3U_BYTES + 1];
    assert_eq!(read_m3u(&bytes, &folder()), Err(M3uError::TooLarge));
    assert!(read_m3u(&bytes[..MAX_M3U_BYTES], &folder()).is_ok());
    assert!(!M3uError::TooLarge.to_string().is_empty());
}

#[test]
fn the_entry_cap_counts_the_rest_as_skipped() {
    let mut text = String::new();
    for _ in 0..MAX_M3U_ENTRIES + 3 {
        text.push_str("a.mp3\n");
    }
    let reading = read_m3u(text.as_bytes(), &folder()).expect("under the size cap");
    assert_eq!(reading.entries.len(), MAX_M3U_ENTRIES);
    assert_eq!(reading.skipped, 3);
}

#[test]
fn a_newline_in_a_title_is_replaced_on_write() {
    let entry = M3uEntry {
        path: PathBuf::from("a.mp3"),
        artist: Some("Ar\r\ntist".to_owned()),
        title: Some("Ti\ntle\rx".to_owned()),
        seconds: Some(10.0),
    };
    let text = write_m3u(&[entry]);
    assert_eq!(text, "#EXTM3U\n#EXTINF:10,Ar  tist - Ti tle x\na.mp3\n");
    let reading = read(&text);
    assert_eq!(reading.entries.len(), 1);
    assert_eq!(reading.entries[0].title.as_deref(), Some("Ti tle x"));
}

#[test]
fn the_display_and_the_seconds_of_a_written_entry() {
    let entries = vec![
        M3uEntry {
            path: PathBuf::from("a.mp3"),
            artist: Some("A".to_owned()),
            title: None,
            seconds: Some(f64::NAN),
        },
        M3uEntry {
            path: PathBuf::from("b.mp3"),
            artist: None,
            title: None,
            seconds: Some(-3.0),
        },
        M3uEntry {
            path: PathBuf::from("c.mp3"),
            artist: None,
            title: Some("T".to_owned()),
            seconds: Some(0.4),
        },
        M3uEntry {
            path: PathBuf::from("d.mp3"),
            artist: Some(String::new()),
            title: Some("T".to_owned()),
            seconds: Some(59.5),
        },
    ];
    assert_eq!(
        write_m3u(&entries),
        "#EXTM3U\n#EXTINF:-1,A\na.mp3\n#EXTINF:-1,\nb.mp3\n#EXTINF:-1,T\nc.mp3\n#EXTINF:60,T\nd.mp3\n"
    );
}

#[test]
fn an_empty_list_writes_only_the_header() {
    assert_eq!(write_m3u(&[]), "#EXTM3U\n");
    let reading = read("#EXTM3U\n");
    assert!(reading.entries.is_empty());
    assert_eq!(reading.skipped, 0);
}
