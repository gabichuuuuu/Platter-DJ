//! The text of a track: `docs/behavior/library.md`, *The text of a track*.

// The lint configuration allows these in test functions; this whole file is test code.
#![allow(clippy::unwrap_used)]

use platter_core::{
    BeatGrid, Bpm, Cue, CueKind, CueName, Cues, HotcueSlot, LoopRegion, MAX_MARKERS,
    MAX_TRACK_TEXT, TrackMemory, TrackTextError,
};

fn slot(number: u8) -> HotcueSlot {
    HotcueSlot::ALL[usize::from(number) - 1]
}

fn sample() -> TrackMemory {
    let mut grid = BeatGrid::default();
    grid.add_marker(0.512);
    grid.add_marker(64.25);
    grid.set_bpm(Bpm::new(125.5).unwrap());
    grid.set_locked(true);
    let mut slots = [None; HotcueSlot::COUNT];
    slots[0] = Some(Cue::of_kind(12.25, CueKind::Cue, 0.0, CueName::new("Intro")).unwrap());
    slots[4] = Some(Cue::looping(LoopRegion {
        start: 30.0,
        end: 34.8,
    }));
    slots[4].as_mut().unwrap().name = CueName::new("Drop  and ñ");
    slots[7] = Some(Cue::of_kind(1.0 / 3.0, CueKind::FadeOut, 0.0, CueName::default()).unwrap());
    TrackMemory {
        grid,
        cues: Cues::from_slots(slots),
    }
}

#[test]
fn a_memory_reads_back_as_it_was_written() {
    let memory = sample();
    let text = memory.to_text();
    assert!(text.starts_with("platter-track 1\n"), "{text}");
    assert!(text.contains("\nbpm 125.5\n"), "{text}");
    assert!(text.contains("\nlock\n"), "{text}");
    assert!(text.contains("\ncue 1 cue 12.25 0 Intro\n"), "{text}");
    assert_eq!(TrackMemory::parse(&text).unwrap(), memory);
    // An empty memory is the header alone.
    assert_eq!(TrackMemory::default().to_text(), "platter-track 1\n");
    assert!(TrackMemory::default().is_empty());
    assert!(!memory.is_empty());
}

#[test]
fn the_text_of_the_note_reads_as_it_says() {
    let text = "platter-track 1\nbpm 125.5\nlock\nmarker 0.512\ncue 1 cue 12.25 0 Intro\n\
                cue 5 loop 30 4.8 Drop\n";
    let memory = TrackMemory::parse(text).unwrap();
    assert_eq!(memory.grid.bpm(), Some(Bpm::new(125.5).unwrap()));
    assert!(memory.grid.is_locked());
    assert_eq!(memory.grid.markers(), [0.512]);
    let intro = memory.cues.get(slot(1)).unwrap();
    assert_eq!(intro.kind(), CueKind::Cue);
    assert_eq!(intro.position, 12.25);
    assert_eq!(intro.name.as_str(), "Intro");
    let drop = memory.cues.get(slot(5)).unwrap();
    assert_eq!(drop.kind(), CueKind::Loop);
    assert_eq!((drop.position, drop.length()), (30.0, 4.8));
    assert_eq!(drop.name.as_str(), "Drop");
    assert_eq!(memory.cues.selected(), None);
    // Windows line ends and blank lines read the same.
    assert_eq!(
        TrackMemory::parse(&text.replace('\n', "\r\n\r\n")).unwrap(),
        memory
    );
}

#[test]
fn a_cue_without_a_name_and_a_name_with_spaces_keep_their_text() {
    let memory =
        TrackMemory::parse("platter-track 1\ncue 2 load 3 0\ncue 3 cue 4 0  two  words\n").unwrap();
    assert_eq!(memory.cues.get(slot(2)).unwrap().name.as_str(), "");
    assert_eq!(memory.cues.get(slot(2)).unwrap().kind(), CueKind::Load);
    assert_eq!(
        memory.cues.get(slot(3)).unwrap().name.as_str(),
        " two  words"
    );
}

#[test]
fn a_text_that_is_not_one_platter_reads_is_refused_whole() {
    let refused = [
        ("", TrackTextError::Header),
        ("platter-track 2\nbpm 120\n", TrackTextError::Header),
        ("PLATTER-TRACK 1\n", TrackTextError::Header),
        ("platter-track 1\ntempo 120\n", TrackTextError::Line(2)),
        (
            "platter-track 1\nbpm 120\nbpm 121\n",
            TrackTextError::Line(3),
        ),
        ("platter-track 1\nbpm 12\n", TrackTextError::Line(2)),
        ("platter-track 1\nbpm NaN\n", TrackTextError::Line(2)),
        ("platter-track 1\nbpm inf\n", TrackTextError::Line(2)),
        ("platter-track 1\nbpm 120,5\n", TrackTextError::Line(2)),
        ("platter-track 1\nlock\nlock\n", TrackTextError::Line(3)),
        ("platter-track 1\nlock now\n", TrackTextError::Line(2)),
        ("platter-track 1\nmarker -1\n", TrackTextError::Line(2)),
        ("platter-track 1\nmarker 90000\n", TrackTextError::Line(2)),
        (
            "platter-track 1\nmarker 1\nmarker 1.0005\n",
            TrackTextError::Line(3),
        ),
        (
            "platter-track 1\ncue 9 cue 1 0 x\n",
            TrackTextError::Line(2),
        ),
        (
            "platter-track 1\ncue 0 cue 1 0 x\n",
            TrackTextError::Line(2),
        ),
        (
            "platter-track 1\ncue 1 grid 1 0 x\n",
            TrackTextError::Line(2),
        ),
        (
            "platter-track 1\ncue 1 loop 1 0 x\n",
            TrackTextError::Line(2),
        ),
        (
            "platter-track 1\ncue 1 cue -1 0 x\n",
            TrackTextError::Line(2),
        ),
        ("platter-track 1\ncue 1 cue 1\n", TrackTextError::Line(2)),
        (
            "platter-track 1\ncue 1 cue 1 0 a\ncue 1 cue 2 0 b\n",
            TrackTextError::Line(3),
        ),
    ];
    for (text, error) in refused {
        assert_eq!(TrackMemory::parse(text), Err(error), "{text:?}");
    }
}

#[test]
fn the_limits_of_the_text_and_of_the_grid_hold() {
    let mut text = String::from("platter-track 1\n");
    while text.len() <= MAX_TRACK_TEXT {
        text.push('\n');
    }
    assert_eq!(TrackMemory::parse(&text), Err(TrackTextError::TooLong));

    let mut full = String::from("platter-track 1\n");
    for index in 0..MAX_MARKERS {
        full.push_str(&format!("marker {}\n", index as f64 * 0.5));
    }
    assert_eq!(
        TrackMemory::parse(&full).unwrap().grid.markers().len(),
        MAX_MARKERS
    );
    full.push_str("marker 9999\n");
    assert_eq!(
        TrackMemory::parse(&full),
        Err(TrackTextError::Line(MAX_MARKERS + 2))
    );
}

#[test]
fn a_tempo_without_markers_reads_with_one_at_the_start() {
    // As a tempo set on a grid without markers does.
    let memory = TrackMemory::parse("platter-track 1\nbpm 128\n").unwrap();
    assert_eq!(memory.grid.markers(), [0.0]);
    assert_eq!(TrackMemory::parse(&memory.to_text()).unwrap(), memory);
}

#[test]
fn numbers_read_back_to_the_same_value() {
    for value in [0.0, 1.0 / 3.0, 0.1 + 0.2, 86_399.999_999_9, 1e-9] {
        let mut slots = [None; HotcueSlot::COUNT];
        slots[0] = Some(Cue::at(value));
        let memory = TrackMemory {
            grid: BeatGrid::default(),
            cues: Cues::from_slots(slots),
        };
        let back = TrackMemory::parse(&memory.to_text()).unwrap();
        assert_eq!(back.cues.get(slot(1)).unwrap().position, value);
    }
}
