//! Invented data for the capture mode of the application and for the tests: a view of the
//! interface with no engine and no device behind it. The names and numbers are those of the
//! reference screens in `docs/design/screens/`; none of them is a real track or device.

use std::path::PathBuf;
use std::sync::Arc;

use platter_core::{Bpm, Cue, CueKind, CueName, DeckId, HotcueSlot, LoopRegion, SampleRate};
use platter_decode::{Container, Tags};
use platter_engine::{
    AnalysedTempo, DeckSnapshot, EffectControls, EffectKind, Key, Mode, PreviewSnapshot, Snapshot,
    TrackAnalysis, TrackInfo, Transposed, WaveformPeaks,
};
use platter_library::{NodeId, SessionId};

use crate::collection::TagWriting;
use crate::command::{DeckLayout, DeckSize};
use crate::i18n::Language;
use crate::keymap::Keymap;
use crate::keys::KeyNotation;
use crate::model::{
    AnalysisStatus, AudioProblem, AudioState, AudioView, BindingRow, BrowserView, Clock,
    CollectionView, ControlType, ControllerManager, ControllerRow, DeviceView, FileRow,
    MaintenanceOutcome, MidiView, OutputKind, PlaylistRow, RowKind, SessionRow, SettingsView,
    StreamView, TrackRow, TreeRow, View,
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
    pub midi: MidiView,
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
            midi: self.midi,
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
            genre: None,
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

/// A folder of past sets and three playlists, by name, as the library lists them; the last one
/// is the preparation list.
fn playlists() -> Vec<PlaylistRow> {
    let row = |id: i64, name: &str, depth: usize, entries: Option<usize>| PlaylistRow {
        id: NodeId::new(id),
        name: name.to_owned(),
        depth,
        folder: entries.is_none(),
        entries,
        current: false,
        preparation: id == 2,
        favourite: match id {
            4 => Some(4),
            2 => Some(5),
            _ => None,
        },
    };
    vec![
        row(3, "Archive", 0, None),
        row(4, "Summer terrace", 1, Some(41)),
        row(1, "Friday warm-up", 0, Some(24)),
        row(2, "New this week", 0, Some(12)),
    ]
}

/// The session of this run, and two earlier ones where tracks were played.
fn sessions() -> (SessionRow, Vec<SessionRow>) {
    let row = |id: i64, label: &str, tracks: usize| SessionRow {
        id: SessionId::new(id),
        label: label.to_owned(),
        tracks,
        current: false,
    };
    (
        row(9, "2026-11-15 20:14", 6),
        vec![
            row(7, "2026-11-14 21:30", 18),
            row(4, "2026-11-08 22:05", 23),
        ],
    )
}

/// The keys of the tracks as the reference screen shows them: A min, E min, A min, D min,
/// C maj, D♭ maj, E min, and two not analysed yet.
const KEYS: [Option<u8>; TRACKS.len()] = [
    Some(21),
    Some(16),
    Some(21),
    Some(14),
    Some(0),
    Some(1),
    Some(16),
    None,
    None,
];

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
                    played: false,
                    prepared: false,
                    colour: 0,
                    rating: None,
                    genre: None,
                    comment: None,
                    key: KEYS[index],
                }),
            }
        })
        .collect()
}

/// A generic two-deck controller with the assignments of the reference screen, learning the
/// control of its loop size.
fn controllers() -> ControllerManager {
    let binding = |command: Option<&str>,
                   deck: Option<DeckId>,
                   control: ControlType,
                   when: &[(u8, u8)],
                   sets: Option<(u8, u8)>,
                   midi: &str| BindingRow {
        command: command.map(str::to_owned),
        deck,
        control,
        when: when.to_vec(),
        sets,
        midi: Some(midi.to_owned()),
    };
    let a = Some(DeckId::A);
    ControllerManager {
        controllers: vec![
            ControllerRow {
                input: "USB MIDI controller".to_owned(),
                mapping: "generic-two-deck.toml".to_owned(),
                open: true,
                problem: None,
                bindings: vec![
                    binding(
                        Some("deck.play_pause"),
                        a,
                        ControlType::Button,
                        &[],
                        None,
                        "note 1 24",
                    ),
                    binding(
                        Some("deck.cue"),
                        a,
                        ControlType::ButtonHeld,
                        &[],
                        None,
                        "note 1 26",
                    ),
                    binding(
                        Some("deck.tempo.set"),
                        a,
                        ControlType::Fader,
                        &[],
                        None,
                        "cc 1 14",
                    ),
                    binding(
                        Some("deck.loop.size.step"),
                        a,
                        ControlType::Encoder,
                        &[],
                        None,
                        "cc 1 20",
                    ),
                    binding(
                        Some("deck.beatjump"),
                        a,
                        ControlType::Encoder,
                        &[(1, 1)],
                        None,
                        "cc 1 20",
                    ),
                    binding(
                        Some("mixer.fader.set"),
                        a,
                        ControlType::Fader,
                        &[],
                        None,
                        "cc 1 7",
                    ),
                    binding(
                        None,
                        None,
                        ControlType::ButtonHeld,
                        &[],
                        Some((1, 1)),
                        "note 1 21",
                    ),
                ],
            },
            // A second one, so the list has a choice besides the one shown.
            ControllerRow {
                input: "Second MIDI controller".to_owned(),
                mapping: "generic-two-deck.toml".to_owned(),
                open: false,
                problem: None,
                bindings: Vec::new(),
            },
        ],
        inputs: vec![
            "USB MIDI controller".to_owned(),
            "Second MIDI controller".to_owned(),
        ],
        selected: Some(0),
        binding: Some(3),
        learning: false,
        target: DeckId::A,
        importable: vec![PathBuf::from(format!("{FOLDER}/club-controller.toml"))],
    }
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
        fullscreen_at_start: false,
        tempo_range: 8,
        deck_size: DeckSize::Full,
        deck_layout: DeckLayout::Two,
        settings_file: "settings.toml".to_owned(),
        keymap_file: "keymap.toml".to_owned(),
        library_file: "library.sqlite".to_owned(),
        min_playtime: 30,
        key_notation: KeyNotation::default(),
        bpm_range: (70, 180),
        load_stopped_only: false,
        music_folders: vec![PathBuf::from(FOLDER), PathBuf::from("Music/Promos")],
        music_folder: Some(0),
        import_at_start: true,
        check_at_start: false,
        tag_writing: TagWriting::default(),
        controllers: controllers(),
    }
}

fn browser() -> BrowserView {
    BrowserView {
        collection: Some(CollectionView {
            available: Some(true),
            tracks: Some(1284),
            playlists: playlists(),
            session: Some(sessions().0),
            sessions: sessions().1,
            // Two tracks whose files a check found gone.
            missing: vec![
                "External drive/Crates/Hard Groove/Iron Lung.flac".to_owned(),
                "External drive/Crates/Hard Groove/Late Train.mp3".to_owned(),
            ],
            missing_total: Some(2),
            maintained: Some(MaintenanceOutcome::Checked {
                missing: 2,
                found: 0,
            }),
            ..CollectionView::default()
        }),
        tree: tree(),
        search: String::new(),
        sort: None,
        columns: crate::command::ListColumn::SHOWN_AT_FIRST.to_vec(),
        folder: Some(PathBuf::from("Home/Music/Sets")),
        explorer: Some(PathBuf::from("Home/Music/Sets")),
        pictures: vec![
            PathBuf::from("Home/Music/Sets/Cover.jpg"),
            PathBuf::from("Home/Music/Sets/Night sky.png"),
        ],
        closed_folders: Vec::new(),
        history_open: true,
        // The sample files are invented: the cells show the icon of a file without a cover.
        covers: std::collections::HashMap::new(),
        crumbs: vec!["Home".to_owned(), "Music".to_owned(), "Sets".to_owned()],
        rows: rows(),
        selected: Some(2),
        marked: vec![2],
        reading: false,
        // As the design system shows the status bar.
        analysis: Some(AnalysisStatus { done: 3, total: 12 }),
        problem: None,
        deck_covers: [None, None, None, None],
    }
}

/// Two tracks loaded and playing on a four-output device: the state of `M2-main.html`.
pub fn playing() -> Scene {
    let mut engine = Snapshot {
        decks: [
            loaded_deck(0, 299.95, 124.0 / 125.0, [0.42, 0.38]),
            loaded_deck(1, 32.15, 124.0 / 126.0, [0.47, 0.5]),
            DeckSnapshot::default(),
            DeckSnapshot::default(),
        ],
        master_levels: [0.55, 0.5],
        limiter_gain: 1.0,
        ..Snapshot::default()
    };
    engine.mixer.crossfader = 0.35;
    m4_effects(&mut engine.effects);
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
    // The preview player holds the selected track, paused at 01:48, as the reference screen.
    let previewed = track(2);
    engine.preview = PreviewSnapshot {
        peaks: Some(Arc::new(peaks(previewed.seconds, 123.0, 3))),
        track: Some(previewed),
        loading: false,
        playing: false,
        position_seconds: 108.0,
    };
    // The analysis of deck A has finished: its resets have something to bring back.
    engine.decks[0].analysis = Some(TrackAnalysis {
        tempo: Bpm::new(125.0).ok().map(|bpm| AnalysedTempo {
            bpm,
            first_beat: 0.1,
        }),
        key: Some(Key {
            tonic: 9,
            mode: Mode::Minor,
        }),
        loudness: Some(-9.0),
    });
    engine.decks[0].resulting_key = Some(Transposed {
        key: Key {
            tonic: 9,
            mode: Mode::Minor,
        },
        cents: 0.0,
    });
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
        midi: MidiView {
            named: 1,
            open: 1,
            active: false,
        },
    }
}

/// The effects of the M4 screen: delay on in unit 1, the gater on in unit 2, decks C and A
/// through unit 1, deck B through unit 2, a mixer effect on each channel.
fn m4_effects(effects: &mut EffectControls) {
    let one = &mut effects.units[0];
    one.slots = [
        Some(EffectKind::Delay),
        Some(EffectKind::Reverb),
        Some(EffectKind::Filter),
    ];
    one.values.dry_wet = 0.6;
    one.values.slots[0].on = true;
    one.values.slots[0].amount = 0.4;
    one.values.slots[1].amount = 0.3;
    let two = &mut effects.units[1];
    two.slots = [
        Some(EffectKind::Flanger),
        Some(EffectKind::Gater),
        Some(EffectKind::Slicer),
    ];
    two.values.dry_wet = 0.45;
    two.values.slots[1].on = true;
    two.values.slots[1].amount = 0.7;
    effects.assigned[DeckId::C.index()][0] = true;
    effects.assigned[DeckId::A.index()][0] = true;
    effects.assigned[DeckId::B.index()][1] = true;
    for (mixer, effect) in effects.mixer.iter_mut().zip([
        EffectKind::Reverb,
        EffectKind::Delay,
        EffectKind::Bitcrusher,
        EffectKind::Gater,
    ]) {
        mixer.effect = effect;
    }
    effects.mixer[0].amount = 0.3;
}

/// The playing state in the four-deck layout: deck C plays under deck A, deck D is loaded and
/// stopped under deck B.
pub fn four_decks() -> Scene {
    let mut scene = playing();
    scene.settings.deck_layout = DeckLayout::Four;
    // The Limiter key lit, as the M4 screen draws it.
    scene.engine.mixer.maximiser.on = true;
    let decks = &mut scene.engine.decks;
    decks[2] = loaded_deck(2, 75.4, 1.0, [0.35, 0.33]);
    decks[2].controls.fader = 0.8;
    decks[3] = loaded_deck(3, 0.0, 1.0, [0.0, 0.0]);
    decks[3].playing = false;
    decks[3].controls.fader = 0.0;
    // Decks A, B and C play at the gain autogain gives them; deck D waits for its analysis.
    for deck in decks.iter_mut().take(3) {
        deck.autogain_db = Some(deck.controls.gain_db);
    }
    scene
}

/// The main state with the playlist *Friday warm-up* shown in the list.
pub fn playlist() -> Scene {
    let mut scene = playing();
    let friday = NodeId::new(1);
    let browser = &mut scene.browser;
    if let Some(collection) = &mut browser.collection {
        collection.playlist = Some(friday);
        collection.matched = Some(24);
        for row in &mut collection.playlists {
            row.current = row.id == friday;
        }
    }
    browser.folder = None;
    browser.crumbs.clear();
    // Two tracks of the playlist were played in this session, and the two after them are in
    // the preparation list. The DJ has rated them all and tagged some with a colour.
    for (index, row) in browser.rows.iter_mut().enumerate() {
        if let RowKind::Track(track) = &mut row.kind {
            track.played = (3..5).contains(&index);
            track.prepared = (5..7).contains(&index);
            track.rating = Some([4, 5, 3, 2, 4, 5, 1, 3, 0][index % 9]);
            track.colour = [1, 0, 3, 0, 5, 0, 2, 8, 0][index % 9];
        }
    }
    for row in &mut browser.tree {
        row.current = false;
    }
    scene
}

/// The list shows the history of an earlier session; every track in it was played then.
pub fn history() -> Scene {
    let mut scene = playing();
    let browser = &mut scene.browser;
    if let Some(collection) = &mut browser.collection {
        let earlier = SessionId::new(7);
        collection.history = Some(earlier);
        collection.matched = Some(18);
        for row in &mut collection.sessions {
            row.current = row.id == earlier;
        }
    }
    browser.folder = None;
    browser.crumbs.clear();
    for row in &mut browser.tree {
        row.current = false;
    }
    scene
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
            decks: [
                DeckSnapshot::default(),
                reading,
                DeckSnapshot::default(),
                DeckSnapshot::default(),
            ],
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
        // Nothing is selected yet when Platter starts.
        browser: BrowserView {
            selected: None,
            marked: Vec::new(),
            ..browser()
        },
        keymap: Keymap::defaults(),
        clock: Some(Clock {
            hour: 23,
            minute: 41,
        }),
        // A controller in the settings that is not connected.
        midi: MidiView {
            named: 1,
            open: 0,
            active: false,
        },
    }
}
