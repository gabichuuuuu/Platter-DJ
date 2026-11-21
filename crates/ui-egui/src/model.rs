//! What the interface draws: a view of the application at one moment.
//!
//! The application builds a [`View`] every frame from the state snapshot of the engine, the
//! audio output, the settings and the file list. The interface draws it and answers with
//! commands. The capture mode builds the same values from invented data, without an engine
//! or a device.

use crate::layout::Layouts;
use std::path::PathBuf;

use platter_core::DeckId;
use platter_engine::Snapshot;
use platter_library::{NodeId, SessionId};

use crate::collection::TagWriting;
use crate::command::{DeckLayout, DeckSize, EffectUnits, ListColumn, SortColumn, WaveColour};
use crate::i18n::Language;
use crate::keymap::Keymap;
use crate::keys::KeyNotation;

/// Time of day for the clock of the status header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    pub hour: u8,
    pub minute: u8,
}

/// How an audio device is connected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputKind {
    BuiltIn,
    Usb,
    Bluetooth,
    #[default]
    Other,
}

/// What a running audio stream reports.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamView {
    pub kind: OutputKind,
    pub sample_rate: u32,
    pub channels: u16,
    /// Frames per buffer the device uses, which may differ from the setting.
    pub buffer_frames: u32,
    pub latency_ms: f64,
    /// Share of the buffer time the audio thread needs, `0.0..=1.0` and beyond.
    pub load: f32,
    pub dropouts: u64,
    /// The headphones have outputs of their own.
    pub headphones: bool,
}

/// Why no audio device is playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioProblem {
    /// The system has no output device.
    NoDevice,
    /// The device chosen in the settings is not connected.
    DeviceMissing,
    /// The device does not accept the sample rate chosen.
    Unsupported,
    /// Another program holds the device.
    Busy,
    /// The system refused for another reason; the log has the details.
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioState {
    Running(StreamView),
    Stopped(AudioProblem),
}

/// An output device, for the settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceView {
    pub name: String,
    pub outputs: u16,
    pub inputs: u16,
    /// Which of the sample rates on offer the device accepts.
    pub sample_rates: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioView {
    pub state: AudioState,
    pub devices: Vec<DeviceView>,
    /// Sample rates and buffer sizes the settings offer.
    pub sample_rates: Vec<u32>,
    pub buffer_sizes: Vec<u32>,
}

impl AudioView {
    pub fn stream(&self) -> Option<&StreamView> {
        match &self.state {
            AudioState::Running(stream) => Some(stream),
            AudioState::Stopped(_) => None,
        }
    }

    /// Whether pre-listening can be heard.
    pub fn has_headphones(&self) -> bool {
        self.stream().is_some_and(|stream| stream.headphones)
    }
}

/// The settings as the preference pages show them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsView {
    /// Name of the chosen device; `None` is the default device of the system.
    pub device: Option<String>,
    pub sample_rate: u32,
    pub buffer_frames: u32,
    /// Output of the device for master left, master right, headphones left and right.
    pub routing: [u16; 4],
    pub language: Language,
    /// The window covers the whole screen.
    pub fullscreen: bool,
    /// The keys show their tooltips.
    pub tooltips: bool,
    /// The window goes to the whole screen as Platter starts.
    pub fullscreen_at_start: bool,
    /// The range every tempo fader gets, in percent.
    pub tempo_range: u32,
    /// The size of decks A and B.
    pub deck_size: DeckSize,
    /// Two decks or four.
    pub deck_layout: DeckLayout,
    /// The layouts of the Layouts page, and what the active one shows besides the decks.
    pub layouts: Layouts,
    pub shown: Shown,
    /// The colours of the waveforms, from the Decks page.
    pub wave_colour: WaveColour,
    /// Two effect units or four, and the unit each panel of the effect row shows.
    pub effect_units: EffectUnits,
    pub fx_panels: [platter_engine::FxUnit; 2],
    /// Names of the files of the user, as the pages mention them.
    pub settings_file: String,
    pub keymap_file: String,
    pub library_file: String,
    /// Seconds a deck plays a track before it counts as played.
    pub min_playtime: u32,
    /// How the browser writes keys.
    pub key_notation: KeyNotation,
    /// The tempos the analysis looks in, lowest and highest, in whole BPM.
    pub bpm_range: (u32, u32),
    /// A playing deck takes no track.
    pub load_stopped_only: bool,
    /// Key lock runs on worker threads.
    pub key_lock_threads: bool,
    /// The folder the mix recorder writes into, as the Recording page shows it, and whether
    /// it is one the user chose.
    pub recording_folder: String,
    pub recording_folder_chosen: bool,
    /// The start of the names of the recordings, and the size their files split at, in MiB.
    pub recording_prefix: String,
    pub recording_split: u32,
    /// The music folders of the Library page, and the one it shows.
    pub music_folders: Vec<PathBuf>,
    pub music_folder: Option<usize>,
    /// Their music is imported when Platter starts.
    pub import_at_start: bool,
    /// The collection is checked, with the maintenance open, when Platter starts.
    pub check_at_start: bool,
    /// What Platter writes into the files.
    pub tag_writing: TagWriting,
    /// The controllers of the settings, for the Controllers page.
    pub controllers: ControllerManager,
}

/// What the Controllers page of the preferences shows: `docs/behavior/controllers.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerManager {
    pub controllers: Vec<ControllerRow>,
    /// The MIDI inputs of the system, by name.
    pub inputs: Vec<String>,
    /// The controller the page shows.
    pub selected: Option<usize>,
    /// The assignment Learn, Duplicate and Delete act on.
    pub binding: Option<usize>,
    pub learning: bool,
    /// The deck a new assignment drives.
    pub target: DeckId,
    /// The mapping files in the folder the explorer shows, which Import offers.
    pub importable: Vec<PathBuf>,
}

impl Default for ControllerManager {
    fn default() -> Self {
        Self {
            controllers: Vec::new(),
            inputs: Vec::new(),
            selected: None,
            binding: None,
            learning: false,
            target: DeckId::A,
            importable: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControllerRow {
    pub input: String,
    /// The name of its mapping file.
    pub mapping: String,
    /// Its input is open and its mapping in use.
    pub open: bool,
    /// Why the mapping or the input is not in use.
    pub problem: Option<String>,
    pub bindings: Vec<BindingRow>,
}

/// One assignment of a mapping, as the table shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingRow {
    /// The command it sends; `None` for a button that sets a modifier.
    pub command: Option<String>,
    pub deck: Option<DeckId>,
    pub control: ControlType,
    /// The conditions on the modifiers, `(modifier, value)`.
    pub when: Vec<(u8, u8)>,
    /// The modifier a button sets, and to which value.
    pub sets: Option<(u8, u8)>,
    /// Its control as the mapping names it (`note 1 36`); `None` while it waits for Learn.
    pub midi: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlType {
    Button,
    ButtonHeld,
    Fader,
    Encoder,
}

/// A folder of the tree on the left of the file selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    /// Level below the roots of the tree.
    pub depth: usize,
    /// Its folders are shown below it.
    pub open: bool,
    /// The folder the file list shows.
    pub current: bool,
    /// Audio files in the folder, once it has been read.
    pub files: Option<usize>,
}

/// A folder or a playlist of the library, in the tree below *Playlists*.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistRow {
    pub id: NodeId,
    pub name: String,
    /// Level below *Playlists*.
    pub depth: usize,
    pub folder: bool,
    /// A smart playlist, whose tracks come from its rules.
    pub smart: bool,
    /// The rules of a smart playlist as text (`platter_library::rules_text`); empty for the
    /// others.
    pub rules: String,
    /// Entries of a playlist, or tracks a smart playlist holds; `None` for a folder.
    pub entries: Option<usize>,
    /// The playlist the list shows.
    pub current: bool,
    /// The playlist used as the preparation list.
    pub preparation: bool,
    /// The favourite place the playlist is in, if any.
    pub favourite: Option<usize>,
}

/// A session of the History node of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionRow {
    pub id: SessionId,
    /// When it opened, as the tree shows it.
    pub label: String,
    /// Tracks played in it that are still in the collection.
    pub tracks: usize,
    /// The history the list shows.
    pub current: bool,
}

/// What the tags and the header of an audio file say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackRow {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub bpm: Option<f64>,
    pub seconds: Option<f64>,
    /// Name of the container, such as `FLAC`; empty when it is not known.
    pub format: String,
    /// Zero when it is not known.
    pub sample_rate: u32,
    /// A track of the collection whose file was gone at the last consistency check.
    pub missing: bool,
    /// A track of the collection played in the session of this run.
    pub played: bool,
    /// A track of the collection that is in the preparation list.
    pub prepared: bool,
    /// The colour tag of a track of the collection, 1 to 8, or 0 for none.
    pub colour: u8,
    /// The rating of a track of the collection, 0 to 5; `None` for a file of the explorer.
    pub rating: Option<u8>,
    /// Genre and comment of a track of the collection, for its details.
    pub genre: Option<String>,
    pub comment: Option<String>,
    /// The key the analysis found, as the collection keeps it (`crate::keys`).
    pub key: Option<u8>,
    /// A track of the collection with a set of stems linked to it: `docs/behavior/stems.md`.
    pub stems: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowKind {
    Folder,
    Track(TrackRow),
    /// An audio file whose header has not been read yet.
    Pending,
    /// A file with an audio extension that cannot be read.
    Unreadable,
}

/// The cover of a track, decoded and scaled down, 8-bit RGBA with straight alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverImage {
    pub width: u32,
    pub height: u32,
    pub rgba: std::sync::Arc<[u8]>,
}

/// A row of the file list.
#[derive(Clone, Debug, PartialEq)]
pub struct FileRow {
    pub path: PathBuf,
    /// Name of the file or the folder.
    pub name: String,
    pub kind: RowKind,
}

/// What the last import of a folder into the collection did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportOutcome {
    Done {
        added: usize,
        known: usize,
        unreadable: usize,
        /// The user stopped it before its end.
        stopped: bool,
    },
    Failed,
}

/// What the last export or import of playlist files did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaylistFileOutcome {
    /// The name of the file written and how many entries it holds.
    Exported {
        file: String,
        entries: usize,
    },
    Imported {
        playlists: usize,
        tracks: usize,
        skipped: usize,
    },
    Failed,
}

/// What the last action of the maintenance of the collection did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintenanceOutcome {
    /// A consistency check: tracks newly marked missing, and tracks whose file came back.
    Checked {
        missing: usize,
        found: usize,
    },
    Relocated {
        relocated: usize,
        not_found: usize,
        /// Left because their file name was not unique.
        ambiguous: usize,
    },
    /// Missing tracks taken out of the collection.
    Removed(usize),
    Failed,
}

/// The collection as the browser shows it: its node in the tree and the state of the
/// library behind it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CollectionView {
    /// `None` while the library is being opened.
    pub available: Option<bool>,
    /// Tracks in the collection, once the library has counted them.
    pub tracks: Option<usize>,
    /// Tracks that match the search text, once the library has answered it.
    pub matched: Option<usize>,
    /// The list shows the whole collection.
    pub current: bool,
    /// The playlist the list shows, if it shows one.
    pub playlist: Option<NodeId>,
    /// The session whose history the list shows, if it shows one.
    pub history: Option<SessionId>,
    /// The session of this run, once the library has opened it.
    pub session: Option<SessionRow>,
    /// Earlier sessions where tracks were played, the latest first.
    pub sessions: Vec<SessionRow>,
    /// The folders and playlists of the library, in the order of the tree.
    pub playlists: Vec<PlaylistRow>,
    pub importing: bool,
    /// The files the import running has looked at so far.
    pub import_looked: usize,
    pub imported: Option<ImportOutcome>,
    /// The paths of the tracks marked missing, the first ones, as the maintenance lists them.
    pub missing: Vec<String>,
    /// How many tracks are marked missing in all, once the library has said.
    pub missing_total: Option<usize>,
    /// A check, a relocation or a removal is running.
    pub maintaining: bool,
    pub maintained: Option<MaintenanceOutcome>,
    /// A playlist file is being written or read.
    pub exchanging: bool,
    pub exchanged: Option<PlaylistFileOutcome>,
}

impl CollectionView {
    /// The list shows the collection, a playlist or a history, not a folder.
    pub fn shown(&self) -> bool {
        self.current || self.playlist.is_some() || self.history.is_some()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrowserView {
    /// The collection node above the folders, when the browser has a library.
    pub collection: Option<CollectionView>,
    pub tree: Vec<TreeRow>,
    /// The text of the search field.
    pub search: String,
    /// The column the list is sorted by, and whether the order is turned round.
    pub sort: Option<(SortColumn, bool)>,
    /// The columns the list shows besides the title, in their order.
    pub columns: Vec<ListColumn>,
    /// The folder the list shows; `None` while it shows the collection.
    pub folder: Option<PathBuf>,
    /// The folder the explorer shows or last showed, even while the list shows the library.
    pub explorer: Option<PathBuf>,
    /// The JPEG and PNG files of that folder, by name, the first
    /// [`crate::files::MAX_PICTURES`]: the pictures a cover can be imported from.
    pub pictures: Vec<PathBuf>,
    /// The folders of playlists closed in the tree.
    pub closed_folders: Vec<NodeId>,
    /// The earlier sessions show under *History*.
    pub history_open: bool,
    /// The folder of the mix recorder, which *Recordings* shows; `None` without one.
    pub recordings: Option<PathBuf>,
    /// The covers read of the tracks of the list, by path.
    pub covers: std::collections::HashMap<PathBuf, CoverImage>,
    /// Names of the folders from a root of the tree down to the current one.
    pub crumbs: Vec<String>,
    pub rows: Vec<FileRow>,
    /// The current row.
    pub selected: Option<usize>,
    /// Every selected row, in the order of the list; the current row alone unless several
    /// are selected.
    pub marked: Vec<usize>,
    /// The current folder is still being read.
    pub reading: bool,
    /// The analysis of the collection in the background, while it runs.
    pub analysis: Option<AnalysisStatus>,
    /// A track being separated into stems: `docs/behavior/stems.md`, *Separation*.
    pub separation: Option<SeparationStatus>,
    /// The separation model is installed.
    pub separation_model: bool,
    /// Why the analysis of the collection stopped, if it did.
    pub problem: Option<String>,
    /// The covers of the tracks the decks hold.
    pub deck_covers: [Option<CoverImage>; DeckId::COUNT],
}

/// The track being separated into stems and the share done, from 0 to 1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeparationStatus {
    pub track: String,
    pub done: f32,
}

/// How far the analysis of the collection in the background has gone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnalysisStatus {
    pub done: usize,
    pub total: usize,
}

impl BrowserView {
    /// Audio files of the current folder.
    pub fn file_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| !matches!(row.kind, RowKind::Folder))
            .count()
    }
}

/// Something the user must know about a deck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeckNotice {
    /// The last file sent to the deck could not be read.
    LoadFailed,
    /// The stems of the last track could not be played; the track plays alone.
    StemsFailed,
}

/// The MIDI controllers of the settings, for the indicator of the status header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MidiView {
    /// Controllers the settings file names.
    pub named: usize,
    /// Those whose input opened.
    pub open: usize,
    /// A message came from an open controller a moment ago.
    pub active: bool,
}

pub struct View<'a> {
    pub engine: &'a Snapshot,
    pub audio: &'a AudioView,
    pub settings: &'a SettingsView,
    pub browser: &'a BrowserView,
    pub keymap: &'a Keymap,
    /// Absent when the time of day is not known.
    pub clock: Option<Clock>,
    pub notices: [Option<DeckNotice>; DeckId::COUNT],
    pub midi: MidiView,
}

/// What the active layout shows besides the decks: `docs/behavior/interface.md`, *Layouts*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub effects: bool,
    pub mixer: bool,
    /// The parallel lanes, with two decks.
    pub lanes: bool,
    /// Deck A alone across the window.
    pub single: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Self {
            effects: true,
            mixer: true,
            lanes: true,
            single: false,
        }
    }
}

impl SettingsView {
    /// The decks the window shows, left column first in each row.
    pub fn shown_decks(&self) -> &'static [platter_core::DeckId] {
        if self.shown.single {
            &[platter_core::DeckId::A]
        } else {
            self.deck_layout.decks()
        }
    }
}
