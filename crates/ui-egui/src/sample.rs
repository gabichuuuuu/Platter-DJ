//! Invented data for the capture mode of the application and for the tests: a view of the
//! interface with no engine and no device behind it. The names and numbers are those of the
//! reference screens in `docs/design/screens/`; none of them is a real track or device.

use std::path::PathBuf;
use std::sync::Arc;

use platter_core::{Bpm, Cue, CueKind, CueName, DeckId, HotcueSlot, LoopRegion, SampleRate};
use platter_decode::{Container, Tags};
use platter_engine::{DeckSnapshot, Snapshot, TrackInfo, WaveformPeaks};

use crate::i18n::Language;
use crate::keymap::Keymap;
use crate::model::{
    AudioProblem, AudioState, AudioView, BrowserView, Clock, CollectionView, DeviceView, FileRow,
    OutputKind, RowKind, SettingsView, StreamView, TrackRow, TreeRow, View,
};

/// Everything a [`View`] borrows, owned in one place.
#[derive(Clone, Debug)]
pub struct Scene {
    pub engine: Snapshot,
    pub audio: AudioView,
    pub settings: SettingsView,
    pub browser: BrowserView,
    pub keymap: Keymap,
    pub clock: Option<Clock>,
}

impl Scene {
    pub fn view(&self) -> View<'_> {
        View {
            engine: &self.engine,
            audio: &self.audio,
            settings: &self.settings,
            browser: &self.browser,
            keymap: &self.keymap,
            clock: self.clock,
            notices: [None; DeckId::COUNT],
        }
    }
}

const FOLDER: &str = "Music/Sets";

/// The tracks of the sample folder: title, artist, album, tempo, seconds, file extension.
const TRACKS: [(&str, &str, &str, f64, f64, &str); 9] = [
    (
        "Night Drive (Extended Mix)",
        "Ilse Maren",
        "Salt Flats EP",
        125.0,
        372.4,
        "flac",
    ),
    (
        "Low Tide",
        "Okonkwo & Reyes",
        "Harbour Lights",
        126.0,
        380.2,
        "mp3",
    ),
    (
        "Paper Lanterns",
        "Mirela Costin",
        "Paper Lanterns",
        123.0,
        344.0,
        "wav",
    ),
    (
        "Second Shift",
        "Tomas Hadley",
        "Works, Vol. 2",
        125.0,
        422.0,
        "flac",
    ),
    (
        "Glasshouse",
        "Anneke Voss",
        "Glasshouse",
        124.0,
        398.0,
        "mp3",
    ),
    (
        "Undertow (Dub)",
        "Okonkwo & Reyes",
        "Harbour Lights",
        122.0,
        490.0,
        "wav",
    ),
    (
        "Kite Season",
        "Ravi Anand",
        "Field Notes",
        126.0,
        327.0,
        "flac",
    ),
    (
        "Signal Fires",
        "Mirela Costin",
        "Paper Lanterns",
        128.0,
        361.0,
        "mp3",
    ),
    (
        "Halfway House",
        "Dara Quinlan",
        "Halfway House",
        121.0,
        453.0,
        "wav",
    ),
];

fn format_of(extension: &str) -> (Container, SampleRate) {
    match extension {
        "flac" => (Container::Flac, SampleRate::HZ_44100),
        "mp3" => (Container::Mp3, SampleRate::HZ_44100),
        _ => (Container::Wav, SampleRate::HZ_48000),
    }
}

fn path_of(index: usize) -> PathBuf {
    let (title, _, _, _, _, extension) = TRACKS[index];
    PathBuf::from(format!("{FOLDER}/{title}.{extension}"))
}

fn track(index: usize) -> TrackInfo {
    let (title, artist, album, bpm, seconds, extension) = TRACKS[index];
    let (container, sample_rate) = format_of(extension);
    TrackInfo {
        path: path_of(index),
        container,
        sample_rate,
        channels: 2,
        seconds,
        tags: Tags {
            title: Some(title.to_owned()),
            artist: Some(artist.to_owned()),
            album: Some(album.to_owned()),
            bpm: Some(bpm),
        },
    }
}

/// A waveform that looks like dance music without being any: a kick on every beat, hats
/// between the beats, and a body that rises and falls in sections of 32 beats.
fn peaks(seconds: f64, bpm: f64, seed: u32) -> WaveformPeaks {
    const PER_SECOND: f64 = 400.0;
    let columns = (seconds * PER_SECOND) as usize;
    let beat = 60.0 / bpm * PER_SECOND;
    let mut noise = seed.wrapping_mul(2_654_435_761).max(1);
    let mut next = || {
        // A small generator with a fixed seed: the picture is the same on every run.
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        (noise >> 8) as f32 / (1u32 << 24) as f32
    };
    let data = (0..columns)
        .map(|index| {
            let position = index as f64 / beat;
            let in_beat = (position.fract()) as f32;
            let section = (position / 32.0) as usize;
            // One section in eight is a breakdown: no kick, a quieter body.
            let breakdown = section % 8 == 5;
            let kick = if breakdown {
                0.0
            } else {
                (1.0 - in_beat * 5.0).max(0.0)
            };
            let off_beat = ((position * 2.0).fract()) as f32;
            let hat = (1.0 - off_beat * 9.0).max(0.0) * if breakdown { 0.3 } else { 0.7 };
            let body = if breakdown { 0.35 } else { 0.55 } + 0.15 * (position as f32 / 5.0).sin();
            let low = (0.25 + 0.7 * kick) * (0.85 + 0.15 * next());
            let mid = body * (0.6 + 0.4 * next());
            let high = (0.12 + 0.5 * hat) * (0.5 + 0.5 * next());
            [low, mid, high].map(|level| (level.clamp(0.0, 1.0) * 255.0) as u8)
        })
        .collect();
    WaveformPeaks::from_columns(data, 1.0 / PER_SECOND)
}

fn loaded_deck(index: usize, position: f64, tempo_ratio: f64, levels: [f32; 2]) -> DeckSnapshot {
    let info = track(index);
    let bpm = info.tags.bpm.unwrap_or(120.0);
    let mut deck = DeckSnapshot {
        peaks: Some(Arc::new(peaks(info.seconds, bpm, index as u32 + 1))),
        track: Some(info),
        playing: true,
        position_seconds: position,
        levels,
        ..DeckSnapshot::default()
    };
    deck.controls.tempo_ratio = tempo_ratio;
    deck.speed = tempo_ratio;
    // A grid on the beats the invented peaks put at whole beats from the start; the second
    // track has its grid locked, so both states of the GRID page are drawn.
    if let Ok(bpm) = Bpm::new(bpm) {
        deck.grid.set_bpm(bpm);
    }
    deck.grid.set_locked(index == 1);
    deck
}

fn tree() -> Vec<TreeRow> {
    let row = |path: &str, depth: usize, open: bool, current: bool, files: Option<usize>| TreeRow {
        path: PathBuf::from(path),
        name: path.rsplit('/').next().unwrap_or(path).to_owned(),
        depth,
        open,
        current,
        files,
    };
    vec![
        row("Home", 0, true, false, None),
        row("Home/Music", 1, true, false, None),
        row("Home/Music/Sets", 2, false, true, Some(13)),
        row("Home/Music/New arrivals", 2, false, false, Some(27)),
        row("Home/Music/Vinyl rips", 2, false, false, Some(64)),
        row("Home/Downloads", 1, false, false, None),
        row("Home/Desktop", 1, false, false, None),
        row("External drive", 0, false, false, None),
    ]
}

fn rows() -> Vec<FileRow> {
    (0..TRACKS.len())
        .map(|index| {
            let (title, artist, album, bpm, seconds, extension) = TRACKS[index];
            let (container, sample_rate) = format_of(extension);
            FileRow {
                path: path_of(index),
                name: format!("{title}.{extension}"),
                kind: RowKind::Track(TrackRow {
                    title: Some(title.to_owned()),
                    artist: Some(artist.to_owned()),
                    album: Some(album.to_owned()),
                    bpm: Some(bpm),
                    seconds: Some(seconds),
                    format: container.name().to_owned(),
                    sample_rate: sample_rate.hz(),
                    missing: false,
                }),
            }
        })
        .collect()
}

fn audio(state: AudioState) -> AudioView {
    AudioView {
        state,
        devices: vec![DeviceView {
            name: "USB audio interface".to_owned(),
            outputs: 4,
            inputs: 2,
            sample_rates: vec![44_100, 48_000, 96_000],
        }],
        sample_rates: vec![44_100, 48_000, 96_000],
        buffer_sizes: vec![64, 128, 256, 512, 1_024],
    }
}

fn settings() -> SettingsView {
    SettingsView {
        device: Some("USB audio interface".to_owned()),
        sample_rate: 48_000,
        buffer_frames: 256,
        routing: [0, 1, 2, 3],
        language: Language::English,
        fullscreen: false,
        tooltips: true,
        settings_file: "settings.toml".to_owned(),
        keymap_file: "keymap.toml".to_owned(),
    }
}

fn browser() -> BrowserView {
    BrowserView {
        collection: Some(CollectionView {
            available: Some(true),
            tracks: Some(1284),
            ..CollectionView::default()
        }),
        tree: tree(),
        folder: Some(PathBuf::from("Home/Music/Sets")),
        crumbs: vec!["Home".to_owned(), "Music".to_owned(), "Sets".to_owned()],
        rows: rows(),
        selected: Some(2),
        reading: false,
    }
}

/// Two tracks loaded and playing on a four-output device: the state of `M2-main.html`.
pub fn playing() -> Scene {
    let mut engine = Snapshot {
        decks: [
            loaded_deck(0, 299.95, 124.0 / 125.0, [0.42, 0.38]),
            loaded_deck(1, 32.15, 124.0 / 126.0, [0.47, 0.5]),
        ],
        master_levels: [0.55, 0.5],
        limiter_gain: 1.0,
        ..Snapshot::default()
    };
    engine.mixer.crossfader = 0.35;
    engine.mixer.master_gain_db = -2.0;
    engine.decks[1].controls.fader = 0.45;
    engine.decks[1].controls.pre_listen = true;
    engine.decks[1].controls.eq_kill[0] = true;
    // Deck A plays inside a loop of four beats; deck B has an in point that waits.
    engine.decks[0].looping.region = Some(LoopRegion {
        start: 298.08,
        end: 300.0,
    });
    engine.decks[1].looping.in_point = Some(31.0);
    // Deck A has three stored cues: a named one, a fade out far back and the loop it plays,
    // which is the selected one. Deck B has one.
    let slot = |number: usize| HotcueSlot::ALL[number - 1];
    let cues = &mut engine.decks[0].cues;
    cues.store(slot(1), Cue::at(297.2));
    cues.rename_selected(CueName::new("Break"));
    cues.store(slot(5), Cue::at(120.0));
    cues.retype_selected(CueKind::FadeOut, None);
    cues.store(
        slot(2),
        Cue::looping(LoopRegion {
            start: 298.08,
            end: 300.0,
        }),
    );
    engine.decks[1].cues.store(slot(3), Cue::at(31.0));
    // Deck A is the tempo master and deck B follows it, a little behind its beat.
    engine.clock.master = Some(DeckId::A);
    engine.clock.bpm = 124.0;
    engine.clock.snap = true;
    engine.decks[0].phase = Some(0.0);
    engine.decks[1].sync = true;
    engine.decks[1].phase = Some(-0.12);
    let stream = StreamView {
        kind: OutputKind::Usb,
        sample_rate: 48_000,
        channels: 4,
        buffer_frames: 256,
        latency_ms: 5.3,
        load: 0.18,
        dropouts: 0,
        headphones: true,
    };
    Scene {
        engine,
        audio: audio(AudioState::Running(stream)),
        settings: settings(),
        browser: browser(),
        keymap: Keymap::defaults(),
        clock: Some(Clock {
            hour: 23,
            minute: 41,
        }),
    }
}

/// No audio device, deck A empty and deck B reading a file: the state of
/// `Startup-states.html`.
pub fn startup() -> Scene {
    let info = track(1);
    let reading = DeckSnapshot {
        track: Some(info),
        loading: true,
        ..DeckSnapshot::default()
    };
    Scene {
        engine: Snapshot {
            decks: [DeckSnapshot::default(), reading],
            ..Snapshot::default()
        },
        audio: AudioView {
            devices: Vec::new(),
            ..audio(AudioState::Stopped(AudioProblem::NoDevice))
        },
        settings: SettingsView {
            device: None,
            ..settings()
        },
        browser: browser(),
        keymap: Keymap::defaults(),
        clock: Some(Clock {
            hour: 23,
            minute: 41,
        }),
    }
}
