//! What the interface draws: a view of the application at one moment.
//!
//! The application builds a [`View`] every frame from the state snapshot of the engine, the
//! audio output, the settings and the file list. The interface draws it and answers with
//! commands. The capture mode builds the same values from invented data, without an engine
//! or a device.

use std::path::PathBuf;

use platter_core::DeckId;
use platter_engine::Snapshot;

use crate::i18n::Language;
use crate::keymap::Keymap;

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
    /// Names of the two files of the user, as the pages mention them.
    pub settings_file: String,
    pub keymap_file: String,
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
    },
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
    /// The list shows the collection.
    pub current: bool,
    pub importing: bool,
    pub imported: Option<ImportOutcome>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrowserView {
    /// The collection node above the folders, when the browser has a library.
    pub collection: Option<CollectionView>,
    pub tree: Vec<TreeRow>,
    /// The text of the search field.
    pub search: String,
    /// The folder the list shows; `None` while it shows the collection.
    pub folder: Option<PathBuf>,
    /// Names of the folders from a root of the tree down to the current one.
    pub crumbs: Vec<String>,
    pub rows: Vec<FileRow>,
    pub selected: Option<usize>,
    /// The current folder is still being read.
    pub reading: bool,
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
}
