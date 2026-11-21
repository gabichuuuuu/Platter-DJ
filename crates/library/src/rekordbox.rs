//! Reading the collection file another DJ program shares: the XML format for sharing
//! playlists its vendor publishes, with the tracks, their beatgrids and cues, and the tree of
//! playlists. Behaviour note: `docs/behavior/library.md`, section *Collection files*.
//!
//! Like the M3U reader it works on bytes and never touches the file system: the caller reads
//! the file and checks that each track exists.

use std::collections::HashMap;
use std::path::PathBuf;

use platter_core::{BeatGrid, Bpm, Cue, CueKind, CueName, Cues, HotcueSlot, TrackMemory};

use crate::m3u::file_url_path;
use crate::xml::{self, Event};

/// The largest file read: a collection of a hundred thousand tracks with their grids and cues
/// stays well under it.
pub const MAX_COLLECTION_BYTES: usize = 256 * 1024 * 1024;
/// The most tracks and playlist entries read.
pub const MAX_COLLECTION_TRACKS: usize = 200_000;
/// The most playlists and folders read.
pub const MAX_COLLECTION_NODES: usize = 10_000;
/// The rating the format writes for five stars; each star is a fifth of it.
const FULL_RATING: u32 = 255;

/// A track of the collection.
#[derive(Clone, Debug, PartialEq)]
pub struct SharedTrack {
    pub path: PathBuf,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    /// Stars, 0 to 5.
    pub rating: u8,
    /// The beatgrid and the cues; empty when the file gives none.
    pub memory: TrackMemory,
}

/// A folder or a playlist of the tree, with the tracks of a playlist as indices into
/// [`SharedCollection::tracks`], in their order.
#[derive(Clone, Debug, PartialEq)]
pub struct SharedNode {
    pub name: String,
    pub playlist: Option<Vec<usize>>,
    pub children: Vec<SharedNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SharedCollection {
    /// The program that wrote the file, as it names itself.
    pub product: Option<String>,
    pub tracks: Vec<SharedTrack>,
    /// What the root folder holds.
    pub playlists: Vec<SharedNode>,
    /// Tracks without a file location, entries naming no track of the collection, and what
    /// went past the limits.
    pub skipped: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionError {
    TooLarge,
    /// Not XML, or XML of another kind.
    NotACollection,
}

/// Reads a collection file.
pub fn read_collection(bytes: &[u8]) -> Result<SharedCollection, CollectionError> {
    if bytes.len() > MAX_COLLECTION_BYTES {
        return Err(CollectionError::TooLarge);
    }
    let events = xml::read(bytes).map_err(|_| CollectionError::NotACollection)?;
    let is_root =
        |event: &Event| matches!(event, Event::Start { name, .. } if name == "DJ_PLAYLISTS");
    if !events.first().is_some_and(is_root) {
        return Err(CollectionError::NotACollection);
    }
    let mut collection = SharedCollection {
        product: None,
        tracks: Vec::new(),
        playlists: Vec::new(),
        skipped: 0,
    };
    // The track of each identifier and of each location, to find the entries of playlists.
    let mut keys = Keys::default();
    let mut index = 0;
    let mut in_collection = false;
    while index < events.len() {
        let event = &events[index];
        index += 1;
        match event {
            Event::Start { name, .. } if name == "PRODUCT" => {
                collection.product = event
                    .attribute("Name")
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned);
            }
            Event::Start { name, .. } if name == "COLLECTION" => in_collection = true,
            Event::End { name } if name == "COLLECTION" => in_collection = false,
            Event::Start { name, empty, .. } if name == "TRACK" && in_collection => {
                let (track, next) = track(&events, index, event, *empty);
                index = next;
                let location = event.attribute("Location").unwrap_or_default();
                match track {
                    Some(track) if collection.tracks.len() < MAX_COLLECTION_TRACKS => {
                        let track_index = collection.tracks.len();
                        if let Some(id) = event.attribute("TrackID") {
                            keys.ids.entry(id.trim().to_owned()).or_insert(track_index);
                        }
                        keys.locations
                            .entry(location.to_owned())
                            .or_insert(track_index);
                        collection.tracks.push(track);
                    }
                    _ => collection.skipped += 1,
                }
            }
            Event::Start {
                name, empty: false, ..
            } if name == "PLAYLISTS" => {
                let mut nodes = 0;
                let mut entries = 0;
                let (root, next) = node_children(
                    &events,
                    index,
                    &keys,
                    &mut nodes,
                    &mut entries,
                    &mut collection.skipped,
                );
                index = next;
                // The tree has one root folder; its children are the playlists.
                collection.playlists = match <[SharedNode; 1]>::try_from(root) {
                    Ok([root]) if root.playlist.is_none() => root.children,
                    Err(nodes) => nodes,
                    Ok([other]) => vec![other],
                };
            }
            _ => {}
        }
    }
    Ok(collection)
}

/// The track that `start` opens, read up to its end; returns it and the index after it.
fn track(
    events: &[Event],
    mut index: usize,
    start: &Event,
    empty: bool,
) -> (Option<SharedTrack>, usize) {
    let path = start
        .attribute("Location")
        .and_then(|location| {
            let rest = location
                .strip_prefix("file://")
                .or_else(|| location.strip_prefix("FILE://"))?;
            file_url_path(rest)
        })
        .filter(|path| !path.is_empty() && !path.contains('\0'))
        .map(PathBuf::from);
    let text = |name: &str| {
        start
            .attribute(name)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let rating = start
        .attribute("Rating")
        .and_then(|rating| rating.trim().parse::<u32>().ok())
        .map_or(0, |rating| {
            // The nearest number of stars.
            ((rating.min(FULL_RATING) * 5 + FULL_RATING / 2) / FULL_RATING) as u8
        });
    let mut tempos: Vec<(f64, f64, u32)> = Vec::new();
    let mut marks: Vec<(f64, f64, i64, i64, String)> = Vec::new();
    if !empty {
        while let Some(event) = events.get(index) {
            index += 1;
            match event {
                Event::End { name } if name == "TRACK" => break,
                Event::Start { name, .. } if name == "TEMPO" => {
                    let start = number(event, "Inizio");
                    let bpm = number(event, "Bpm");
                    let beat = event
                        .attribute("Battito")
                        .and_then(|beat| beat.trim().parse().ok())
                        .unwrap_or(1);
                    if let (Some(start), Some(bpm)) = (start, bpm) {
                        tempos.push((start, bpm, beat));
                    }
                }
                Event::Start { name, .. } if name == "POSITION_MARK" => {
                    let integer = |key| {
                        event
                            .attribute(key)
                            .and_then(|value| value.trim().parse::<i64>().ok())
                    };
                    if let Some(start) = number(event, "Start") {
                        let end = number(event, "End").unwrap_or(start);
                        marks.push((
                            start,
                            end,
                            integer("Type").unwrap_or(0),
                            integer("Num").unwrap_or(-1),
                            event.attribute("Name").unwrap_or_default().to_owned(),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    let track = path.map(|path| SharedTrack {
        path,
        title: text("Name"),
        artist: text("Artist"),
        album: text("Album"),
        genre: text("Genre"),
        comment: text("Comments"),
        rating,
        memory: TrackMemory {
            grid: grid(&tempos),
            cues: cues(&marks),
        },
    });
    (track, index)
}

/// A finite number that is not negative, written with a point or a comma.
fn number(event: &Event, key: &str) -> Option<f64> {
    event
        .attribute(key)
        .and_then(|value| value.trim().replace(',', ".").parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
}

/// The grid of the tempo marks: a marker on the first downbeat of each mark, with its tempo
/// where it differs from that of the grid. A mark on another beat of the bar is moved back
/// to the downbeat before it, or forward a bar when that falls before the start.
fn grid(tempos: &[(f64, f64, u32)]) -> BeatGrid {
    let mut grid = BeatGrid::default();
    let mut first = true;
    for (start, bpm, beat) in tempos {
        let Ok(tempo) = Bpm::new(*bpm) else {
            continue;
        };
        let back = f64::from(beat.saturating_sub(1) % 4) * tempo.beat_seconds();
        let mut downbeat = start - back;
        if downbeat < 0.0 {
            downbeat += 4.0 * tempo.beat_seconds();
        }
        if first {
            grid.set_bpm_from(tempo, downbeat);
            first = false;
        } else if grid.add_marker(downbeat) && grid.bpm() != Some(tempo) {
            grid.set_marker_bpm(downbeat, Some(tempo));
        }
    }
    grid
}

/// The cues of the position marks: hot cues in their slots, then the other marks in the slots
/// left, in the order of the file, as long as there is room.
fn cues(marks: &[(f64, f64, i64, i64, String)]) -> Cues {
    let mut cues = Cues::default();
    let cue = |(start, end, kind, _, name): &(f64, f64, i64, i64, String)| {
        let kind = match kind {
            1 => CueKind::FadeIn,
            2 => CueKind::FadeOut,
            3 => CueKind::Load,
            4 => CueKind::Loop,
            _ => CueKind::Cue,
        };
        Cue::of_kind(*start, kind, end - start, CueName::new(name.trim()))
    };
    let mut later = Vec::new();
    for mark in marks {
        let slot = u8::try_from(mark.3)
            .ok()
            .and_then(|number| HotcueSlot::from_number(f64::from(number) + 1.0).ok());
        match (slot, cue(mark)) {
            (Some(slot), Some(found)) if cues.get(slot).is_none() => {
                cues.store(slot, found);
            }
            (_, Some(found)) => later.push(found),
            (_, None) => {}
        }
    }
    for found in later {
        // A memory cue often repeats a hot cue at the same place.
        let repeated = cues.iter().any(|(_, cue)| {
            cue.kind() == found.kind() && (cue.position - found.position).abs() < 0.001
        });
        if repeated {
            continue;
        }
        let Some(slot) = cues.first_free() else {
            break;
        };
        cues.store(slot, found);
    }
    cues
}

/// The tracks by the identifier and by the location the file gives them.
#[derive(Default)]
struct Keys {
    ids: HashMap<String, usize>,
    locations: HashMap<String, usize>,
}

/// The nodes from `index` up to the end of the element that holds them; returns them and the
/// index after that end.
fn node_children(
    events: &[Event],
    mut index: usize,
    keys: &Keys,
    nodes: &mut usize,
    entries: &mut usize,
    skipped: &mut usize,
) -> (Vec<SharedNode>, usize) {
    let mut children = Vec::new();
    while let Some(event) = events.get(index) {
        index += 1;
        match event {
            Event::End { .. } => break,
            Event::Start { name, empty, .. } if name == "NODE" => {
                let kind = event.attribute("Type").map(str::trim);
                let by_location = event.attribute("KeyType").map(str::trim) == Some("1");
                let title = event
                    .attribute("Name")
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .unwrap_or("Playlist")
                    .to_owned();
                let (node, next) = if kind == Some("1") {
                    let mut tracks = Vec::new();
                    if !empty {
                        while let Some(inner) = events.get(index) {
                            index += 1;
                            match inner {
                                Event::End { name } if name == "NODE" => break,
                                Event::Start { name, .. } if name == "TRACK" => {
                                    let key = inner.attribute("Key").unwrap_or_default();
                                    let found = if by_location {
                                        keys.locations.get(key)
                                    } else {
                                        keys.ids.get(key.trim())
                                    };
                                    match found {
                                        Some(track) if *entries < MAX_COLLECTION_TRACKS => {
                                            *entries += 1;
                                            tracks.push(*track);
                                        }
                                        _ => *skipped += 1,
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    (
                        SharedNode {
                            name: title,
                            playlist: Some(tracks),
                            children: Vec::new(),
                        },
                        index,
                    )
                } else {
                    let (inner, next) = if *empty {
                        (Vec::new(), index)
                    } else {
                        node_children(events, index, keys, nodes, entries, skipped)
                    };
                    (
                        SharedNode {
                            name: title,
                            playlist: None,
                            children: inner,
                        },
                        next,
                    )
                };
                index = next;
                if *nodes < MAX_COLLECTION_NODES {
                    *nodes += 1;
                    children.push(node);
                } else {
                    *skipped += 1;
                }
            }
            _ => {}
        }
    }
    (children, index)
}
