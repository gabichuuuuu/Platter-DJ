//! Runs the binary: `info` on synthesised files and `render` on the example script.

// The lint configuration allows these in test functions; this whole file is test code.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use platter_decode::decode_file;
use platter_testkit::files::{flac_pcm16, wav_pcm16};
use platter_testkit::measure::sine_fit;
use platter_testkit::signal::{channel, sine, stereo, to_i16};

const RATE: f64 = 48_000.0;

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_platter-cli"))
        .args(args)
        .output()
        .unwrap()
}

/// A fresh folder for one test, inside the temporary directory of the test run.
fn folder(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn sine_samples(hz: f64, rate: u32, seconds: f64) -> Vec<i16> {
    let frames = (seconds * f64::from(rate)) as usize;
    to_i16(&stereo(&sine(hz, rate, frames, 0.5)))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace('\r', "")
}

#[test]
fn info_prints_the_properties_of_a_file() {
    let folder = folder("cli-info");
    let file = folder.join("tone.flac");
    std::fs::write(
        &file,
        flac_pcm16(44_100, 2, &sine_samples(440.0, 44_100, 1.5)),
    )
    .unwrap();
    let output = cli(&["info", file.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "format:      FLAC\n\
         duration:    0:01.500 (1.500 s)\n\
         channels:    2\n\
         sample rate: 44100 Hz\n"
    );
}

#[test]
fn info_fails_cleanly_on_a_file_that_is_not_audio() {
    let folder = folder("cli-info-bad");
    let file = folder.join("notes.mp3");
    std::fs::write(&file, "not audio at all\n".repeat(500)).unwrap();
    let output = cli(&["info", file.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).starts_with("platter-cli: "));
    assert!(output.stdout.is_empty());
}

#[test]
fn wrong_arguments_print_the_usage() {
    let output = cli(&["render", "--script"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("usage:"));
    assert!(cli(&["help"]).status.success());
}

#[test]
fn the_example_script_mixes_two_synthesised_tracks() {
    let folder = folder("cli-example");
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/two-deck-mix.toml");
    let script = folder.join("two-deck-mix.toml");
    std::fs::copy(example, &script).unwrap();
    // Deck A: a bass tone and a mid tone. Deck B: the same pair at other pitches, in a
    // file with another sample rate.
    let mut deck_a = sine_samples(100.0, 48_000, 9.0);
    for (sample, mid) in deck_a.iter_mut().zip(sine_samples(1_000.0, 48_000, 9.0)) {
        *sample = *sample / 2 + mid / 2;
    }
    let mut deck_b = sine_samples(60.0, 44_100, 9.0);
    for (sample, mid) in deck_b.iter_mut().zip(sine_samples(3_000.0, 44_100, 9.0)) {
        *sample = *sample / 2 + mid / 2;
    }
    std::fs::write(folder.join("deck-a.wav"), wav_pcm16(48_000, 2, &deck_a)).unwrap();
    std::fs::write(folder.join("deck-b.wav"), wav_pcm16(44_100, 2, &deck_b)).unwrap();

    let out = folder.join("mix.wav");
    let log = folder.join("render.log");
    let output = cli(&[
        "render",
        "--script",
        script.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--log",
        log.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).starts_with("rendered 8.000 s (384000 frames)"));
    let log = std::fs::read_to_string(log).unwrap();
    assert!(log.contains("script read: 16 commands"), "{log}");

    let mix = decode_file(&out).unwrap();
    assert_eq!(mix.sample_rate.hz(), 48_000);
    assert_eq!(mix.channels, 2);
    assert_eq!(mix.frames(), 384_000);
    let left = channel(&mix.samples, 2, 0);
    let level = |second: f64, hz: f64| {
        let from = (second * RATE) as usize;
        sine_fit(&left[from..from + 24_000], hz, RATE).amplitude
    };
    // Each tone is 0.25 in its file. Deck B plays 2 % fast, so its tones are 2 % sharp.
    let (a_low, a_mid, b_low, b_mid) = (100.0, 1_000.0, 61.2, 3_060.0);

    // Second 1: deck A alone.
    assert!((level(1.0, a_low) - 0.25).abs() < 0.01);
    assert!((level(1.0, a_mid) - 0.25).abs() < 0.01);
    assert!(level(1.0, b_mid) < 0.002);

    // Second 3: both decks at the centre of the crossfader (3 dB down), B without bass.
    assert!((level(3.0, a_low) - 0.177).abs() < 0.01);
    assert!((level(3.0, a_mid) - 0.177).abs() < 0.01);
    assert!((level(3.0, b_mid) - 0.177).abs() < 0.01);
    assert!(level(3.0, b_low) < 0.01);

    // Second 4.4: the bass has moved from A to B.
    assert!(level(4.4, a_low) < 0.01);
    assert!((level(4.4, b_low) - 0.177).abs() < 0.01);

    // Second 7: deck B alone, 3 dB under its level.
    assert!(level(7.0, a_mid) < 0.002);
    assert!((level(7.0, b_low) - 0.177).abs() < 0.01);
    assert!((level(7.0, b_mid) - 0.177).abs() < 0.01);
}

#[test]
fn render_reports_a_script_it_cannot_run() {
    let folder = folder("cli-bad-script");
    let script = folder.join("mix.toml");
    std::fs::write(
        &script,
        "sample_rate = 48000\nseconds = 1.0\n\n[[command]]\nat = 0.0\nid = \"deck.load\"\n\
         deck = \"a\"\npath = \"absent.wav\"\n",
    )
    .unwrap();
    let out = folder.join("mix.wav");
    let output = cli(&[
        "render",
        "--script",
        script.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        text(&output.stderr).contains("deck A"),
        "{}",
        text(&output.stderr)
    );
    assert!(!out.exists(), "a failed render left an output file");
}
