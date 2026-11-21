//! The collection file other DJ programs share. Behaviour note: `docs/behavior/library.md`,
//! section *Collection files*.

use std::path::PathBuf;

use platter_core::{CueKind, HotcueSlot};
use platter_library::{CollectionError, MAX_COLLECTION_BYTES, read_collection};

/// A file as the format describes it: two tracks with grids and marks, a third without a
/// location, and a tree with a folder, a playlist by identifier and one by location.
const FILE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="Other DJ" Version="6.0" Company="Someone"/>
  <COLLECTION Entries="3">
    <TRACK TrackID="11" Name="Night &amp; Day" Artist="Ana" Album="One" Genre="House"
           Comments="warm up" Rating="204" AverageBpm="124.00"
           Location="file://localhost/C:/Music/Ana%20-%20Night.mp3">
      <TEMPO Inizio="0.120" Bpm="124.00" Metro="4/4" Battito="1"/>
      <TEMPO Inizio="60.120" Bpm="126.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="Drop" Type="0" Start="32.5" Num="2"/>
      <POSITION_MARK Name="" Type="0" Start="32.5" Num="-1"/>
      <POSITION_MARK Name="Out" Type="2" Start="200.0" Num="-1"/>
      <POSITION_MARK Name="Roll" Type="4" Start="64.0" End="66.0" Num="0"/>
    </TRACK>
    <TRACK TrackID="12" Name="Second" Location="file://localhost/Users/dj/Music/second.flac">
      <TEMPO Inizio="0.5" Bpm="120" Metro="4/4" Battito="3"/>
    </TRACK>
    <TRACK TrackID="13" Name="Streamed" Location="https://example.com/track"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="2">
      <NODE Type="0" Name="Sets" Count="1">
        <NODE Name="Friday" Type="1" KeyType="0" Entries="3">
          <TRACK Key="12"/>
          <TRACK Key="11"/>
          <TRACK Key="99"/>
        </NODE>
      </NODE>
      <NODE Name="By path" Type="1" KeyType="1" Entries="1">
        <TRACK Key="file://localhost/C:/Music/Ana%20-%20Night.mp3"/>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

#[test]
fn tracks_their_grids_and_cues_and_the_tree_of_playlists_are_read() {
    let collection = read_collection(FILE.as_bytes()).unwrap();
    assert_eq!(collection.product.as_deref(), Some("Other DJ"));
    assert_eq!(collection.tracks.len(), 2);
    // The track without a file and the entry naming no track.
    assert_eq!(collection.skipped, 2);

    let first = &collection.tracks[0];
    assert_eq!(first.path, PathBuf::from("C:/Music/Ana - Night.mp3"));
    assert_eq!(first.title.as_deref(), Some("Night & Day"));
    assert_eq!(first.artist.as_deref(), Some("Ana"));
    assert_eq!(first.genre.as_deref(), Some("House"));
    assert_eq!(first.comment.as_deref(), Some("warm up"));
    assert_eq!(first.rating, 4);
    let grid = &first.memory.grid;
    assert_eq!(grid.bpm().map(|bpm| bpm.value()), Some(124.0));
    assert_eq!(grid.markers(), [0.12, 60.12]);
    assert_eq!(grid.bpm_at(61.0).map(|bpm| bpm.value()), Some(126.0));
    let cues = &first.memory.cues;
    let slot = |number: f64| cues.get(HotcueSlot::from_number(number).unwrap());
    let roll = slot(1.0).unwrap();
    assert_eq!(
        (roll.kind(), roll.position, roll.length()),
        (CueKind::Loop, 64.0, 2.0)
    );
    let drop = slot(3.0).unwrap();
    assert_eq!((drop.name.as_str(), drop.position), ("Drop", 32.5));
    // The memory cue at the place of the hot cue is not stored twice; the fade-out mark
    // takes the first free slot.
    let out = slot(2.0).unwrap();
    assert_eq!((out.kind(), out.position), (CueKind::FadeOut, 200.0));
    assert!(slot(4.0).is_none());

    // A mark on the third beat is moved back two beats to its downbeat, which would fall
    // before the start, so it goes a bar forward.
    let second = &collection.tracks[1];
    assert_eq!(second.path, PathBuf::from("/Users/dj/Music/second.flac"));
    assert_eq!(second.memory.grid.markers(), [1.5]);

    let [sets, by_path] = &collection.playlists[..] else {
        panic!("{:?}", collection.playlists);
    };
    assert_eq!((sets.name.as_str(), sets.playlist.as_ref()), ("Sets", None));
    let friday = &sets.children[0];
    assert_eq!(friday.name, "Friday");
    assert_eq!(friday.playlist.as_deref(), Some(&[1, 0][..]));
    assert_eq!(by_path.playlist.as_deref(), Some(&[0][..]));
}

#[test]
fn other_files_are_refused() {
    for other in [
        &b"<?xml version=\"1.0\"?><rss></rss>"[..],
        b"not xml at all",
        b"<DJ_PLAYLISTS><COLLECTION>",
        b"",
    ] {
        assert_eq!(
            read_collection(other),
            Err(CollectionError::NotACollection),
            "{}",
            String::from_utf8_lossy(other)
        );
    }
    let huge = vec![b' '; MAX_COLLECTION_BYTES + 1];
    assert_eq!(read_collection(&huge), Err(CollectionError::TooLarge));
}
