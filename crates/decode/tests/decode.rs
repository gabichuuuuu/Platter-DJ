//! Decoding of files synthesised in the test, and of damaged ones.
//!
//! MP3, AAC and Ogg Vorbis need an encoder to produce a valid file, and the project has
//! none: those three are covered here only by the damaged-input tests.

// The lint configuration allows these in test functions; this whole file is test code.
#![allow(clippy::unwrap_used)]

use platter_decode::{Container, DecodeError, decode_bytes, decode_file, probe_bytes};
use platter_testkit::files::{aiff_pcm16, flac_pcm16, flac_pcm16_tagged, wav_pcm16};
use platter_testkit::measure::sine_fit;
use platter_testkit::signal::{channel, sine, stereo, to_i16};
use proptest::prelude::{ProptestConfig, any, prop, proptest};

const RATE: u32 = 44_100;
const FRAMES: usize = 22_050;

fn stereo_sine() -> Vec<i16> {
    to_i16(&stereo(&sine(440.0, RATE, FRAMES, 0.5)))
}

fn assert_is_the_sine(bytes: Vec<u8>, extension: &str, container: Container) {
    let audio = decode_bytes(bytes, Some(extension)).unwrap();
    assert_eq!(audio.container, container);
    assert_eq!(audio.sample_rate.hz(), RATE);
    assert_eq!(audio.channels, 2);
    assert_eq!(audio.frames(), FRAMES);
    assert!((audio.seconds() - 0.5).abs() < 1e-9);
    for index in 0..2 {
        let fit = sine_fit(&channel(&audio.samples, 2, index), 440.0, f64::from(RATE));
        assert!((fit.amplitude - 0.5).abs() < 1e-3, "{container:?}");
        // 16-bit quantisation leaves a floor near -98 dB below full scale.
        assert!(fit.snr_db() > 80.0, "{container:?}: {}", fit.snr_db());
    }
}

#[test]
fn decodes_wav() {
    assert_is_the_sine(wav_pcm16(RATE, 2, &stereo_sine()), "wav", Container::Wav);
}

#[test]
fn decodes_aiff() {
    assert_is_the_sine(aiff_pcm16(RATE, 2, &stereo_sine()), "aiff", Container::Aiff);
}

#[test]
fn decodes_flac() {
    assert_is_the_sine(flac_pcm16(RATE, 2, &stereo_sine()), "flac", Container::Flac);
}

#[test]
fn the_content_decides_the_format_not_the_extension() {
    let audio = decode_bytes(wav_pcm16(RATE, 2, &stereo_sine()), Some("mp3")).unwrap();
    assert_eq!(audio.container, Container::Wav);
    let audio = decode_bytes(flac_pcm16(RATE, 2, &stereo_sine()), None).unwrap();
    assert_eq!(audio.container, Container::Flac);
}

#[test]
fn mono_becomes_two_equal_channels() {
    let mono = to_i16(&sine(440.0, RATE, 1_000, 0.5));
    let audio = decode_bytes(wav_pcm16(RATE, 1, &mono), Some("wav")).unwrap();
    assert_eq!(audio.channels, 1);
    let stereo = audio.into_stereo();
    assert_eq!(stereo.len(), 2_000);
    assert!(
        stereo
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| frame[0] == frame[1])
    );
}

#[test]
fn more_than_two_channels_keep_the_first_two() {
    let samples: Vec<i16> = (0..400).map(|n| (n % 4) as i16 * 1_000).collect();
    let audio = decode_bytes(wav_pcm16(RATE, 4, &samples), Some("wav")).unwrap();
    assert_eq!(audio.channels, 4);
    let stereo = audio.into_stereo();
    assert_eq!(stereo.len(), 200);
    assert!(
        stereo
            .as_chunks::<2>()
            .0
            .iter()
            .all(|frame| frame[0] == 0.0 && frame[1] > 0.0)
    );
}

#[test]
fn a_truncated_file_is_an_error() {
    for (name, bytes) in [
        ("wav", wav_pcm16(RATE, 2, &stereo_sine())),
        ("aiff", aiff_pcm16(RATE, 2, &stereo_sine())),
        ("flac", flac_pcm16(RATE, 2, &stereo_sine())),
    ] {
        for keep in [bytes.len() / 2, bytes.len() - 1_001, 60, 20, 3] {
            let cut = bytes[..keep].to_vec();
            let result = decode_bytes(cut, Some(name));
            assert!(result.is_err(), "{name} cut to {keep} bytes decoded");
        }
    }
}

#[test]
fn an_empty_or_foreign_file_is_an_error() {
    assert!(decode_bytes(Vec::new(), Some("wav")).is_err());
    assert!(decode_bytes(b"this is a text file, not audio\n".repeat(200), Some("mp3")).is_err());
    assert!(matches!(
        decode_bytes(vec![0; 100_000], Some("flac")),
        Err(DecodeError::Unsupported(_) | DecodeError::Corrupt(_) | DecodeError::Empty)
    ));
}

#[test]
fn a_header_without_audio_is_an_error() {
    assert!(decode_bytes(wav_pcm16(RATE, 2, &[]), Some("wav")).is_err());
}

#[test]
fn a_header_with_unusable_fields_is_an_error() {
    let good = wav_pcm16(RATE, 2, &stereo_sine());
    // Sample rate of zero, then an absurd one.
    for rate in [0u32, 3, u32::MAX] {
        let mut bad = good.clone();
        bad[24..28].copy_from_slice(&rate.to_le_bytes());
        assert!(decode_bytes(bad, Some("wav")).is_err(), "rate {rate}");
    }
    // No channels.
    let mut bad = good.clone();
    bad[22..24].copy_from_slice(&0u16.to_le_bytes());
    assert!(decode_bytes(bad, Some("wav")).is_err());
}

#[test]
fn a_missing_file_is_an_io_error() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-file.wav");
    assert!(matches!(decode_file(&path), Err(DecodeError::Io(_))));
}

#[test]
fn reads_a_file_from_disk() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("decode-from-disk.aiff");
    std::fs::write(&path, aiff_pcm16(RATE, 2, &stereo_sine())).unwrap();
    let audio = decode_file(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(audio.container, Container::Aiff);
    assert_eq!(audio.frames(), FRAMES);
}

/// Decoded audio is always finite, whatever the input.
#[test]
fn probing_reads_the_header_without_the_audio() {
    let samples = stereo_sine();
    for (bytes, extension, container) in [
        (wav_pcm16(RATE, 2, &samples), "wav", Container::Wav),
        (aiff_pcm16(RATE, 2, &samples), "aiff", Container::Aiff),
        (flac_pcm16(RATE, 2, &samples), "flac", Container::Flac),
    ] {
        let info = probe_bytes(bytes, Some(extension)).unwrap();
        assert_eq!(info.container, container);
        assert_eq!(info.sample_rate.hz(), RATE);
        assert_eq!(info.channels, 2);
        assert!((info.seconds.unwrap() - 0.5).abs() < 1e-6, "{container:?}");
        assert_eq!(info.tags.bpm, None);
    }
    assert!(probe_bytes(b"not audio".to_vec(), Some("wav")).is_err());
}

#[test]
fn tags_carry_title_artist_album_and_tempo() {
    let tags = [
        ("TITLE", "Test tone"),
        ("ARTIST", "Signal generator"),
        ("ALBUM", "Fixtures"),
        ("BPM", "124.50"),
    ];
    let bytes = flac_pcm16_tagged(RATE, 2, &stereo_sine(), &tags);
    for read in [
        probe_bytes(bytes.clone(), Some("flac")).unwrap().tags,
        decode_bytes(bytes, Some("flac")).unwrap().tags,
    ] {
        assert_eq!(read.title.as_deref(), Some("Test tone"));
        assert_eq!(read.artist.as_deref(), Some("Signal generator"));
        assert_eq!(read.album.as_deref(), Some("Fixtures"));
        assert_eq!(read.bpm, Some(124.5));
    }
}

#[test]
fn a_tempo_tag_out_of_range_is_ignored() {
    for value in ["0", "5000", "fast", "-120"] {
        let bytes = flac_pcm16_tagged(RATE, 2, &stereo_sine(), &[("BPM", value)]);
        assert_eq!(
            probe_bytes(bytes, Some("flac")).unwrap().tags.bpm,
            None,
            "{value}"
        );
    }
}

fn assert_sane(result: Result<platter_decode::DecodedAudio, DecodeError>) {
    if let Ok(audio) = result {
        assert!(audio.channels >= 1);
        assert!(audio.samples.iter().all(|sample| sample.is_finite()));
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn random_bytes_never_panic(
        bytes in prop::collection::vec(any::<u8>(), 0..4_096),
        extension in prop::sample::select(vec!["wav", "aiff", "flac", "ogg", "mp3", "m4a"]),
    ) {
        assert_sane(decode_bytes(bytes, Some(extension)));
    }

    #[test]
    fn damaged_files_never_panic(
        format in 0usize..3,
        damage in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..24),
    ) {
        let samples = to_i16(&stereo(&sine(440.0, RATE, 2_000, 0.5)));
        let (mut bytes, extension) = match format {
            0 => (wav_pcm16(RATE, 2, &samples), "wav"),
            1 => (aiff_pcm16(RATE, 2, &samples), "aiff"),
            _ => (flac_pcm16(RATE, 2, &samples), "flac"),
        };
        // Most of the damage lands in the header, where the sizes and counts live.
        for (index, value) in damage {
            let position = index.index(bytes.len().min(96));
            bytes[position] = value;
        }
        assert_sane(decode_bytes(bytes, Some(extension)));
    }
}
