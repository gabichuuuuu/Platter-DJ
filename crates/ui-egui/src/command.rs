//! The commands of the application around the engine, and the two kinds together.
//!
//! The engine has its commands (`platter_engine::Command`): play, set a fader, load a
//! track. The application has more: open the preferences, choose the audio device, move the
//! selection of the file list. Both follow the same rule: a stable identifier, a label,
//! typed parameters. Controls, keyboard shortcuts and anything that comes later build an
//! [`Action`] and hand it to the application; nothing changes state by another path.

use crate::layout::{ZONES, Zone};
use std::path::PathBuf;

use platter_core::{DeckId, ValueError};
use platter_engine::command::{
    Args, CommandError, CommandSpec, ParamKind, ParamSpec, SPECS, Value,
};
use platter_engine::{Command, FxUnit};

use platter_library::{
    MAX_NAME_CHARS, MAX_RULES, NodeId, RangeField, Rule, SessionId, TagField, parse_rules,
};

use crate::collection::{TAG_WRITINGS, TagWriting};
use crate::keys::{KEY_NOTATIONS, KeyNotation};
use crate::played::DEFAULT_PLAYTIME;

use crate::i18n::Language;

/// What the dialog of the details of a track edits. An empty text clears its tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackDetails {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub comment: String,
    /// 0 to 5.
    pub rating: u8,
}

/// A field of the dialog of the details of a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailField {
    Title,
    Artist,
    Album,
    Genre,
    Comment,
    Rating,
}

impl DetailField {
    pub const ALL: [Self; 6] = [
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::Genre,
        Self::Comment,
        Self::Rating,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Genre => "genre",
            Self::Comment => "comment",
            Self::Rating => "rating",
        }
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

pub const DETAIL_FIELDS: &[&str] = &["title", "artist", "album", "genre", "comment", "rating"];

/// A set of fields of the dialog of the details.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetailFields(u8);

impl DetailFields {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self(0b11_1111);

    pub fn contains(self, field: DetailField) -> bool {
        self.0 & field.bit() != 0
    }

    pub fn insert(&mut self, field: DetailField) {
        self.0 |= field.bit();
    }

    pub fn toggle(&mut self, field: DetailField) {
        self.0 ^= field.bit();
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The fields named in `text`, separated by commas; `None` when a name is unknown.
    pub fn parse(text: &str) -> Option<Self> {
        let mut fields = Self::NONE;
        for name in text
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let field = DetailField::ALL
                .into_iter()
                .find(|field| field.name() == name)?;
            fields.insert(field);
        }
        Some(fields)
    }
}

/// What follows saving the details: the dialog closes, or shows the position before or after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailsThen {
    Close,
    Previous,
    Next,
}

impl DetailsThen {
    pub const ALL: [Self; 3] = [Self::Close, Self::Previous, Self::Next];

    pub fn name(self) -> &'static str {
        match self {
            Self::Close => "close",
            Self::Previous => "previous",
            Self::Next => "next",
        }
    }
}

const DETAILS_THEN: &[&str] = &["close", "previous", "next"];

/// A page of the preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsPage {
    #[default]
    Audio,
    Input,
    Sync,
    General,
    Decks,
    Mixer,
    Recording,
    Layouts,
    Library,
    Controllers,
    Keyboard,
}

impl SettingsPage {
    pub const ALL: [Self; 11] = [
        Self::Audio,
        Self::Input,
        Self::Sync,
        Self::General,
        Self::Decks,
        Self::Mixer,
        Self::Recording,
        Self::Layouts,
        Self::Library,
        Self::Controllers,
        Self::Keyboard,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Input => "input",
            Self::Sync => "sync",
            Self::General => "general",
            Self::Decks => "decks",
            Self::Mixer => "mixer",
            Self::Recording => "recording",
            Self::Layouts => "layouts",
            Self::Library => "library",
            Self::Controllers => "controllers",
            Self::Keyboard => "keyboard",
        }
    }
}

/// A page of the advanced panel of a deck.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeckPage {
    #[default]
    Move,
    Cue,
    Grid,
}

impl DeckPage {
    pub const ALL: [Self; 3] = [Self::Move, Self::Cue, Self::Grid];

    pub fn name(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Cue => "cue",
            Self::Grid => "grid",
        }
    }
}

/// One side of one output pair of the engine, to be placed on an output of the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteSlot {
    MasterLeft,
    MasterRight,
    HeadphonesLeft,
    HeadphonesRight,
}

impl RouteSlot {
    pub const ALL: [Self; 4] = [
        Self::MasterLeft,
        Self::MasterRight,
        Self::HeadphonesLeft,
        Self::HeadphonesRight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MasterLeft => "master_left",
            Self::MasterRight => "master_right",
            Self::HeadphonesLeft => "headphones_left",
            Self::HeadphonesRight => "headphones_right",
        }
    }

    /// Position in the routing of the settings: master pair first, then headphones.
    pub fn index(self) -> usize {
        match self {
            Self::MasterLeft => 0,
            Self::MasterRight => 1,
            Self::HeadphonesLeft => 2,
            Self::HeadphonesRight => 3,
        }
    }
}

/// A level meter with a clip indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeterId {
    Deck(DeckId),
    Master,
}

impl MeterId {
    pub const ALL: [Self; DeckId::COUNT + 1] = [
        Self::Deck(DeckId::A),
        Self::Deck(DeckId::B),
        Self::Deck(DeckId::C),
        Self::Deck(DeckId::D),
        Self::Master,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Deck(DeckId::A) => "a",
            Self::Deck(DeckId::B) => "b",
            Self::Deck(DeckId::C) => "c",
            Self::Deck(DeckId::D) => "d",
            Self::Master => "master",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Deck(deck) => deck.index(),
            Self::Master => DeckId::COUNT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppCommand {
    SettingsOpen {
        page: SettingsPage,
    },
    SettingsClose,
    SettingsRestoreDefaults,
    /// `None` is the default device of the system.
    AudioDeviceSet {
        device: Option<String>,
    },
    AudioSampleRateSet {
        hz: u32,
    },
    AudioBufferSet {
        frames: u32,
    },
    /// The input device; `None` opens no input (`docs/behavior/live-input.md`).
    InputDeviceSet {
        device: Option<String>,
    },
    /// The MIDI output the clock goes to; `None` sends it nowhere
    /// (`docs/behavior/midi-clock.md`).
    ClockMidiOutputSet {
        port: Option<String>,
    },
    /// Milliseconds the MIDI clock goes out later (positive) or earlier than the audio.
    ClockMidiOffsetSet {
        ms: i32,
    },
    /// The MIDI input whose clock the master clock can follow; `None` listens to none.
    ClockMidiInputSet {
        port: Option<String>,
    },
    /// Puts one side of an output pair on an output of the device, counted from zero.
    AudioRoutingSet {
        slot: RouteSlot,
        output: u16,
    },
    AudioDropoutsReset,
    LanguageSet {
        language: Language,
    },
    /// Opens the keyboard shortcut file in the editor of the system.
    KeymapOpen,
    /// Shows the window on the whole screen, or as a window again.
    FullscreenToggle,
    /// Turns the tooltips of the keys on or off.
    TooltipsToggle,
    /// Turns on or off the start in full screen.
    FullscreenAtStartToggle,
    /// Sets the size of decks A and B.
    DeckSizeSet {
        size: DeckSize,
    },
    /// Shows two decks or four: `docs/behavior/interface.md`, *Deck layout*.
    DeckLayoutSet {
        layout: DeckLayout,
    },
    /// Chooses the colours of every waveform: `docs/behavior/waveform.md`, *Colour modes*.
    WaveColourSet {
        mode: WaveColour,
    },
    /// Shows two effect units or four: `docs/behavior/effects.md`, *Units and modes*.
    EffectUnitsSet {
        units: EffectUnits,
    },
    /// Shows an effect unit in its panel: units 1 and 3 on the left, 2 and 4 on the right.
    FxPanelShow {
        unit: FxUnit,
    },
    /// Sets the range every tempo fader gets, in percent: `docs/behavior/settings.md`.
    TempoRangeSet {
        percent: u32,
    },
    /// Sets how long a deck plays a track before it counts as played, in seconds.
    MinPlaytimeSet {
        seconds: u32,
    },
    /// Puts the selected track into the preview player and plays it.
    PreviewLoadSelected,
    /// The controller manager: `docs/behavior/controllers.md`, *The controller manager*.
    MidiControllerAdd,
    MidiControllerRemove {
        index: usize,
    },
    MidiControllerSelect {
        index: usize,
    },
    MidiInputSet {
        index: usize,
        input: String,
    },
    MidiMappingImport {
        index: usize,
        path: PathBuf,
    },
    MidiMappingExport {
        index: usize,
        folder: PathBuf,
    },
    MidiBindingSelect {
        index: usize,
    },
    MidiBindingAdd {
        command: String,
    },
    MidiBindingDuplicate,
    MidiBindingDelete,
    MidiLearnToggle,
    MidiTargetSet {
        deck: DeckId,
    },
    /// Turns on or off the rule that a playing deck takes no track.
    LoadStoppedOnlyToggle,
    /// Runs key lock on worker threads, or on the audio thread: `docs/behavior/keylock.md`,
    /// *Worker threads*.
    KeyLockThreadsToggle,
    /// The layouts of the window: `docs/behavior/interface.md`, *Layouts*.
    LayoutSelect {
        index: usize,
    },
    LayoutDuplicate,
    LayoutRename {
        name: String,
    },
    LayoutDelete,
    LayoutMoveUp,
    LayoutMoveDown,
    LayoutZoneToggle {
        zone: Zone,
    },
    /// The preferences of the mix recorder: `docs/behavior/recording.md`, *Preferences*.
    RecorderFolderSet {
        path: PathBuf,
    },
    RecorderFolderReset,
    RecorderPrefixSet {
        prefix: String,
    },
    RecorderSplitSet {
        megabytes: u32,
    },
    /// The overdub decay of the loop recorder, in percent: `docs/behavior/recording.md`,
    /// *Loop recorder*.
    LoopDecaySet {
        percent: u32,
    },
    /// The music folders of the Library page: `docs/behavior/library.md`.
    MusicFolderAdd {
        path: PathBuf,
    },
    MusicFolderRemove {
        index: usize,
    },
    MusicFolderSelect {
        index: usize,
    },
    ImportAtStartToggle,
    CheckAtStartToggle,
    /// Sets how the browser writes keys.
    KeyNotationSet {
        notation: KeyNotation,
    },
    /// Sets the tempos the analysis looks in: `docs/behavior/analysis.md`, *Range of tempos*.
    AnalysisRangeSet {
        min: u32,
        max: u32,
    },
    /// Sets what Platter writes into the files.
    TagWritingSet {
        mode: TagWriting,
    },
    /// Shows a folder in the file list.
    BrowserNavigate {
        path: PathBuf,
    },
    /// Opens or closes a folder of the tree.
    BrowserToggle {
        path: PathBuf,
    },
    /// Shows the folder that holds the current one.
    BrowserUp,
    BrowserSelect {
        index: usize,
    },
    /// Adds a row to the selection, or takes it out, and makes it the current row.
    BrowserSelectToggle {
        index: usize,
    },
    /// Selects every row from the current row to `index`; the current row stays.
    BrowserSelectExtend {
        index: usize,
    },
    /// Selects every row of the list.
    BrowserSelectAll,
    /// Moves the selection of the file list by a number of rows.
    BrowserMove {
        delta: i32,
    },
    /// Opens the selected folder.
    BrowserActivate,
    BrowserLoadSelected {
        deck: DeckId,
    },
    /// Loads the selected track into a deck without its stems.
    BrowserLoadSelectedTrack {
        deck: DeckId,
    },
    /// Links the audio files of a folder named after the stems to the selected track:
    /// `docs/behavior/stems.md`.
    BrowserStemsAttach {
        path: PathBuf,
    },
    /// The selected track forgets its stems; their files stay.
    BrowserStemsRemove,
    /// Separates the selected track into stems: `docs/behavior/stems.md`, *Separation*.
    BrowserStemsGenerate,
    /// Stops the separation that runs.
    BrowserStemsCancel,
    /// Installs the separation model from `4stems.tar.gz` in a folder.
    StemsModelInstall {
        path: PathBuf,
    },
    /// Shows a fixed node of the tree, such as the collection, in the list.
    BrowserNodeSelect {
        node: BrowserNode,
    },
    /// Shows the history of an earlier session in the list.
    BrowserHistorySelect {
        session: SessionId,
    },
    /// Closes a folder of playlists in the tree, or opens it.
    BrowserFolderToggle {
        folder: NodeId,
    },
    /// Hides the earlier sessions under *History*, or shows them.
    BrowserHistoryToggle,
    /// Makes a playlist of the tracks played in a session: `docs/behavior/browser.md`,
    /// *History*.
    BrowserHistoryToPlaylist {
        session: SessionId,
    },
    /// Writes the history of a session as an M3U file into a folder.
    BrowserHistoryExport {
        session: SessionId,
        path: PathBuf,
    },
    /// Forgets the history of a session of an earlier run.
    BrowserHistoryDelete {
        session: SessionId,
    },
    /// Shows a playlist of the library in the list.
    BrowserPlaylistSelect {
        playlist: NodeId,
    },
    /// Creates a playlist or a folder named `name` in a folder of the tree, or at its top
    /// with `None`.
    BrowserPlaylistCreate {
        folder: Option<NodeId>,
        item: NewItem,
        name: String,
    },
    /// Gives a playlist or a folder of the tree another name.
    BrowserPlaylistRename {
        item: NodeId,
        name: String,
    },
    /// Puts a playlist or a folder in another folder of the tree, or at its top with `None`.
    BrowserPlaylistMove {
        item: NodeId,
        folder: Option<NodeId>,
    },
    /// Adds the track at `path` at the end of a playlist, taking it into the collection
    /// first when it is not there.
    BrowserPlaylistAdd {
        playlist: NodeId,
        path: PathBuf,
    },
    /// Deletes a playlist of the library. A folder is not deleted this way.
    BrowserPlaylistDelete {
        playlist: NodeId,
    },
    /// Takes every entry out of a playlist.
    BrowserPlaylistClear {
        playlist: NodeId,
    },
    /// Takes the repeats of each track out of a playlist, keeping its first entry.
    BrowserPlaylistRemoveRepeats {
        playlist: NodeId,
    },
    /// Takes the selected entry out of the playlist the list shows.
    BrowserPlaylistRemove,
    /// Replaces the rules of a smart playlist: `docs/behavior/library.md`, *Smart playlists*.
    BrowserSmartRulesSet {
        smart: NodeId,
        rules: Vec<Rule>,
    },
    /// Makes a playlist, named after the smart playlist, of the tracks it holds now.
    BrowserSmartToPlaylist {
        smart: NodeId,
    },
    /// Makes a copy of a smart playlist, with its rules, beside it.
    BrowserSmartDuplicate {
        smart: NodeId,
    },
    /// Opens the dialog of the rules of a smart playlist.
    BrowserSmartRulesOpen {
        smart: NodeId,
    },
    /// Closes the dialog of the rules without saving them.
    BrowserSmartRulesClose,
    /// Adds a row to the dialog of the rules.
    BrowserSmartRuleAdd,
    /// Takes a row out of the dialog of the rules.
    BrowserSmartRuleRemove {
        row: usize,
    },
    /// Gives a row of the dialog of the rules another field, with an empty value.
    BrowserSmartRuleField {
        row: usize,
        field: RuleField,
    },
    /// Chooses the colour, the key, or *Yes* (0) and *No* (1), of a row of the dialog.
    BrowserSmartRuleChoice {
        row: usize,
        value: usize,
    },
    /// Sets the play count of the selected track to zero, so it is no longer marked played.
    BrowserPlayedReset,
    /// Deletes the selected track from the collection and from every playlist.
    BrowserTrackDelete,
    /// Tags the selected track with a colour, 1 to 8, or none with 0.
    BrowserTrackColourSet {
        colour: u8,
    },
    /// Rates the selected track, 0 to 5.
    BrowserTrackRatingSet {
        rating: u8,
    },
    /// Opens the dialog of the details of the selected tracks.
    BrowserTrackDetailsOpen,
    BrowserTrackDetailsClose,
    /// The rating chosen in the dialog, before it is saved.
    BrowserTrackDetailsRating {
        rating: u8,
    },
    /// Imports the picture at `path` as the front cover of the selected tracks.
    BrowserTrackCoverImport {
        path: PathBuf,
    },
    /// Deletes every picture of the selected tracks.
    BrowserTrackCoverDelete,
    /// Ticks a field of the dialog for several tracks, or unticks it.
    BrowserTrackDetailsTick {
        field: DetailField,
    },
    /// Stores `fields` of `details` in the library for the tracks at `position` of the dialog:
    /// 0 for every selected track, 1 and up for one of them. Then the dialog closes or moves.
    BrowserTrackDetailsSave {
        details: TrackDetails,
        fields: DetailFields,
        position: usize,
        then: DetailsThen,
    },
    /// Opens the dialog of the maintenance of the collection, over the preferences.
    LibraryMaintenanceOpen,
    LibraryMaintenanceClose,
    /// Looks for the file of every track and marks the tracks whose file is gone.
    LibraryCheck,
    /// Looks for the files of the missing tracks in a folder and the folders inside it.
    LibraryRelocate {
        path: PathBuf,
    },
    /// Takes the tracks marked missing out of the collection.
    LibraryMissingRemove,
    /// Asks whether to remove the missing tracks, in place of the key that removes them.
    LibraryMissingAsk,
    /// Answers the question: the missing tracks stay.
    LibraryMissingKeep,
    /// Writes a playlist as an M3U file into a folder.
    BrowserPlaylistExport {
        playlist: NodeId,
        path: PathBuf,
    },
    /// Makes a playlist of each playlist file of a folder, in a folder of the tree or at its
    /// top.
    BrowserPlaylistImport {
        folder: Option<NodeId>,
        path: PathBuf,
    },
    /// Imports the collection files of a folder: `docs/behavior/browser.md`, *Collection
    /// files*.
    BrowserCollectionImport {
        path: PathBuf,
    },
    /// Makes a playlist the preparation list.
    BrowserPreparationSelect {
        playlist: NodeId,
    },
    /// Adds the selected track at the end of the preparation list.
    BrowserPreparationAppend,
    /// Puts a playlist in a favourite place of the browser.
    BrowserFavouriteSet {
        place: usize,
        playlist: NodeId,
    },
    /// Empties a favourite place of the browser.
    BrowserFavouriteClear {
        place: usize,
    },
    /// Adds the selected track to the preparation list after the track loaded last.
    BrowserPreparationNext,
    /// Keeps the rows of the list whose tags hold every word of `text`.
    BrowserSearchSet {
        text: String,
    },
    /// Gives the keyboard to the search field.
    BrowserSearchFocus,
    /// Sorts the list by a column, or turns the order round when it is sorted by it already.
    BrowserSort {
        column: SortColumn,
    },
    /// Shows a column of the list, or hides it.
    BrowserColumnToggle {
        column: ListColumn,
    },
    /// Adds the music of a folder and of the folders inside it to the collection.
    LibraryImportFolder {
        path: PathBuf,
    },
    /// Stops the import that runs before the next file.
    LibraryImportStop,
    WaveformZoomIn {
        deck: DeckId,
    },
    WaveformZoomOut {
        deck: DeckId,
    },
    /// Shows the four lanes of the stems of a stem deck in its parallel lane, or folds them
    /// back to its waveform with the mute keys.
    WaveformStemLanesToggle {
        deck: DeckId,
    },
    /// Shows a page of the cells of a remix deck, from 0.
    RemixPageSet {
        deck: DeckId,
        page: usize,
    },
    /// Swaps remaining and elapsed time as the large reading of a deck.
    DeckTimeDisplayToggle {
        deck: DeckId,
    },
    DeckNoticeDismiss {
        deck: DeckId,
    },
    /// Shows a page of the advanced panel of a deck.
    DeckPageSet {
        deck: DeckId,
        page: DeckPage,
    },
    /// Switches the mapping mode of the hotcues of a deck: while it is on, the key of a
    /// hotcue takes the selected cue.
    DeckCueMapToggle {
        deck: DeckId,
    },
    MixerClipReset {
        meter: MeterId,
    },
}

const DECK: ParamSpec = ParamSpec {
    name: "deck",
    kind: ParamKind::Deck,
};

const PATH: ParamSpec = ParamSpec {
    name: "path",
    kind: ParamKind::Path,
};

const fn text(name: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Text,
    }
}

const fn number(name: &'static str, min: f64, max: f64) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Number { min, max },
    }
}

const fn choice(name: &'static str, choices: &'static [&'static str]) -> ParamSpec {
    ParamSpec {
        name,
        kind: ParamKind::Choice(choices),
    }
}

const PAGES: &[&str] = &[
    "audio",
    "input",
    "sync",
    "general",
    "decks",
    "mixer",
    "recording",
    "layouts",
    "library",
    "controllers",
    "keyboard",
];
const LANGUAGES: &[&str] = &["en", "es"];
const SLOTS: &[&str] = &[
    "master_left",
    "master_right",
    "headphones_left",
    "headphones_right",
];
const METERS: &[&str] = &["a", "b", "master"];
const DECK_PAGES: &[&str] = &["move", "cue", "grid"];
/// Outputs a device may have, as far as the routing goes.
pub const MAX_OUTPUTS: u16 = 64;
const MAX_ROWS: f64 = 1_000_000.0;
const NODES: &[&str] = &["collection", "preparation", "history"];
/// The largest identifier of a playlist or a session: the largest whole number a parameter carries exactly.
const MAX_NODE_ID: f64 = 9_007_199_254_740_991.0;
const PLAYLIST: ParamSpec = number("playlist", 1.0, MAX_NODE_ID);
const SMART: ParamSpec = number("smart", 1.0, MAX_NODE_ID);
const RULE_ROW: ParamSpec = number("row", 0.0, (MAX_RULES - 1) as f64);
const RULE_FIELDS: &[&str] = &[
    "title", "artist", "album", "genre", "tempo", "length", "rating", "plays", "imported",
    "played", "colour", "key", "missing", "analysed",
];
/// The places of the favourites a playlist can take; the first three are fixed.
pub const FAVOURITE_PLACES: std::ops::RangeInclusive<usize> = 4..=8;
const FAVOURITE_PLACE: ParamSpec = number("place", 4.0, 8.0);
const NEW_ITEMS: &[&str] = &["playlist", "folder", "smart"];
const NAME: ParamSpec = ParamSpec {
    name: "name",
    kind: ParamKind::Text,
};
/// Longest search text, in characters; the rest of a longer one is not taken.
pub const MAX_SEARCH_CHARS: usize = 200;

const SORT_COLUMNS: &[&str] = &[
    "title", "artist", "album", "bpm", "time", "key", "genre", "rating", "colour",
];

/// The columns of the browser list that can be sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Title,
    Artist,
    Album,
    Bpm,
    Time,
    Key,
    Genre,
    Rating,
    Colour,
}

impl SortColumn {
    pub const ALL: [Self; 9] = [
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::Bpm,
        Self::Time,
        Self::Key,
        Self::Genre,
        Self::Rating,
        Self::Colour,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Bpm => "bpm",
            Self::Time => "time",
            Self::Key => "key",
            Self::Genre => "genre",
            Self::Rating => "rating",
            Self::Colour => "colour",
        }
    }
}

const LIST_COLUMNS: &[&str] = &[
    "colour", "artist", "album", "genre", "bpm", "key", "time", "rating", "comment", "file",
];

/// The size decks A and B are drawn in: `docs/behavior/interface.md`, *Deck sizes*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeckSize {
    #[default]
    Full,
    /// A header of one row and no advanced panel.
    Compact,
}

impl DeckSize {
    pub const ALL: [Self; 2] = [Self::Full, Self::Compact];

    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Compact => "compact",
        }
    }

    /// The size a double click on the header of a deck switches to.
    pub fn other(self) -> Self {
        match self {
            Self::Full => Self::Compact,
            Self::Compact => Self::Full,
        }
    }
}

/// How many decks the window shows: `docs/behavior/interface.md`, *Deck layout*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeckLayout {
    /// Decks A and B, with the waveform lanes above them.
    Two,
    /// Decks A to D, C under A and D under B, each with its own waveform.
    #[default]
    Four,
}

impl DeckLayout {
    pub const ALL: [Self; 2] = [Self::Two, Self::Four];

    pub fn name(self) -> &'static str {
        match self {
            Self::Two => "two",
            Self::Four => "four",
        }
    }

    /// The decks the window shows, left column first in each row.
    pub fn decks(self) -> &'static [DeckId] {
        match self {
            Self::Two => &[DeckId::A, DeckId::B],
            Self::Four => &DeckId::ALL,
        }
    }
}

pub const DECK_SIZES: &[&str] = &["full", "compact"];
pub const DECK_LAYOUTS: &[&str] = &["two", "four"];
pub const WAVE_COLOURS: &[&str] = &["deck", "bands", "mono", "bass"];
/// The sizes the Recording page splits the files of the mix recorder at, in MiB.
pub const RECORDING_SPLITS: [u32; 3] = [512, 1024, 2048];
const SPLIT_NAMES: &[&str] = &["512", "1024", "2048"];

fn split_name(megabytes: u32) -> &'static str {
    match megabytes {
        512 => "512",
        1024 => "1024",
        _ => "2048",
    }
}

/// The colours of the three bands of every waveform: `docs/behavior/waveform.md`, *Colour
/// modes*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WaveColour {
    /// Each deck in its own hue, the bands by lightness.
    #[default]
    Deck,
    /// One colour per band, the same on every deck.
    Bands,
    /// Three greys.
    Mono,
    /// The low band in the hue of the deck, the others in grey.
    Bass,
}

impl WaveColour {
    pub const ALL: [Self; 4] = [Self::Deck, Self::Bands, Self::Mono, Self::Bass];

    pub fn name(self) -> &'static str {
        match self {
            Self::Deck => "deck",
            Self::Bands => "bands",
            Self::Mono => "mono",
            Self::Bass => "bass",
        }
    }
}
pub const EFFECT_UNITS: &[&str] = &["two", "four"];

/// How many effect units the effect row shows: `docs/behavior/effects.md`, *Units and modes*.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EffectUnits {
    /// Units 1 and 2.
    #[default]
    Two,
    /// Units 1 and 3 in the left panel, 2 and 4 in the right one, one at a time.
    Four,
}

impl EffectUnits {
    pub const ALL: [Self; 2] = [Self::Two, Self::Four];

    pub fn name(self) -> &'static str {
        match self {
            Self::Two => "two",
            Self::Four => "four",
        }
    }

    /// The units a channel can be assigned to from its keys.
    pub fn units(self) -> &'static [FxUnit] {
        match self {
            Self::Two => &FX_UNITS_TWO,
            Self::Four => &FxUnit::ALL,
        }
    }
}

const FX_UNITS_TWO: [FxUnit; 2] = [FxUnit::ALL[0], FxUnit::ALL[1]];

/// The panel of the effect row that shows `unit`: 0 on the left for units 1 and 3, 1 on the
/// right for units 2 and 4.
pub fn fx_panel_of(unit: FxUnit) -> usize {
    unit.index() % 2
}

/// The columns of the browser list that can be shown or hidden: all but the badge of a deck,
/// the cover and the title. `docs/behavior/browser.md`, *Sorting*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListColumn {
    Colour,
    Artist,
    Album,
    Genre,
    Bpm,
    Key,
    Time,
    Rating,
    Comment,
    /// The format of the file, or the state of a file that cannot be played.
    File,
}

impl ListColumn {
    /// In the order the list shows them.
    pub const ALL: [Self; 10] = [
        Self::Colour,
        Self::Artist,
        Self::Album,
        Self::Genre,
        Self::Bpm,
        Self::Key,
        Self::Time,
        Self::Rating,
        Self::Comment,
        Self::File,
    ];

    /// The columns shown until the user chooses others.
    pub const SHOWN_AT_FIRST: [Self; 8] = [
        Self::Colour,
        Self::Artist,
        Self::Album,
        Self::Bpm,
        Self::Key,
        Self::Time,
        Self::Rating,
        Self::File,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Colour => "colour",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Genre => "genre",
            Self::Bpm => "bpm",
            Self::Key => "key",
            Self::Time => "time",
            Self::Rating => "rating",
            Self::Comment => "comment",
            Self::File => "file",
        }
    }

    /// What its header sorts by; `None` for the comment and the format.
    pub fn sort(self) -> Option<SortColumn> {
        match self {
            Self::Colour => Some(SortColumn::Colour),
            Self::Artist => Some(SortColumn::Artist),
            Self::Album => Some(SortColumn::Album),
            Self::Genre => Some(SortColumn::Genre),
            Self::Bpm => Some(SortColumn::Bpm),
            Self::Key => Some(SortColumn::Key),
            Self::Time => Some(SortColumn::Time),
            Self::Rating => Some(SortColumn::Rating),
            Self::Comment | Self::File => None,
        }
    }
}

/// What `browser.playlist.create` makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NewItem {
    Playlist,
    Folder,
    /// A smart playlist without rules, which holds the whole collection until it has some.
    Smart,
}

impl NewItem {
    pub const ALL: [Self; 3] = [Self::Playlist, Self::Folder, Self::Smart];

    pub fn name(self) -> &'static str {
        match self {
            Self::Playlist => "playlist",
            Self::Folder => "folder",
            Self::Smart => "smart",
        }
    }
}

/// The field of a rule of a smart playlist, as the dialog of the rules chooses it:
/// `docs/behavior/library.md`, *Smart playlists*.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleField {
    Title,
    Artist,
    Album,
    Genre,
    Tempo,
    Length,
    Rating,
    Plays,
    Imported,
    Played,
    Colour,
    Key,
    Missing,
    Analysed,
}

impl RuleField {
    pub const ALL: [Self; 14] = [
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::Genre,
        Self::Tempo,
        Self::Length,
        Self::Rating,
        Self::Plays,
        Self::Imported,
        Self::Played,
        Self::Colour,
        Self::Key,
        Self::Missing,
        Self::Analysed,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Artist => "artist",
            Self::Album => "album",
            Self::Genre => "genre",
            Self::Tempo => "tempo",
            Self::Length => "length",
            Self::Rating => "rating",
            Self::Plays => "plays",
            Self::Imported => "imported",
            Self::Played => "played",
            Self::Colour => "colour",
            Self::Key => "key",
            Self::Missing => "missing",
            Self::Analysed => "analysed",
        }
    }

    /// The key of its name in the translations.
    pub fn label(self) -> &'static str {
        match self {
            Self::Title => "rules.field_title",
            Self::Artist => "rules.field_artist",
            Self::Album => "rules.field_album",
            Self::Genre => "rules.field_genre",
            Self::Tempo => "rules.field_tempo",
            Self::Length => "rules.field_length",
            Self::Rating => "rules.field_rating",
            Self::Plays => "rules.field_plays",
            Self::Imported => "rules.field_imported",
            Self::Played => "rules.field_played",
            Self::Colour => "rules.field_colour",
            Self::Key => "rules.field_key",
            Self::Missing => "rules.field_missing",
            Self::Analysed => "rules.field_analysed",
        }
    }

    /// The key of the unit written after a range, in the translations; a count has none.
    pub fn unit(self) -> Option<&'static str> {
        match self {
            Self::Tempo => Some("rules.unit_bpm"),
            Self::Length => Some("rules.unit_seconds"),
            Self::Imported | Self::Played => Some("rules.unit_date"),
            _ => None,
        }
    }

    pub fn of_tag(field: TagField) -> Self {
        match field {
            TagField::Title => Self::Title,
            TagField::Artist => Self::Artist,
            TagField::Album => Self::Album,
            TagField::Genre => Self::Genre,
        }
    }

    pub fn of_range(field: RangeField) -> Self {
        match field {
            RangeField::Tempo => Self::Tempo,
            RangeField::Duration => Self::Length,
            RangeField::Rating => Self::Rating,
            RangeField::PlayCount => Self::Plays,
            RangeField::Imported => Self::Imported,
            RangeField::LastPlayed => Self::Played,
        }
    }

    pub fn tag(self) -> Option<TagField> {
        Some(match self {
            Self::Title => TagField::Title,
            Self::Artist => TagField::Artist,
            Self::Album => TagField::Album,
            Self::Genre => TagField::Genre,
            _ => return None,
        })
    }

    pub fn range(self) -> Option<RangeField> {
        Some(match self {
            Self::Tempo => RangeField::Tempo,
            Self::Length => RangeField::Duration,
            Self::Rating => RangeField::Rating,
            Self::Plays => RangeField::PlayCount,
            Self::Imported => RangeField::Imported,
            Self::Played => RangeField::LastPlayed,
            _ => return None,
        })
    }
}

/// The nodes of the browser tree that are not folders of the explorer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserNode {
    Collection,
    /// The playlist used as the preparation list.
    Preparation,
    /// The history of the session of this run.
    History,
}

impl BrowserNode {
    pub const ALL: [Self; 3] = [Self::Collection, Self::Preparation, Self::History];

    pub fn name(self) -> &'static str {
        match self {
            Self::Collection => "collection",
            Self::Preparation => "preparation",
            Self::History => "history",
        }
    }
}

/// Every command of the application, in the order a list shows them.
pub const APP_SPECS: &[CommandSpec] = &[
    CommandSpec {
        id: "app.settings.open",
        label: "Open preferences",
        params: &[choice("page", PAGES)],
    },
    CommandSpec {
        id: "app.settings.close",
        label: "Close preferences",
        params: &[],
    },
    CommandSpec {
        id: "app.settings.restore_defaults",
        label: "Restore default settings",
        params: &[],
    },
    CommandSpec {
        id: "app.language.set",
        label: "Set language",
        params: &[choice("language", LANGUAGES)],
    },
    CommandSpec {
        id: "app.keymap.open",
        label: "Open the shortcut file",
        params: &[],
    },
    CommandSpec {
        id: "app.fullscreen.toggle",
        label: "Full screen",
        params: &[],
    },
    CommandSpec {
        id: "app.tooltips.toggle",
        label: "Tooltips",
        params: &[],
    },
    CommandSpec {
        id: "app.fullscreen_at_start.toggle",
        label: "Full screen at start",
        params: &[],
    },
    CommandSpec {
        id: "app.deck_size.set",
        label: "Size of the decks",
        params: &[choice("size", DECK_SIZES)],
    },
    CommandSpec {
        id: "app.deck_layout.set",
        label: "Two or four decks",
        params: &[choice("layout", DECK_LAYOUTS)],
    },
    CommandSpec {
        id: "app.wave_colour.set",
        label: "Waveform colour",
        params: &[choice("mode", WAVE_COLOURS)],
    },
    CommandSpec {
        id: "app.effect_units.set",
        label: "Two or four effect units",
        params: &[choice("units", EFFECT_UNITS)],
    },
    CommandSpec {
        id: "app.fx_panel.show",
        label: "Show an effect unit in its panel",
        params: &[number("unit", 1.0, 4.0)],
    },
    CommandSpec {
        id: "app.tempo_range.set",
        label: "Tempo range of every deck",
        params: &[number("percent", 8.0, 50.0)],
    },
    CommandSpec {
        id: "layout.select",
        label: "Choose a layout",
        params: &[number("index", 0.0, 63.0)],
    },
    CommandSpec {
        id: "layout.duplicate",
        label: "Duplicate the layout",
        params: &[],
    },
    CommandSpec {
        id: "layout.rename",
        label: "Rename the layout",
        params: &[text("name")],
    },
    CommandSpec {
        id: "layout.delete",
        label: "Delete the layout",
        params: &[],
    },
    CommandSpec {
        id: "layout.move_up",
        label: "Move the layout up",
        params: &[],
    },
    CommandSpec {
        id: "layout.move_down",
        label: "Move the layout down",
        params: &[],
    },
    CommandSpec {
        id: "layout.zone.toggle",
        label: "Show or hide a zone of the layout",
        params: &[choice("zone", ZONES)],
    },
    CommandSpec {
        id: "recorder.folder.set",
        label: "Folder of the recordings",
        params: &[ParamSpec {
            name: "path",
            kind: ParamKind::Path,
        }],
    },
    CommandSpec {
        id: "recorder.folder.reset",
        label: "Default folder of the recordings",
        params: &[],
    },
    CommandSpec {
        id: "recorder.prefix.set",
        label: "Prefix of the recordings",
        params: &[text("prefix")],
    },
    CommandSpec {
        id: "recorder.split.set",
        label: "Split the recordings at",
        params: &[choice("megabytes", SPLIT_NAMES)],
    },
    CommandSpec {
        id: "recorder.loop_decay.set",
        label: "Overdub decay of the loop recorder",
        params: &[number("percent", 0.0, 100.0)],
    },
    CommandSpec {
        id: "app.min_playtime.set",
        label: "Minimum play time",
        params: &[number("seconds", 5.0, 600.0)],
    },
    CommandSpec {
        id: "preview.load_selected",
        label: "Preview the selected track",
        params: &[],
    },
    CommandSpec {
        id: "midi.controller.add",
        label: "Add a controller",
        params: &[],
    },
    CommandSpec {
        id: "midi.controller.remove",
        label: "Remove a controller",
        params: &[number("index", 0.0, 7.0)],
    },
    CommandSpec {
        id: "midi.controller.select",
        label: "Show a controller",
        params: &[number("index", 0.0, 7.0)],
    },
    CommandSpec {
        id: "midi.input.set",
        label: "Set the input of a controller",
        params: &[
            number("index", 0.0, 7.0),
            ParamSpec {
                name: "input",
                kind: ParamKind::Text,
            },
        ],
    },
    CommandSpec {
        id: "midi.mapping.import",
        label: "Import a mapping",
        params: &[
            number("index", 0.0, 7.0),
            ParamSpec {
                name: "path",
                kind: ParamKind::Path,
            },
        ],
    },
    CommandSpec {
        id: "midi.mapping.export",
        label: "Export a mapping",
        params: &[
            number("index", 0.0, 7.0),
            ParamSpec {
                name: "folder",
                kind: ParamKind::Path,
            },
        ],
    },
    CommandSpec {
        id: "midi.binding.select",
        label: "Select an assignment",
        params: &[number("index", 0.0, 4096.0)],
    },
    CommandSpec {
        id: "midi.binding.add",
        label: "Add an assignment",
        params: &[ParamSpec {
            name: "command",
            kind: ParamKind::Text,
        }],
    },
    CommandSpec {
        id: "midi.binding.duplicate",
        label: "Duplicate the assignment",
        params: &[],
    },
    CommandSpec {
        id: "midi.binding.delete",
        label: "Delete the assignment",
        params: &[],
    },
    CommandSpec {
        id: "midi.learn.toggle",
        label: "Learn",
        params: &[],
    },
    CommandSpec {
        id: "midi.target.set",
        label: "Deck of new assignments",
        params: &[DECK],
    },
    CommandSpec {
        id: "library.music_folder.add",
        label: "Add a music folder",
        params: &[ParamSpec {
            name: "path",
            kind: ParamKind::Path,
        }],
    },
    CommandSpec {
        id: "library.music_folder.remove",
        label: "Remove a music folder",
        params: &[number("index", 0.0, 63.0)],
    },
    CommandSpec {
        id: "library.music_folder.select",
        label: "Show a music folder",
        params: &[number("index", 0.0, 63.0)],
    },
    CommandSpec {
        id: "library.import_at_start.toggle",
        label: "Import the music folders at start",
        params: &[],
    },
    CommandSpec {
        id: "library.check_at_start.toggle",
        label: "Check the collection at start",
        params: &[],
    },
    CommandSpec {
        id: "app.load_stopped_only.toggle",
        label: "Load only into a stopped deck",
        params: &[],
    },
    CommandSpec {
        id: "app.key_lock_threads.toggle",
        label: "Key lock on several cores",
        params: &[],
    },
    CommandSpec {
        id: "app.key_notation.set",
        label: "Key notation",
        params: &[choice("notation", KEY_NOTATIONS)],
    },
    CommandSpec {
        id: "app.analysis_range.set",
        label: "Range of tempos",
        params: &[number("min", 20.0, 400.0), number("max", 20.0, 400.0)],
    },
    CommandSpec {
        id: "library.tag_writing.set",
        label: "Tag writing",
        params: &[choice("mode", TAG_WRITINGS)],
    },
    CommandSpec {
        id: "audio.device.set",
        label: "Set audio device",
        params: &[ParamSpec {
            name: "device",
            kind: ParamKind::Text,
        }],
    },
    CommandSpec {
        id: "input.device.set",
        label: "Set input device",
        params: &[ParamSpec {
            name: "device",
            kind: ParamKind::Text,
        }],
    },
    CommandSpec {
        id: "clock.midi.output.set",
        label: "Set MIDI clock output",
        params: &[ParamSpec {
            name: "port",
            kind: ParamKind::Text,
        }],
    },
    CommandSpec {
        id: "clock.midi.input.set",
        label: "Set MIDI clock input",
        params: &[ParamSpec {
            name: "port",
            kind: ParamKind::Text,
        }],
    },
    CommandSpec {
        id: "clock.midi.offset.set",
        label: "Set MIDI clock offset",
        params: &[number("ms", -50.0, 50.0)],
    },
    CommandSpec {
        id: "audio.sample_rate.set",
        label: "Set sample rate",
        params: &[number("hz", 8_000.0, 384_000.0)],
    },
    CommandSpec {
        id: "audio.buffer.set",
        label: "Set buffer size",
        params: &[number("frames", 16.0, 16_384.0)],
    },
    CommandSpec {
        id: "audio.routing.set",
        label: "Set output routing",
        params: &[
            choice("slot", SLOTS),
            number("output", 0.0, (MAX_OUTPUTS - 1) as f64),
        ],
    },
    CommandSpec {
        id: "audio.dropouts.reset",
        label: "Reset dropout count",
        params: &[],
    },
    CommandSpec {
        id: "browser.navigate",
        label: "Show folder",
        params: &[PATH],
    },
    CommandSpec {
        id: "browser.toggle",
        label: "Open or close folder in the tree",
        params: &[PATH],
    },
    CommandSpec {
        id: "browser.up",
        label: "Show parent folder",
        params: &[],
    },
    CommandSpec {
        id: "browser.select",
        label: "Select row",
        params: &[number("index", 0.0, MAX_ROWS)],
    },
    CommandSpec {
        id: "browser.select.toggle",
        label: "Add row to selection or take it out",
        params: &[number("index", 0.0, MAX_ROWS)],
    },
    CommandSpec {
        id: "browser.select.extend",
        label: "Select rows up to a row",
        params: &[number("index", 0.0, MAX_ROWS)],
    },
    CommandSpec {
        id: "browser.select.all",
        label: "Select all rows",
        params: &[],
    },
    CommandSpec {
        id: "browser.move",
        label: "Move selection",
        params: &[number("delta", -MAX_ROWS, MAX_ROWS)],
    },
    CommandSpec {
        id: "browser.activate",
        label: "Open selected folder",
        params: &[],
    },
    CommandSpec {
        id: "browser.load_selected",
        label: "Load selected track",
        params: &[DECK],
    },
    CommandSpec {
        id: "browser.node.select",
        label: "Show node of the tree",
        params: &[choice("node", NODES)],
    },
    CommandSpec {
        id: "browser.history.select",
        label: "Show history of a session",
        params: &[number("session", 1.0, MAX_NODE_ID)],
    },
    CommandSpec {
        id: "browser.folder.toggle",
        label: "Open or close a folder of playlists",
        params: &[number("folder", 1.0, MAX_NODE_ID)],
    },
    CommandSpec {
        id: "browser.history.toggle",
        label: "Show or hide the earlier histories",
        params: &[],
    },
    CommandSpec {
        id: "browser.history.to_playlist",
        label: "Save a history as a playlist",
        params: &[number("session", 1.0, MAX_NODE_ID)],
    },
    CommandSpec {
        id: "browser.history.export",
        label: "Export a history to a folder",
        params: &[number("session", 1.0, MAX_NODE_ID), PATH],
    },
    CommandSpec {
        id: "browser.history.delete",
        label: "Delete a history",
        params: &[number("session", 1.0, MAX_NODE_ID)],
    },
    CommandSpec {
        id: "browser.playlist.select",
        label: "Show playlist",
        params: &[PLAYLIST],
    },
    CommandSpec {
        id: "browser.playlist.create",
        label: "Create playlist or folder",
        params: &[
            number("folder", 0.0, MAX_NODE_ID),
            choice("item", NEW_ITEMS),
            NAME,
        ],
    },
    CommandSpec {
        id: "browser.playlist.rename",
        label: "Rename playlist or folder",
        params: &[number("item", 1.0, MAX_NODE_ID), NAME],
    },
    CommandSpec {
        id: "browser.playlist.move",
        label: "Move playlist or folder",
        params: &[
            number("item", 1.0, MAX_NODE_ID),
            number("folder", 0.0, MAX_NODE_ID),
        ],
    },
    CommandSpec {
        id: "browser.playlist.add",
        label: "Add track to playlist",
        params: &[PLAYLIST, PATH],
    },
    CommandSpec {
        id: "browser.playlist.delete",
        label: "Delete playlist",
        params: &[PLAYLIST],
    },
    CommandSpec {
        id: "browser.playlist.clear",
        label: "Empty playlist",
        params: &[PLAYLIST],
    },
    CommandSpec {
        id: "browser.playlist.remove_repeats",
        label: "Remove repeated tracks from playlist",
        params: &[PLAYLIST],
    },
    CommandSpec {
        id: "browser.playlist.remove",
        label: "Remove selected track from playlist",
        params: &[],
    },
    CommandSpec {
        id: "browser.smart.rules.set",
        label: "Set the rules of a smart playlist",
        params: &[SMART, text("rules")],
    },
    CommandSpec {
        id: "browser.smart.to_playlist",
        label: "Save a smart playlist as a playlist",
        params: &[SMART],
    },
    CommandSpec {
        id: "browser.smart.duplicate",
        label: "Duplicate a smart playlist",
        params: &[SMART],
    },
    CommandSpec {
        id: "browser.smart.rules.open",
        label: "Edit the rules of a smart playlist",
        params: &[SMART],
    },
    CommandSpec {
        id: "browser.smart.rules.close",
        label: "Close the rules of a smart playlist",
        params: &[],
    },
    CommandSpec {
        id: "browser.smart.rule.add",
        label: "Add a rule",
        params: &[],
    },
    CommandSpec {
        id: "browser.smart.rule.remove",
        label: "Remove a rule",
        params: &[RULE_ROW],
    },
    CommandSpec {
        id: "browser.smart.rule.field",
        label: "Choose the field of a rule",
        params: &[RULE_ROW, choice("field", RULE_FIELDS)],
    },
    CommandSpec {
        id: "browser.smart.rule.choice",
        label: "Choose the value of a rule",
        params: &[RULE_ROW, number("value", 0.0, 23.0)],
    },
    CommandSpec {
        id: "browser.playlist.export",
        label: "Export playlist to a folder",
        params: &[PLAYLIST, PATH],
    },
    CommandSpec {
        id: "browser.playlist.import",
        label: "Import playlists from a folder",
        params: &[number("folder", 0.0, MAX_NODE_ID), PATH],
    },
    CommandSpec {
        id: "browser.collection.import",
        label: "Import collections from a folder",
        params: &[PATH],
    },
    CommandSpec {
        id: "browser.load_selected_track",
        label: "Load the selected track without its stems",
        params: &[DECK],
    },
    CommandSpec {
        id: "browser.stems.attach",
        label: "Attach the stems in a folder to the selected track",
        params: &[PATH],
    },
    CommandSpec {
        id: "browser.stems.remove",
        label: "Remove the stems of the selected track",
        params: &[],
    },
    CommandSpec {
        id: "browser.stems.generate",
        label: "Separate the selected track into stems",
        params: &[],
    },
    CommandSpec {
        id: "browser.stems.cancel",
        label: "Stop the separation into stems",
        params: &[],
    },
    CommandSpec {
        id: "stems.model.install",
        label: "Install the separation model from a folder",
        params: &[PATH],
    },
    CommandSpec {
        id: "browser.preparation.select",
        label: "Use playlist as preparation list",
        params: &[PLAYLIST],
    },
    CommandSpec {
        id: "browser.favourite.set",
        label: "Put playlist in a favourite place",
        params: &[FAVOURITE_PLACE, PLAYLIST],
    },
    CommandSpec {
        id: "browser.favourite.clear",
        label: "Empty a favourite place",
        params: &[FAVOURITE_PLACE],
    },
    CommandSpec {
        id: "browser.preparation.append",
        label: "Append selected track to preparation list",
        params: &[],
    },
    CommandSpec {
        id: "browser.preparation.next",
        label: "Add selected track as next to preparation list",
        params: &[],
    },
    CommandSpec {
        id: "library.maintenance.open",
        label: "Open collection maintenance",
        params: &[],
    },
    CommandSpec {
        id: "library.maintenance.close",
        label: "Close collection maintenance",
        params: &[],
    },
    CommandSpec {
        id: "library.check",
        label: "Check consistency of the collection",
        params: &[],
    },
    CommandSpec {
        id: "library.relocate",
        label: "Relocate missing tracks in a folder",
        params: &[PATH],
    },
    CommandSpec {
        id: "library.missing.ask",
        label: "Ask before removing missing tracks",
        params: &[],
    },
    CommandSpec {
        id: "library.missing.keep",
        label: "Keep missing tracks",
        params: &[],
    },
    CommandSpec {
        id: "library.missing.remove",
        label: "Remove missing tracks from the collection",
        params: &[],
    },
    CommandSpec {
        id: "browser.track.colour.set",
        label: "Set colour of selected track",
        params: &[number("colour", 0.0, 8.0)],
    },
    CommandSpec {
        id: "browser.track.rating.set",
        label: "Set rating of selected track",
        params: &[number("rating", 0.0, 5.0)],
    },
    CommandSpec {
        id: "browser.track.details.open",
        label: "Edit details of selected tracks",
        params: &[],
    },
    CommandSpec {
        id: "browser.track.details.close",
        label: "Close track details",
        params: &[],
    },
    CommandSpec {
        id: "browser.track.details.rating",
        label: "Choose rating in track details",
        params: &[number("rating", 0.0, 5.0)],
    },
    CommandSpec {
        id: "browser.track.cover.import",
        label: "Import cover for selected tracks",
        params: &[PATH],
    },
    CommandSpec {
        id: "browser.track.cover.delete",
        label: "Delete cover of selected tracks",
        params: &[],
    },
    CommandSpec {
        id: "browser.track.details.tick",
        label: "Tick a field in track details",
        params: &[choice("field", DETAIL_FIELDS)],
    },
    CommandSpec {
        id: "browser.track.details.save",
        label: "Save track details",
        params: &[
            text("title"),
            text("artist"),
            text("album"),
            text("genre"),
            text("comment"),
            number("rating", 0.0, 5.0),
            text("fields"),
            number("position", 0.0, MAX_ROWS),
            choice("then", DETAILS_THEN),
        ],
    },
    CommandSpec {
        id: "browser.track.delete",
        label: "Delete selected track from collection",
        params: &[],
    },
    CommandSpec {
        id: "browser.played.reset",
        label: "Reset played state of selected track",
        params: &[],
    },
    CommandSpec {
        id: "browser.search.set",
        label: "Search the list",
        params: &[ParamSpec {
            name: "text",
            kind: ParamKind::Text,
        }],
    },
    CommandSpec {
        id: "browser.search.focus",
        label: "Go to the search field",
        params: &[],
    },
    CommandSpec {
        id: "browser.sort",
        label: "Sort the list by a column",
        params: &[choice("column", SORT_COLUMNS)],
    },
    CommandSpec {
        id: "browser.column.toggle",
        label: "Show or hide a column of the list",
        params: &[choice("column", LIST_COLUMNS)],
    },
    CommandSpec {
        id: "library.import.stop",
        label: "Stop the import",
        params: &[],
    },
    CommandSpec {
        id: "library.import_folder",
        label: "Import folder into the collection",
        params: &[PATH],
    },
    CommandSpec {
        id: "waveform.zoom.in",
        label: "Zoom in",
        params: &[DECK],
    },
    CommandSpec {
        id: "waveform.zoom.out",
        label: "Zoom out",
        params: &[DECK],
    },
    CommandSpec {
        id: "waveform.stem_lanes.toggle",
        label: "Show or hide the stem lanes",
        params: &[DECK],
    },
    CommandSpec {
        id: "remix.page.set",
        label: "Show a page of the remix cells",
        params: &[DECK, number("page", 1.0, 4.0)],
    },
    CommandSpec {
        id: "deck.time_display.toggle",
        label: "Swap remaining and elapsed time",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.notice.dismiss",
        label: "Dismiss deck message",
        params: &[DECK],
    },
    CommandSpec {
        id: "deck.page.set",
        label: "Show deck page",
        params: &[DECK, choice("page", DECK_PAGES)],
    },
    CommandSpec {
        id: "deck.cue.map",
        label: "Map cue to hotcue",
        params: &[DECK],
    },
    CommandSpec {
        id: "mixer.clip.reset",
        label: "Reset clip indicator",
        params: &[choice("meter", METERS)],
    },
];

/// Reads the parameters of one command by the names its specification declares.
struct Reader<'a> {
    spec: &'static CommandSpec,
    args: &'a Args,
}

impl Reader<'_> {
    fn param(&self, param: &'static str) -> Result<&Value, CommandError> {
        self.args.get(param).ok_or(CommandError::MissingParam {
            command: self.spec.id,
            param,
        })
    }

    fn text(&self, param: &'static str) -> Result<&str, CommandError> {
        match self.param(param)? {
            Value::Text(text) => Ok(text),
            Value::Number(_) => Err(CommandError::WrongType {
                command: self.spec.id,
                param,
            }),
        }
    }

    /// A whole number inside the range the specification declares.
    fn integer(&self, param: &'static str) -> Result<i64, CommandError> {
        let Value::Number(value) = self.param(param)? else {
            return Err(CommandError::WrongType {
                command: self.spec.id,
                param,
            });
        };
        let (min, max) = self
            .spec
            .params
            .iter()
            .find_map(|spec| match spec.kind {
                ParamKind::Number { min, max } if spec.name == param => Some((min, max)),
                _ => None,
            })
            .unwrap_or((0.0, 0.0));
        let value = ValueError::check(param, *value, min, max)?;
        Ok(value.round() as i64)
    }

    /// A position in a list, which the bounds of its parameter keep from being negative.
    fn index(&self, param: &'static str) -> Result<usize, CommandError> {
        Ok(usize::try_from(self.integer(param)?).unwrap_or(0))
    }

    /// The name of a playlist or a folder, cut to the longest the library keeps.
    fn name(&self) -> Result<String, CommandError> {
        Ok(self.text("name")?.chars().take(MAX_NAME_CHARS).collect())
    }

    /// A favourite place, which the bounds of its parameter keep within 4 to 8.
    fn place(&self) -> Result<usize, CommandError> {
        Ok(usize::try_from(self.integer("place")?).unwrap_or(0))
    }

    fn playlist(&self) -> Result<NodeId, CommandError> {
        Ok(NodeId::new(self.integer("playlist")?))
    }

    fn deck(&self) -> Result<DeckId, CommandError> {
        Ok(DeckId::parse(self.text("deck")?)?)
    }

    /// One of the choices of `param`, as its position in `choices`.
    fn choice<T: Copy>(
        &self,
        param: &'static str,
        choices: &[T],
        name: impl Fn(T) -> &'static str,
        expected: &'static str,
    ) -> Result<T, CommandError> {
        let text = self.text(param)?;
        choices
            .iter()
            .copied()
            .find(|choice| name(*choice) == text)
            .ok_or(CommandError::Value(ValueError::UnknownChoice {
                name: param,
                expected,
            }))
    }
}

impl AppCommand {
    pub fn spec_of(id: &str) -> Option<&'static CommandSpec> {
        APP_SPECS.iter().find(|spec| spec.id == id)
    }

    /// Builds a command from its identifier and untyped parameters, like
    /// `platter_engine::Command::from_args`.
    pub fn from_args(id: &str, args: &Args) -> Result<Self, CommandError> {
        let spec = Self::spec_of(id).ok_or_else(|| CommandError::UnknownCommand(id.to_owned()))?;
        if let Some(extra) = args
            .keys()
            .find(|name| spec.params.iter().all(|param| param.name != name.as_str()))
        {
            return Err(CommandError::UnknownParam {
                command: spec.id,
                param: extra.clone(),
            });
        }
        let read = Reader { spec, args };
        Ok(match spec.id {
            "app.settings.open" => Self::SettingsOpen {
                page: read.choice(
                    "page",
                    &SettingsPage::ALL,
                    SettingsPage::name,
                    "audio, general, mixer",
                )?,
            },
            "app.settings.close" => Self::SettingsClose,
            "app.settings.restore_defaults" => Self::SettingsRestoreDefaults,
            "app.language.set" => Self::LanguageSet {
                language: read.choice("language", &Language::ALL, Language::code, "en, es")?,
            },
            "app.keymap.open" => Self::KeymapOpen,
            "app.fullscreen.toggle" => Self::FullscreenToggle,
            "app.tooltips.toggle" => Self::TooltipsToggle,
            "app.fullscreen_at_start.toggle" => Self::FullscreenAtStartToggle,
            "app.deck_size.set" => Self::DeckSizeSet {
                size: read.choice("size", &DeckSize::ALL, DeckSize::name, "full, compact")?,
            },
            "app.deck_layout.set" => Self::DeckLayoutSet {
                layout: read.choice("layout", &DeckLayout::ALL, DeckLayout::name, "two, four")?,
            },
            "app.wave_colour.set" => Self::WaveColourSet {
                mode: read.choice(
                    "mode",
                    &WaveColour::ALL,
                    WaveColour::name,
                    "deck, bands, mono, bass",
                )?,
            },
            "app.effect_units.set" => Self::EffectUnitsSet {
                units: read.choice("units", &EffectUnits::ALL, EffectUnits::name, "two, four")?,
            },
            "app.fx_panel.show" => Self::FxPanelShow {
                unit: FxUnit::from_number(read.integer("unit")? as f64)?,
            },
            "app.tempo_range.set" => Self::TempoRangeSet {
                percent: u32::try_from(read.integer("percent")?).unwrap_or(0),
            },
            "library.tag_writing.set" => Self::TagWritingSet {
                mode: read.choice(
                    "mode",
                    &TagWriting::ALL,
                    TagWriting::name,
                    "everything, platter, nothing",
                )?,
            },
            "preview.load_selected" => Self::PreviewLoadSelected,
            "midi.controller.add" => Self::MidiControllerAdd,
            "midi.controller.remove" => Self::MidiControllerRemove {
                index: read.index("index")?,
            },
            "midi.controller.select" => Self::MidiControllerSelect {
                index: read.index("index")?,
            },
            "midi.input.set" => Self::MidiInputSet {
                index: read.index("index")?,
                input: read.text("input")?.to_owned(),
            },
            "midi.mapping.import" => Self::MidiMappingImport {
                index: read.index("index")?,
                path: PathBuf::from(read.text("path")?),
            },
            "midi.mapping.export" => Self::MidiMappingExport {
                index: read.index("index")?,
                folder: PathBuf::from(read.text("folder")?),
            },
            "midi.binding.select" => Self::MidiBindingSelect {
                index: read.index("index")?,
            },
            "midi.binding.add" => Self::MidiBindingAdd {
                command: read.text("command")?.to_owned(),
            },
            "midi.binding.duplicate" => Self::MidiBindingDuplicate,
            "midi.binding.delete" => Self::MidiBindingDelete,
            "midi.learn.toggle" => Self::MidiLearnToggle,
            "midi.target.set" => Self::MidiTargetSet { deck: read.deck()? },
            "app.load_stopped_only.toggle" => Self::LoadStoppedOnlyToggle,
            "app.key_lock_threads.toggle" => Self::KeyLockThreadsToggle,
            "layout.select" => Self::LayoutSelect {
                index: read.index("index")?,
            },
            "layout.duplicate" => Self::LayoutDuplicate,
            "layout.rename" => Self::LayoutRename {
                name: read.text("name")?.to_owned(),
            },
            "layout.delete" => Self::LayoutDelete,
            "layout.move_up" => Self::LayoutMoveUp,
            "layout.move_down" => Self::LayoutMoveDown,
            "layout.zone.toggle" => Self::LayoutZoneToggle {
                zone: read.choice("zone", &Zone::ALL, Zone::name, "effects, mixer, lanes")?,
            },
            "recorder.folder.set" => Self::RecorderFolderSet {
                path: PathBuf::from(read.text("path")?),
            },
            "recorder.folder.reset" => Self::RecorderFolderReset,
            "recorder.prefix.set" => Self::RecorderPrefixSet {
                prefix: read.text("prefix")?.to_owned(),
            },
            "recorder.loop_decay.set" => Self::LoopDecaySet {
                percent: u32::try_from(read.integer("percent")?.clamp(0, 100)).unwrap_or(0),
            },
            "recorder.split.set" => Self::RecorderSplitSet {
                megabytes: read.choice(
                    "megabytes",
                    &RECORDING_SPLITS,
                    split_name,
                    "512, 1024, 2048",
                )?,
            },
            "library.music_folder.add" => Self::MusicFolderAdd {
                path: PathBuf::from(read.text("path")?),
            },
            "library.music_folder.remove" => Self::MusicFolderRemove {
                index: read.index("index")?,
            },
            "library.music_folder.select" => Self::MusicFolderSelect {
                index: read.index("index")?,
            },
            "library.import_at_start.toggle" => Self::ImportAtStartToggle,
            "library.check_at_start.toggle" => Self::CheckAtStartToggle,
            "app.analysis_range.set" => Self::AnalysisRangeSet {
                min: u32::try_from(read.integer("min")?).unwrap_or(0),
                max: u32::try_from(read.integer("max")?).unwrap_or(0),
            },
            "app.key_notation.set" => Self::KeyNotationSet {
                notation: read.choice(
                    "notation",
                    &KeyNotation::ALL,
                    KeyNotation::name,
                    "flats, sharps, open_key",
                )?,
            },
            "app.min_playtime.set" => Self::MinPlaytimeSet {
                seconds: u32::try_from(read.integer("seconds")?).unwrap_or(DEFAULT_PLAYTIME),
            },
            "audio.device.set" => {
                let device = read.text("device")?;
                Self::AudioDeviceSet {
                    device: (!device.is_empty()).then(|| device.to_owned()),
                }
            }
            "input.device.set" => {
                let device = read.text("device")?;
                Self::InputDeviceSet {
                    device: (!device.is_empty()).then(|| device.to_owned()),
                }
            }
            "clock.midi.output.set" => {
                let port = read.text("port")?;
                Self::ClockMidiOutputSet {
                    port: (!port.is_empty()).then(|| port.to_owned()),
                }
            }
            "clock.midi.input.set" => {
                let port = read.text("port")?;
                Self::ClockMidiInputSet {
                    port: (!port.is_empty()).then(|| port.to_owned()),
                }
            }
            "clock.midi.offset.set" => Self::ClockMidiOffsetSet {
                ms: read.integer("ms")? as i32,
            },
            "audio.sample_rate.set" => Self::AudioSampleRateSet {
                hz: read.integer("hz")? as u32,
            },
            "audio.buffer.set" => Self::AudioBufferSet {
                frames: read.integer("frames")? as u32,
            },
            "audio.routing.set" => Self::AudioRoutingSet {
                slot: read.choice(
                    "slot",
                    &RouteSlot::ALL,
                    RouteSlot::name,
                    "master_left, master_right, headphones_left, headphones_right",
                )?,
                output: read.integer("output")? as u16,
            },
            "audio.dropouts.reset" => Self::AudioDropoutsReset,
            other => return Self::view_from_args(other, &read),
        })
    }

    /// The commands of the file list and of what the window shows.
    fn view_from_args(id: &str, read: &Reader<'_>) -> Result<Self, CommandError> {
        Ok(match id {
            "browser.navigate" => Self::BrowserNavigate {
                path: PathBuf::from(read.text("path")?),
            },
            "browser.toggle" => Self::BrowserToggle {
                path: PathBuf::from(read.text("path")?),
            },
            "browser.up" => Self::BrowserUp,
            "browser.select" => Self::BrowserSelect {
                index: read.integer("index")? as usize,
            },
            "browser.select.toggle" => Self::BrowserSelectToggle {
                index: read.index("index")?,
            },
            "browser.select.extend" => Self::BrowserSelectExtend {
                index: read.index("index")?,
            },
            "browser.select.all" => Self::BrowserSelectAll,
            "browser.move" => Self::BrowserMove {
                delta: read.integer("delta")? as i32,
            },
            "browser.activate" => Self::BrowserActivate,
            "browser.load_selected" => Self::BrowserLoadSelected { deck: read.deck()? },
            "browser.node.select" => Self::BrowserNodeSelect {
                node: read.choice("node", &BrowserNode::ALL, BrowserNode::name, "collection")?,
            },
            "browser.history.select" => Self::BrowserHistorySelect {
                session: SessionId::new(read.integer("session")?),
            },
            "browser.folder.toggle" => Self::BrowserFolderToggle {
                folder: NodeId::new(read.integer("folder")?),
            },
            "browser.history.toggle" => Self::BrowserHistoryToggle,
            "browser.history.to_playlist" => Self::BrowserHistoryToPlaylist {
                session: SessionId::new(read.integer("session")?),
            },
            "browser.history.export" => Self::BrowserHistoryExport {
                session: SessionId::new(read.integer("session")?),
                path: PathBuf::from(read.text("path")?),
            },
            "browser.history.delete" => Self::BrowserHistoryDelete {
                session: SessionId::new(read.integer("session")?),
            },
            "browser.playlist.select" => Self::BrowserPlaylistSelect {
                playlist: read.playlist()?,
            },
            "browser.playlist.create" => {
                let folder = read.integer("folder")?;
                Self::BrowserPlaylistCreate {
                    folder: (folder > 0).then(|| NodeId::new(folder)),
                    item: read.choice(
                        "item",
                        &NewItem::ALL,
                        NewItem::name,
                        "playlist, folder, smart",
                    )?,
                    name: read.name()?,
                }
            }
            "browser.playlist.rename" => Self::BrowserPlaylistRename {
                item: NodeId::new(read.integer("item")?),
                name: read.name()?,
            },
            "browser.playlist.move" => {
                let folder = read.integer("folder")?;
                Self::BrowserPlaylistMove {
                    item: NodeId::new(read.integer("item")?),
                    folder: (folder > 0).then(|| NodeId::new(folder)),
                }
            }
            "browser.playlist.add" => Self::BrowserPlaylistAdd {
                playlist: read.playlist()?,
                path: PathBuf::from(read.text("path")?),
            },
            "browser.playlist.delete" => Self::BrowserPlaylistDelete {
                playlist: read.playlist()?,
            },
            "browser.playlist.clear" => Self::BrowserPlaylistClear {
                playlist: read.playlist()?,
            },
            "browser.playlist.remove_repeats" => Self::BrowserPlaylistRemoveRepeats {
                playlist: read.playlist()?,
            },
            "browser.playlist.remove" => Self::BrowserPlaylistRemove,
            "browser.smart.rules.set" => Self::BrowserSmartRulesSet {
                smart: NodeId::new(read.integer("smart")?),
                rules: parse_rules(read.text("rules")?).map_err(|_| {
                    CommandError::Value(ValueError::UnknownChoice {
                        name: "rules",
                        expected: "rules as `field value`, separated by `;`: \
                                   docs/behavior/library.md, Smart playlists",
                    })
                })?,
            },
            "browser.smart.to_playlist" => Self::BrowserSmartToPlaylist {
                smart: NodeId::new(read.integer("smart")?),
            },
            "browser.smart.duplicate" => Self::BrowserSmartDuplicate {
                smart: NodeId::new(read.integer("smart")?),
            },
            "browser.smart.rules.open" => Self::BrowserSmartRulesOpen {
                smart: NodeId::new(read.integer("smart")?),
            },
            "browser.smart.rules.close" => Self::BrowserSmartRulesClose,
            "browser.smart.rule.add" => Self::BrowserSmartRuleAdd,
            "browser.smart.rule.remove" => Self::BrowserSmartRuleRemove {
                row: read.index("row")?,
            },
            "browser.smart.rule.field" => Self::BrowserSmartRuleField {
                row: read.index("row")?,
                field: read.choice(
                    "field",
                    &RuleField::ALL,
                    RuleField::name,
                    "title, artist, album, genre, tempo, length, rating, plays, imported, \
                     played, colour, key, missing, analysed",
                )?,
            },
            "browser.smart.rule.choice" => Self::BrowserSmartRuleChoice {
                row: read.index("row")?,
                value: read.index("value")?,
            },
            "browser.played.reset" => Self::BrowserPlayedReset,
            "browser.track.delete" => Self::BrowserTrackDelete,
            "browser.track.details.open" => Self::BrowserTrackDetailsOpen,
            "browser.track.details.close" => Self::BrowserTrackDetailsClose,
            "browser.track.details.rating" => Self::BrowserTrackDetailsRating {
                rating: u8::try_from(read.integer("rating")?).unwrap_or(0),
            },
            "browser.track.details.save" => Self::BrowserTrackDetailsSave {
                details: TrackDetails {
                    title: read.text("title")?.to_owned(),
                    artist: read.text("artist")?.to_owned(),
                    album: read.text("album")?.to_owned(),
                    genre: read.text("genre")?.to_owned(),
                    comment: read.text("comment")?.to_owned(),
                    rating: u8::try_from(read.integer("rating")?).unwrap_or(0),
                },
                fields: DetailFields::parse(read.text("fields")?).ok_or(CommandError::Value(
                    ValueError::UnknownChoice {
                        name: "fields",
                        expected: "title, artist, album, genre, comment, rating",
                    },
                ))?,
                position: read.index("position")?,
                then: read.choice(
                    "then",
                    &DetailsThen::ALL,
                    DetailsThen::name,
                    "close, previous, next",
                )?,
            },
            "browser.track.cover.import" => Self::BrowserTrackCoverImport {
                path: PathBuf::from(read.text("path")?),
            },
            "browser.track.cover.delete" => Self::BrowserTrackCoverDelete,
            "browser.track.details.tick" => Self::BrowserTrackDetailsTick {
                field: read.choice(
                    "field",
                    &DetailField::ALL,
                    DetailField::name,
                    "title, artist, album, genre, comment, rating",
                )?,
            },
            "browser.track.colour.set" => Self::BrowserTrackColourSet {
                colour: u8::try_from(read.integer("colour")?).unwrap_or(0),
            },
            "browser.track.rating.set" => Self::BrowserTrackRatingSet {
                rating: u8::try_from(read.integer("rating")?).unwrap_or(0),
            },
            "library.maintenance.open" => Self::LibraryMaintenanceOpen,
            "library.maintenance.close" => Self::LibraryMaintenanceClose,
            "library.check" => Self::LibraryCheck,
            "library.relocate" => Self::LibraryRelocate {
                path: PathBuf::from(read.text("path")?),
            },
            "library.missing.remove" => Self::LibraryMissingRemove,
            "library.missing.ask" => Self::LibraryMissingAsk,
            "library.missing.keep" => Self::LibraryMissingKeep,
            "browser.playlist.export" => Self::BrowserPlaylistExport {
                playlist: read.playlist()?,
                path: PathBuf::from(read.text("path")?),
            },
            "browser.playlist.import" => {
                let folder = read.integer("folder")?;
                Self::BrowserPlaylistImport {
                    folder: (folder > 0).then(|| NodeId::new(folder)),
                    path: PathBuf::from(read.text("path")?),
                }
            }
            "browser.collection.import" => Self::BrowserCollectionImport {
                path: PathBuf::from(read.text("path")?),
            },
            "browser.load_selected_track" => Self::BrowserLoadSelectedTrack { deck: read.deck()? },
            "browser.stems.attach" => Self::BrowserStemsAttach {
                path: PathBuf::from(read.text("path")?),
            },
            "browser.stems.remove" => Self::BrowserStemsRemove,
            "browser.stems.generate" => Self::BrowserStemsGenerate,
            "browser.stems.cancel" => Self::BrowserStemsCancel,
            "stems.model.install" => Self::StemsModelInstall {
                path: PathBuf::from(read.text("path")?),
            },
            "browser.preparation.select" => Self::BrowserPreparationSelect {
                playlist: read.playlist()?,
            },
            "browser.preparation.append" => Self::BrowserPreparationAppend,
            "browser.favourite.set" => Self::BrowserFavouriteSet {
                place: read.place()?,
                playlist: read.playlist()?,
            },
            "browser.favourite.clear" => Self::BrowserFavouriteClear {
                place: read.place()?,
            },
            "browser.preparation.next" => Self::BrowserPreparationNext,
            "browser.search.set" => Self::BrowserSearchSet {
                text: read.text("text")?.chars().take(MAX_SEARCH_CHARS).collect(),
            },
            "browser.search.focus" => Self::BrowserSearchFocus,
            "browser.sort" => Self::BrowserSort {
                column: read.choice(
                    "column",
                    &SortColumn::ALL,
                    SortColumn::name,
                    "title, artist, album, bpm, time, key, genre, rating, colour",
                )?,
            },
            "browser.column.toggle" => Self::BrowserColumnToggle {
                column: read.choice(
                    "column",
                    &ListColumn::ALL,
                    ListColumn::name,
                    "colour, artist, album, genre, bpm, key, time, rating, comment, file",
                )?,
            },
            "library.import.stop" => Self::LibraryImportStop,
            "library.import_folder" => Self::LibraryImportFolder {
                path: PathBuf::from(read.text("path")?),
            },
            "waveform.zoom.in" => Self::WaveformZoomIn { deck: read.deck()? },
            "waveform.zoom.out" => Self::WaveformZoomOut { deck: read.deck()? },
            "waveform.stem_lanes.toggle" => Self::WaveformStemLanesToggle { deck: read.deck()? },
            "remix.page.set" => Self::RemixPageSet {
                deck: read.deck()?,
                page: read.index("page")?.saturating_sub(1),
            },
            "deck.time_display.toggle" => Self::DeckTimeDisplayToggle { deck: read.deck()? },
            "deck.notice.dismiss" => Self::DeckNoticeDismiss { deck: read.deck()? },
            "deck.page.set" => Self::DeckPageSet {
                deck: read.deck()?,
                page: read.choice("page", &DeckPage::ALL, DeckPage::name, "move, cue, grid")?,
            },
            "deck.cue.map" => Self::DeckCueMapToggle { deck: read.deck()? },
            "mixer.clip.reset" => Self::MixerClipReset {
                meter: read.choice("meter", &MeterId::ALL, MeterId::name, "a, b, c, d, master")?,
            },
            other => return Err(CommandError::UnknownCommand(other.to_owned())),
        })
    }

    /// The stable identifier of the command.
    pub fn id(&self) -> &'static str {
        match self {
            Self::SettingsOpen { .. } => "app.settings.open",
            Self::SettingsClose => "app.settings.close",
            Self::SettingsRestoreDefaults => "app.settings.restore_defaults",
            Self::LanguageSet { .. } => "app.language.set",
            Self::KeymapOpen => "app.keymap.open",
            Self::FullscreenToggle => "app.fullscreen.toggle",
            Self::TooltipsToggle => "app.tooltips.toggle",
            Self::FullscreenAtStartToggle => "app.fullscreen_at_start.toggle",
            Self::DeckSizeSet { .. } => "app.deck_size.set",
            Self::DeckLayoutSet { .. } => "app.deck_layout.set",
            Self::WaveColourSet { .. } => "app.wave_colour.set",
            Self::EffectUnitsSet { .. } => "app.effect_units.set",
            Self::FxPanelShow { .. } => "app.fx_panel.show",
            Self::TempoRangeSet { .. } => "app.tempo_range.set",
            Self::MinPlaytimeSet { .. } => "app.min_playtime.set",
            Self::KeyNotationSet { .. } => "app.key_notation.set",
            Self::AnalysisRangeSet { .. } => "app.analysis_range.set",
            Self::LoadStoppedOnlyToggle => "app.load_stopped_only.toggle",
            Self::KeyLockThreadsToggle => "app.key_lock_threads.toggle",
            Self::LayoutSelect { .. } => "layout.select",
            Self::LayoutDuplicate => "layout.duplicate",
            Self::LayoutRename { .. } => "layout.rename",
            Self::LayoutDelete => "layout.delete",
            Self::LayoutMoveUp => "layout.move_up",
            Self::LayoutMoveDown => "layout.move_down",
            Self::LayoutZoneToggle { .. } => "layout.zone.toggle",
            Self::RecorderFolderSet { .. } => "recorder.folder.set",
            Self::RecorderFolderReset => "recorder.folder.reset",
            Self::RecorderPrefixSet { .. } => "recorder.prefix.set",
            Self::RecorderSplitSet { .. } => "recorder.split.set",
            Self::LoopDecaySet { .. } => "recorder.loop_decay.set",
            Self::MusicFolderAdd { .. } => "library.music_folder.add",
            Self::MusicFolderRemove { .. } => "library.music_folder.remove",
            Self::MusicFolderSelect { .. } => "library.music_folder.select",
            Self::ImportAtStartToggle => "library.import_at_start.toggle",
            Self::CheckAtStartToggle => "library.check_at_start.toggle",
            Self::PreviewLoadSelected => "preview.load_selected",
            Self::MidiControllerAdd => "midi.controller.add",
            Self::MidiControllerRemove { .. } => "midi.controller.remove",
            Self::MidiControllerSelect { .. } => "midi.controller.select",
            Self::MidiInputSet { .. } => "midi.input.set",
            Self::MidiMappingImport { .. } => "midi.mapping.import",
            Self::MidiMappingExport { .. } => "midi.mapping.export",
            Self::MidiBindingSelect { .. } => "midi.binding.select",
            Self::MidiBindingAdd { .. } => "midi.binding.add",
            Self::MidiBindingDuplicate => "midi.binding.duplicate",
            Self::MidiBindingDelete => "midi.binding.delete",
            Self::MidiLearnToggle => "midi.learn.toggle",
            Self::MidiTargetSet { .. } => "midi.target.set",
            Self::TagWritingSet { .. } => "library.tag_writing.set",
            Self::AudioDeviceSet { .. } => "audio.device.set",
            Self::InputDeviceSet { .. } => "input.device.set",
            Self::ClockMidiOutputSet { .. } => "clock.midi.output.set",
            Self::ClockMidiOffsetSet { .. } => "clock.midi.offset.set",
            Self::ClockMidiInputSet { .. } => "clock.midi.input.set",
            Self::AudioSampleRateSet { .. } => "audio.sample_rate.set",
            Self::AudioBufferSet { .. } => "audio.buffer.set",
            Self::AudioRoutingSet { .. } => "audio.routing.set",
            Self::AudioDropoutsReset => "audio.dropouts.reset",
            Self::BrowserNavigate { .. } => "browser.navigate",
            Self::BrowserToggle { .. } => "browser.toggle",
            Self::BrowserUp => "browser.up",
            Self::BrowserSelect { .. } => "browser.select",
            Self::BrowserSelectToggle { .. } => "browser.select.toggle",
            Self::BrowserSelectExtend { .. } => "browser.select.extend",
            Self::BrowserSelectAll => "browser.select.all",
            Self::BrowserMove { .. } => "browser.move",
            Self::BrowserActivate => "browser.activate",
            Self::BrowserLoadSelected { .. } => "browser.load_selected",
            Self::BrowserNodeSelect { .. } => "browser.node.select",
            Self::BrowserHistorySelect { .. } => "browser.history.select",
            Self::BrowserFolderToggle { .. } => "browser.folder.toggle",
            Self::BrowserHistoryToggle => "browser.history.toggle",
            Self::BrowserHistoryToPlaylist { .. } => "browser.history.to_playlist",
            Self::BrowserHistoryExport { .. } => "browser.history.export",
            Self::BrowserHistoryDelete { .. } => "browser.history.delete",
            Self::BrowserPlaylistSelect { .. } => "browser.playlist.select",
            Self::BrowserPlaylistCreate { .. } => "browser.playlist.create",
            Self::BrowserPlaylistRename { .. } => "browser.playlist.rename",
            Self::BrowserPlaylistMove { .. } => "browser.playlist.move",
            Self::BrowserPlaylistAdd { .. } => "browser.playlist.add",
            Self::BrowserPlaylistDelete { .. } => "browser.playlist.delete",
            Self::BrowserPlaylistClear { .. } => "browser.playlist.clear",
            Self::BrowserPlaylistRemoveRepeats { .. } => "browser.playlist.remove_repeats",
            Self::BrowserPlaylistRemove => "browser.playlist.remove",
            Self::BrowserSmartRulesSet { .. } => "browser.smart.rules.set",
            Self::BrowserSmartToPlaylist { .. } => "browser.smart.to_playlist",
            Self::BrowserSmartDuplicate { .. } => "browser.smart.duplicate",
            Self::BrowserSmartRulesOpen { .. } => "browser.smart.rules.open",
            Self::BrowserSmartRulesClose => "browser.smart.rules.close",
            Self::BrowserSmartRuleAdd => "browser.smart.rule.add",
            Self::BrowserSmartRuleRemove { .. } => "browser.smart.rule.remove",
            Self::BrowserSmartRuleField { .. } => "browser.smart.rule.field",
            Self::BrowserSmartRuleChoice { .. } => "browser.smart.rule.choice",
            Self::BrowserPlayedReset => "browser.played.reset",
            Self::BrowserTrackDelete => "browser.track.delete",
            Self::BrowserTrackDetailsOpen => "browser.track.details.open",
            Self::BrowserTrackDetailsClose => "browser.track.details.close",
            Self::BrowserTrackDetailsRating { .. } => "browser.track.details.rating",
            Self::BrowserTrackDetailsSave { .. } => "browser.track.details.save",
            Self::BrowserTrackDetailsTick { .. } => "browser.track.details.tick",
            Self::BrowserTrackCoverImport { .. } => "browser.track.cover.import",
            Self::BrowserTrackCoverDelete => "browser.track.cover.delete",
            Self::BrowserTrackColourSet { .. } => "browser.track.colour.set",
            Self::BrowserTrackRatingSet { .. } => "browser.track.rating.set",
            Self::LibraryMaintenanceOpen => "library.maintenance.open",
            Self::LibraryMaintenanceClose => "library.maintenance.close",
            Self::LibraryCheck => "library.check",
            Self::LibraryRelocate { .. } => "library.relocate",
            Self::LibraryMissingRemove => "library.missing.remove",
            Self::LibraryMissingAsk => "library.missing.ask",
            Self::LibraryMissingKeep => "library.missing.keep",
            Self::BrowserPlaylistExport { .. } => "browser.playlist.export",
            Self::BrowserPlaylistImport { .. } => "browser.playlist.import",
            Self::BrowserCollectionImport { .. } => "browser.collection.import",
            Self::BrowserLoadSelectedTrack { .. } => "browser.load_selected_track",
            Self::BrowserStemsAttach { .. } => "browser.stems.attach",
            Self::BrowserStemsRemove => "browser.stems.remove",
            Self::BrowserStemsGenerate => "browser.stems.generate",
            Self::BrowserStemsCancel => "browser.stems.cancel",
            Self::StemsModelInstall { .. } => "stems.model.install",
            Self::BrowserPreparationSelect { .. } => "browser.preparation.select",
            Self::BrowserPreparationAppend => "browser.preparation.append",
            Self::BrowserFavouriteSet { .. } => "browser.favourite.set",
            Self::BrowserFavouriteClear { .. } => "browser.favourite.clear",
            Self::BrowserPreparationNext => "browser.preparation.next",
            Self::BrowserSearchSet { .. } => "browser.search.set",
            Self::BrowserSearchFocus => "browser.search.focus",
            Self::BrowserSort { .. } => "browser.sort",
            Self::BrowserColumnToggle { .. } => "browser.column.toggle",
            Self::LibraryImportFolder { .. } => "library.import_folder",
            Self::LibraryImportStop => "library.import.stop",
            Self::WaveformZoomIn { .. } => "waveform.zoom.in",
            Self::WaveformZoomOut { .. } => "waveform.zoom.out",
            Self::WaveformStemLanesToggle { .. } => "waveform.stem_lanes.toggle",
            Self::RemixPageSet { .. } => "remix.page.set",
            Self::DeckTimeDisplayToggle { .. } => "deck.time_display.toggle",
            Self::DeckNoticeDismiss { .. } => "deck.notice.dismiss",
            Self::DeckPageSet { .. } => "deck.page.set",
            Self::DeckCueMapToggle { .. } => "deck.cue.map",
            Self::MixerClipReset { .. } => "mixer.clip.reset",
        }
    }
}

/// Something a control or a shortcut asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Engine(Command),
    App(AppCommand),
}

impl From<Command> for Action {
    fn from(command: Command) -> Self {
        Self::Engine(command)
    }
}

impl From<AppCommand> for Action {
    fn from(command: AppCommand) -> Self {
        Self::App(command)
    }
}

impl Action {
    pub fn id(&self) -> &'static str {
        match self {
            Self::Engine(command) => command.id(),
            Self::App(command) => command.id(),
        }
    }

    /// The specification of a command of either kind.
    pub fn spec_of(id: &str) -> Option<&'static CommandSpec> {
        Command::spec_of(id).or_else(|| AppCommand::spec_of(id))
    }

    /// Every command of the engine and of the application.
    pub fn specs() -> impl Iterator<Item = &'static CommandSpec> {
        SPECS.iter().chain(APP_SPECS)
    }

    /// Builds an action of either kind from an identifier and untyped parameters.
    pub fn from_args(id: &str, args: &Args) -> Result<Self, CommandError> {
        if Command::spec_of(id).is_some() {
            Command::from_args(id, args).map(Self::Engine)
        } else {
            AppCommand::from_args(id, args).map(Self::App)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use platter_engine::command::{Args, CommandError, CommandSpec, ParamKind, Value};
    use platter_library::{NodeId, SessionId};

    use platter_core::{DeckId, ValueError};

    use super::{
        APP_SPECS, Action, AppCommand, BrowserNode, DECK_LAYOUTS, DETAIL_FIELDS, DeckLayout,
        DetailField, DetailFields, NEW_ITEMS, NODES, NewItem, PAGES, RECORDING_SPLITS, RULE_FIELDS,
        RuleField, SORT_COLUMNS, SPLIT_NAMES, SettingsPage, SortColumn, WAVE_COLOURS, WaveColour,
        split_name,
    };

    /// Valid arguments for a specification: the first choice, the lower bound, deck A.
    fn sample_args(spec: &CommandSpec) -> Args {
        spec.params
            .iter()
            .map(|param| {
                let value = match param.kind {
                    ParamKind::Deck => Value::Text("a".to_owned()),
                    ParamKind::Number { min, .. } => Value::Number(min),
                    ParamKind::Choice(choices) => Value::Text(choices[0].to_owned()),
                    ParamKind::Path => Value::Text("music".to_owned()),
                    // The fields of the details are a text of known names.
                    ParamKind::Text if param.name == "fields" => Value::Text("title".to_owned()),
                    // The rules of a smart playlist are a text of known fields.
                    ParamKind::Text if param.name == "rules" => {
                        Value::Text("genre house; tempo 120..".to_owned())
                    }
                    ParamKind::Text => Value::Text("device".to_owned()),
                };
                (param.name.to_owned(), value)
            })
            .collect()
    }

    /// `docs/behavior/library.md`, *Smart playlists*.
    #[test]
    fn the_rules_of_a_smart_playlist_are_read_from_text_and_a_mistake_is_refused() {
        let args = |rules: &str| {
            Args::from([
                ("smart".to_owned(), Value::Number(7.0)),
                ("rules".to_owned(), Value::Text(rules.to_owned())),
            ])
        };
        assert_eq!(
            Action::from_args("browser.smart.rules.set", &args("colour 2; missing no")),
            Ok(Action::App(AppCommand::BrowserSmartRulesSet {
                smart: NodeId::new(7),
                rules: vec![
                    platter_library::Rule::Colour(2),
                    platter_library::Rule::Missing(false)
                ],
            }))
        );
        assert!(matches!(
            Action::from_args("browser.smart.rules.set", &args("colour pink")),
            Err(CommandError::Value(ValueError::UnknownChoice {
                name: "rules",
                ..
            }))
        ));
    }

    /// Behaviour note: `docs/behavior/browser.md`, *Track details*, edge cases.
    #[test]
    fn the_fields_of_the_details_are_named_and_an_unknown_name_is_refused() {
        let fields = DetailFields::parse(" genre, rating,,").unwrap();
        assert!(fields.contains(DetailField::Genre) && fields.contains(DetailField::Rating));
        assert!(!fields.contains(DetailField::Title));
        assert_eq!(DetailFields::parse(""), Some(DetailFields::NONE));
        let all = DETAIL_FIELDS.join(",");
        assert_eq!(DetailFields::parse(&all), Some(DetailFields::ALL));
        let mut args = sample_args(
            APP_SPECS
                .iter()
                .find(|spec| spec.id == "browser.track.details.save")
                .unwrap(),
        );
        args.insert("fields".to_owned(), Value::Text("title,year".to_owned()));
        assert!(matches!(
            Action::from_args("browser.track.details.save", &args),
            Err(CommandError::Value(ValueError::UnknownChoice {
                name: "fields",
                ..
            }))
        ));
    }

    #[test]
    fn identifiers_are_unique_across_the_engine_and_the_application() {
        let ids: BTreeSet<&str> = Action::specs().map(|spec| spec.id).collect();
        assert_eq!(ids.len(), Action::specs().count());
        for spec in APP_SPECS {
            assert!(
                spec.id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "{}",
                spec.id
            );
            assert!(!spec.label.is_empty());
        }
    }

    #[test]
    fn the_choices_a_specification_lists_name_every_value() {
        let pages: Vec<_> = SettingsPage::ALL
            .into_iter()
            .map(SettingsPage::name)
            .collect();
        assert_eq!(pages, PAGES);
        let nodes: Vec<_> = BrowserNode::ALL
            .into_iter()
            .map(BrowserNode::name)
            .collect();
        assert_eq!(nodes, NODES);
        let columns: Vec<_> = SortColumn::ALL.into_iter().map(SortColumn::name).collect();
        assert_eq!(columns, SORT_COLUMNS);
        let items: Vec<_> = NewItem::ALL.into_iter().map(NewItem::name).collect();
        assert_eq!(items, NEW_ITEMS);
        let fields: Vec<_> = RuleField::ALL.into_iter().map(RuleField::name).collect();
        assert_eq!(fields, RULE_FIELDS);
        let layouts: Vec<_> = DeckLayout::ALL.into_iter().map(DeckLayout::name).collect();
        assert_eq!(layouts, DECK_LAYOUTS);
        let modes: Vec<_> = WaveColour::ALL.into_iter().map(WaveColour::name).collect();
        assert_eq!(modes, WAVE_COLOURS);
        let splits: Vec<_> = RECORDING_SPLITS.into_iter().map(split_name).collect();
        assert_eq!(splits, SPLIT_NAMES);
    }

    /// `docs/behavior/interface.md`, *Deck layout*, edge cases.
    #[test]
    fn the_deck_layout_takes_two_or_four() {
        let args =
            |layout: &str| Args::from([("layout".to_owned(), Value::Text(layout.to_owned()))]);
        assert_eq!(
            Action::from_args("app.deck_layout.set", &args("four")),
            Ok(Action::App(AppCommand::DeckLayoutSet {
                layout: DeckLayout::Four
            }))
        );
        assert!(matches!(
            Action::from_args("app.deck_layout.set", &args("three")),
            Err(CommandError::Value(ValueError::UnknownChoice {
                name: "layout",
                ..
            }))
        ));
        assert_eq!(DeckLayout::Four.decks(), DeckId::ALL);
        assert_eq!(DeckLayout::Two.decks(), [DeckId::A, DeckId::B]);
    }

    #[test]
    fn every_specification_builds_its_command_and_needs_every_parameter() {
        for spec in Action::specs() {
            let args = sample_args(spec);
            let action = Action::from_args(spec.id, &args).unwrap();
            assert_eq!(action.id(), spec.id);
            for param in spec.params {
                let mut missing = args.clone();
                missing.remove(param.name);
                assert_eq!(
                    Action::from_args(spec.id, &missing),
                    Err(CommandError::MissingParam {
                        command: spec.id,
                        param: param.name
                    })
                );
            }
        }
    }

    #[test]
    fn values_outside_what_a_command_accepts_are_rejected() {
        let mut args = Args::new();
        args.insert("hz".to_owned(), Value::Number(12.0));
        assert!(matches!(
            AppCommand::from_args("audio.sample_rate.set", &args),
            Err(CommandError::Value(_))
        ));
        args.clear();
        args.insert("page".to_owned(), Value::Text("network".to_owned()));
        assert!(matches!(
            AppCommand::from_args("app.settings.open", &args),
            Err(CommandError::Value(_))
        ));
        args.insert("extra".to_owned(), Value::Number(1.0));
        assert!(matches!(
            AppCommand::from_args("app.settings.open", &args),
            Err(CommandError::UnknownParam { .. })
        ));
        assert!(matches!(
            Action::from_args("app.explode", &Args::new()),
            Err(CommandError::UnknownCommand(_))
        ));
    }

    #[test]
    fn a_session_is_named_by_a_whole_number_from_one() {
        let with = |value| {
            let mut args = Args::new();
            args.insert("session".to_owned(), value);
            AppCommand::from_args("browser.history.select", &args)
        };
        assert_eq!(
            with(Value::Number(7.0)),
            Ok(AppCommand::BrowserHistorySelect {
                session: SessionId::new(7)
            })
        );
        assert!(matches!(
            with(Value::Number(0.0)),
            Err(CommandError::Value(_))
        ));
        assert!(matches!(
            with(Value::Text("7".to_owned())),
            Err(CommandError::WrongType { .. })
        ));
    }

    #[test]
    fn a_playlist_is_named_by_a_whole_number_from_one() {
        let with = |value| {
            let mut args = Args::new();
            args.insert("playlist".to_owned(), value);
            AppCommand::from_args("browser.playlist.select", &args)
        };
        assert_eq!(
            with(Value::Number(17.0)),
            Ok(AppCommand::BrowserPlaylistSelect {
                playlist: NodeId::new(17)
            })
        );
        assert!(matches!(
            with(Value::Number(0.0)),
            Err(CommandError::Value(_))
        ));
        assert!(matches!(
            with(Value::Text("17".to_owned())),
            Err(CommandError::WrongType { .. })
        ));
    }

    #[test]
    fn a_new_item_goes_to_the_top_with_folder_zero_and_its_name_is_cut() {
        let mut args = Args::new();
        args.insert("folder".to_owned(), Value::Number(0.0));
        args.insert("item".to_owned(), Value::Text("folder".to_owned()));
        args.insert("name".to_owned(), Value::Text("x".repeat(300)));
        assert_eq!(
            AppCommand::from_args("browser.playlist.create", &args),
            Ok(AppCommand::BrowserPlaylistCreate {
                folder: None,
                item: NewItem::Folder,
                name: "x".repeat(200),
            })
        );
        args.insert("folder".to_owned(), Value::Number(7.0));
        assert!(matches!(
            AppCommand::from_args("browser.playlist.create", &args),
            Ok(AppCommand::BrowserPlaylistCreate {
                folder: Some(folder),
                ..
            }) if folder == NodeId::new(7)
        ));
    }

    #[test]
    fn an_empty_device_name_is_the_default_device() {
        let mut args = Args::new();
        args.insert("device".to_owned(), Value::Text(String::new()));
        assert_eq!(
            AppCommand::from_args("audio.device.set", &args),
            Ok(AppCommand::AudioDeviceSet { device: None })
        );
    }
}
