//! The context menus of the playlists, drawn without a window.
//!
//! Behaviour note: `docs/behavior/browser.md`, *Playlists*, *Preparation list*, *Favourites*,
//! *Deleting tracks*, *Playlist files*, *Colour and rating* and *Track details*.

#![allow(clippy::unwrap_used)]

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use platter_library::NodeId;
use platter_ui_egui::collection::TagWriting;
use platter_ui_egui::command::{
    Action, AppCommand, BrowserNode, DetailField, DetailFields, DetailsThen,
};
use platter_ui_egui::cx::ControlKind;
use platter_ui_egui::files::MAX_PICTURES;
use platter_ui_egui::i18n::Language;
use platter_ui_egui::sample::{self, Scene};
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

/// The playlist *Friday warm-up* of the sample scenes.
const FRIDAY: i64 = 1;

/// The window of these tests: the designed width, and the height that gives the browser of the
/// two-deck layout the room it had before the effect row took 74 points above the lanes. The
/// tests reach rows of the tree and of the list that a shorter browser scrolls out of view.
const WINDOW_HEIGHT: f32 = DESIGN_HEIGHT + 74.0;

struct Stage {
    ctx: egui::Context,
    shell: Shell,
    scene: Scene,
}

impl Stage {
    fn new(scene: Scene) -> Self {
        let shell = Shell::new(Language::English).unwrap();
        let ctx = egui::Context::default();
        shell.install(&ctx);
        let mut stage = Self { ctx, shell, scene };
        stage.frame(Vec::new());
        stage
    }

    fn frame(&mut self, events: Vec<Event>) -> Vec<Action> {
        self.frame_with(events, Modifiers::NONE)
    }

    /// A frame with `modifiers` held, as the keyboard state egui reads them from.
    fn frame_with(&mut self, events: Vec<Event>, modifiers: Modifiers) -> Vec<Action> {
        let events = std::iter::once(Event::ModifiersChanged(modifiers))
            .chain(events)
            .collect();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(DESIGN_WIDTH, WINDOW_HEIGHT),
            )),
            events,
            focused: true,
            ..RawInput::default()
        };
        let view = self.scene.view();
        let shell = &mut self.shell;
        let mut actions = Vec::new();
        let mut output = self.ctx.run_ui(input, |ui| {
            actions = shell.show(ui, &view);
        });
        output.textures_delta.clear();
        actions
    }

    /// The middles of the controls of `command` that the pointer can operate, in the order
    /// they were drawn.
    fn controls(&self, command: &str) -> Vec<Pos2> {
        self.shell
            .controls()
            .iter()
            .filter(|control| control.command == command && control.kind != ControlKind::Keys)
            .map(|control| control.rect.center())
            .collect()
    }

    fn control(&self, command: &str) -> Option<Pos2> {
        self.controls(command).first().copied()
    }

    /// The middles of the controls that send `command`, in the order they were drawn: the rows
    /// of the tree, or the chips of the favourites above it, which are narrower.
    fn placed(&self, command: &str, chips: bool) -> Vec<Pos2> {
        self.shell
            .controls()
            .iter()
            .filter(|control| {
                control.command == command
                    && control.kind != ControlKind::Keys
                    && (control.rect.width() <= 120.0) == chips
            })
            .map(|control| control.rect.center())
            .collect()
    }

    fn tree_rows(&self, command: &str) -> Vec<Pos2> {
        self.placed(command, false)
    }

    fn chips(&self, command: &str) -> Vec<Pos2> {
        self.placed(command, true)
    }

    /// The row of *Friday warm-up* in the tree, the second playlist after *Summer terrace*.
    fn friday_row(&self) -> Pos2 {
        self.tree_rows("browser.playlist.select")[1]
    }

    fn click(&mut self, at: Pos2, button: PointerButton) -> Vec<Action> {
        let press = |pressed| Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut actions = self.frame(vec![Event::PointerMoved(at), press(true)]);
        actions.extend(self.frame(vec![press(false)]));
        actions.extend(self.frame(Vec::new()));
        actions
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) -> Vec<Action> {
        let mut actions = self.frame_with(
            vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            modifiers,
        );
        actions.extend(self.frame(Vec::new()));
        actions
    }

    fn menu_open(&self) -> bool {
        self.shell.state.browser.menu.is_some()
    }
}

fn friday() -> NodeId {
    NodeId::new(FRIDAY)
}

fn app(actions: &[Action]) -> Vec<AppCommand> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::App(command) => Some(command.clone()),
            Action::Engine(_) => None,
        })
        .collect()
}

#[test]
fn the_right_button_on_a_playlist_opens_its_menu_and_an_item_acts_on_it() {
    let mut stage = Stage::new(sample::playing());
    let row = stage.friday_row();
    let opened = stage.click(row, PointerButton::Secondary);
    // The right button does not show the playlist.
    assert!(app(&opened).is_empty(), "{opened:?}");
    assert!(stage.menu_open());
    let delete = stage.control("browser.playlist.delete").unwrap();
    assert!(stage.control("browser.playlist.clear").is_some());
    assert!(stage.control("browser.playlist.remove_repeats").is_some());
    let actions = stage.click(delete, PointerButton::Primary);
    assert_eq!(
        app(&actions),
        [AppCommand::BrowserPlaylistDelete { playlist: friday() }]
    );
    assert!(!stage.menu_open());
}

#[test]
fn escape_or_a_press_outside_closes_the_menu_without_acting() {
    let mut stage = Stage::new(sample::playing());
    let row = stage.friday_row();
    stage.click(row, PointerButton::Secondary);
    assert!(stage.menu_open());
    let closed = stage.key(Key::Escape, Modifiers::NONE);
    assert!(!stage.menu_open());
    assert!(app(&closed).is_empty(), "{closed:?}");

    stage.click(row, PointerButton::Secondary);
    assert!(stage.menu_open());
    // A press on the search field closes the menu and still reaches the field.
    let field = stage
        .shell
        .controls()
        .iter()
        .find(|control| control.command == "browser.search.set")
        .unwrap()
        .rect
        .center();
    let actions = stage.click(field, PointerButton::Primary);
    assert!(!stage.menu_open());
    assert!(
        app(&actions)
            .iter()
            .all(|command| !matches!(command, AppCommand::BrowserPlaylistDelete { .. })),
        "{actions:?}"
    );
}

#[test]
fn shift_f10_opens_the_menu_of_the_focused_playlist_and_the_arrows_pick_an_item() {
    let mut stage = Stage::new(sample::playing());
    let row = stage.friday_row();
    // A click shows the playlist and gives its row the keyboard.
    stage.click(row, PointerButton::Primary);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    stage.key(Key::F10, shift);
    assert!(stage.menu_open());
    // Down past Rename, Move to and Empty, and Enter: the repeats go; the arrows do not move
    // the list.
    let mut moved = Vec::new();
    for _ in 0..3 {
        moved.extend(stage.key(Key::ArrowDown, Modifiers::NONE));
    }
    assert!(app(&moved).is_empty(), "{moved:?}");
    let picked = stage.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        app(&picked),
        [AppCommand::BrowserPlaylistRemoveRepeats { playlist: friday() }]
    );
    assert!(!stage.menu_open());
}

#[test]
fn the_right_button_on_a_row_of_a_playlist_selects_it_and_offers_to_remove_it() {
    let mut stage = Stage::new(sample::playlist());
    let list = stage.control("browser.select").unwrap();
    let opened = stage.click(list, PointerButton::Secondary);
    assert!(
        app(&opened)
            .iter()
            .any(|command| matches!(command, AppCommand::BrowserSelect { .. })),
        "{opened:?}"
    );
    assert!(stage.menu_open());
    let remove = stage.control("browser.playlist.remove").unwrap();
    let actions = stage.click(remove, PointerButton::Primary);
    assert_eq!(app(&actions), [AppCommand::BrowserPlaylistRemove]);
}

#[test]
fn a_row_of_a_folder_has_no_menu() {
    let mut stage = Stage::new(sample::playing());
    let list = stage.control("browser.select").unwrap();
    stage.click(list, PointerButton::Secondary);
    assert!(!stage.menu_open());
}

#[test]
fn delete_takes_the_selected_track_out_of_the_playlist() {
    let mut stage = Stage::new(sample::playlist());
    let actions = stage.key(Key::Delete, Modifiers::NONE);
    assert!(
        app(&actions).contains(&AppCommand::BrowserPlaylistRemove),
        "{actions:?}"
    );
}

impl Stage {
    fn typed(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_owned())]);
    }

    fn naming(&self) -> Option<platter_ui_egui::Naming> {
        self.shell.state.browser.naming
    }

    /// Opens the menu of the heading *Playlists* from the keyboard: a click gives its row the
    /// keyboard, and Shift+F10 opens the menu.
    fn heading_menu(&mut self) {
        let heading = self.playlists_heading();
        self.click(heading, PointerButton::Primary);
        let shift = Modifiers {
            shift: true,
            ..Modifiers::NONE
        };
        self.key(Key::F10, shift);
        assert!(self.menu_open());
    }

    /// The heading *Playlists* sits right below *Collection* in the tree.
    fn playlists_heading(&self) -> Pos2 {
        self.tree_rows("browser.node.select")[0] + egui::vec2(0.0, 27.0)
    }
}

#[test]
fn a_new_playlist_is_named_in_the_tree_and_created_with_enter() {
    let mut stage = Stage::new(sample::playing());
    stage.heading_menu();
    // New playlist is the first item.
    stage.key(Key::Enter, Modifiers::NONE);
    assert!(!stage.menu_open());
    assert_eq!(
        stage.naming(),
        Some(platter_ui_egui::Naming::New {
            folder: None,
            item: platter_ui_egui::command::NewItem::Playlist,
        })
    );
    stage.frame(Vec::new());
    // Typing in the field plays no shortcut: Z would play deck A.
    stage.typed("Z set");
    let actions = stage.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        app(&actions),
        [AppCommand::BrowserPlaylistCreate {
            folder: None,
            item: platter_ui_egui::command::NewItem::Playlist,
            name: "Z set".to_owned(),
        }]
    );
    assert!(
        actions
            .iter()
            .all(|action| matches!(action, Action::App(_)))
    );
    assert_eq!(stage.naming(), None);
}

#[test]
fn escape_or_an_empty_name_creates_nothing() {
    let mut stage = Stage::new(sample::playing());
    stage.heading_menu();
    // The second item makes a folder.
    stage.key(Key::ArrowDown, Modifiers::NONE);
    stage.key(Key::Enter, Modifiers::NONE);
    stage.frame(Vec::new());
    stage.typed("Old sets");
    let escaped = stage.key(Key::Escape, Modifiers::NONE);
    assert!(app(&escaped).is_empty(), "{escaped:?}");
    assert_eq!(stage.naming(), None);

    stage.heading_menu();
    stage.key(Key::Enter, Modifiers::NONE);
    stage.frame(Vec::new());
    let empty = stage.key(Key::Enter, Modifiers::NONE);
    assert!(app(&empty).is_empty(), "{empty:?}");
    assert_eq!(stage.naming(), None);
}

#[test]
fn rename_starts_from_the_name_of_the_playlist() {
    let mut stage = Stage::new(sample::playing());
    let row = stage.friday_row();
    stage.click(row, PointerButton::Secondary);
    // Rename is the first item of the menu of a playlist.
    stage.key(Key::Enter, Modifiers::NONE);
    stage.frame(Vec::new());
    for _ in 0.."warm-up".len() {
        stage.key(Key::Backspace, Modifiers::NONE);
    }
    stage.typed("night");
    let actions = stage.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        app(&actions),
        [AppCommand::BrowserPlaylistRename {
            item: friday(),
            name: "Friday night".to_owned(),
        }]
    );
}

#[test]
fn a_press_elsewhere_closes_the_name_field_without_creating() {
    let mut stage = Stage::new(sample::playing());
    stage.heading_menu();
    stage.key(Key::Enter, Modifiers::NONE);
    stage.frame(Vec::new());
    stage.typed("Draft");
    let list = stage.control("browser.select").unwrap();
    let actions = stage.click(list, PointerButton::Primary);
    assert!(
        app(&actions)
            .iter()
            .all(|command| !matches!(command, AppCommand::BrowserPlaylistCreate { .. })),
        "{actions:?}"
    );
    assert_eq!(stage.naming(), None);
}

#[test]
fn insert_on_a_focused_playlist_adds_the_track_the_list_has_selected() {
    let mut stage = Stage::new(sample::playing());
    let selected = stage.scene.browser.rows[2].path.clone();
    let row = stage.friday_row();
    stage.click(row, PointerButton::Primary);
    let actions = stage.key(Key::Insert, Modifiers::NONE);
    assert!(
        app(&actions).contains(&AppCommand::BrowserPlaylistAdd {
            playlist: friday(),
            path: selected,
        }),
        "{actions:?}"
    );
}

#[test]
fn a_track_dragged_onto_a_playlist_is_added_to_it() {
    let mut stage = Stage::new(sample::playing());
    let first = stage.scene.browser.rows[0].path.clone();
    let list = stage.control("browser.select").unwrap();
    // The first row of the list, below its header.
    let from = egui::pos2(list.x, stage.list_top() + 13.0);
    let to = stage.friday_row();
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    stage.frame(vec![Event::PointerMoved(from), button(from, true)]);
    let mut actions = Vec::new();
    for step in 1..=8 {
        let at = from + (to - from) * (step as f32 / 8.0);
        actions.extend(stage.frame(vec![Event::PointerMoved(at)]));
    }
    actions.extend(stage.frame(vec![button(to, false)]));
    actions.extend(stage.frame(Vec::new()));
    assert!(
        app(&actions).contains(&AppCommand::BrowserPlaylistAdd {
            playlist: friday(),
            path: first,
        }),
        "{actions:?}"
    );
}

impl Stage {
    /// The top of the rows of the list.
    fn list_top(&self) -> f32 {
        self.shell
            .controls()
            .iter()
            .find(|control| control.command == "browser.select")
            .unwrap()
            .rect
            .top()
    }
}

/// The folder *Archive* of the sample scenes, which holds *Summer terrace*.
const ARCHIVE: i64 = 3;

#[test]
fn move_to_lists_the_folders_a_playlist_can_go_to() {
    let mut stage = Stage::new(sample::playing());
    let row = stage.friday_row();
    stage.click(row, PointerButton::Secondary);
    // Move to is the second item: it opens the folders in place of the menu.
    stage.key(Key::ArrowDown, Modifiers::NONE);
    let opened = stage.key(Key::Enter, Modifiers::NONE);
    assert!(app(&opened).is_empty(), "{opened:?}");
    assert!(stage.menu_open());
    let places = stage.controls("browser.playlist.move");
    // The top of the tree and the folder Archive.
    assert_eq!(places.len(), 2);
    let actions = stage.click(places[1], PointerButton::Primary);
    assert_eq!(
        app(&actions),
        [AppCommand::BrowserPlaylistMove {
            item: friday(),
            folder: Some(NodeId::new(ARCHIVE)),
        }]
    );
}

#[test]
fn a_folder_cannot_be_moved_into_itself() {
    let mut stage = Stage::new(sample::playing());
    // Archive is the first playlist row; its folder row has no select control, so it is
    // reached through the playlist below it.
    let summer = stage.tree_rows("browser.playlist.select")[0];
    let archive = summer - egui::vec2(0.0, 27.0);
    stage.click(archive, PointerButton::Secondary);
    assert!(stage.menu_open());
    // New playlist, New folder, New smart playlist, Rename, Move to.
    for _ in 0..4 {
        stage.key(Key::ArrowDown, Modifiers::NONE);
    }
    stage.key(Key::Enter, Modifiers::NONE);
    // Only the top of the tree: Archive is the only folder.
    assert_eq!(stage.controls("browser.playlist.move").len(), 1);
}

#[test]
fn a_playlist_dragged_onto_a_folder_goes_into_it() {
    let mut stage = Stage::new(sample::playing());
    let from = stage.friday_row();
    let summer = stage.tree_rows("browser.playlist.select")[0];
    let to = summer - egui::vec2(0.0, 27.0);
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    stage.frame(vec![Event::PointerMoved(from), button(from, true)]);
    let mut actions = Vec::new();
    for step in 1..=8 {
        let at = from + (to - from) * (step as f32 / 8.0);
        actions.extend(stage.frame(vec![Event::PointerMoved(at)]));
    }
    actions.extend(stage.frame(vec![button(to, false)]));
    actions.extend(stage.frame(Vec::new()));
    assert!(
        app(&actions).contains(&AppCommand::BrowserPlaylistMove {
            item: friday(),
            folder: Some(NodeId::new(ARCHIVE)),
        }),
        "{actions:?}"
    );
}

#[test]
fn the_menu_of_a_row_resets_the_played_state_of_its_track() {
    let mut stage = Stage::new(sample::playlist());
    let list = stage.control("browser.select").unwrap();
    stage.click(list, PointerButton::Secondary);
    let reset = stage.control("browser.played.reset").unwrap();
    let actions = stage.click(reset, PointerButton::Primary);
    assert_eq!(app(&actions), [AppCommand::BrowserPlayedReset]);
}

#[test]
fn a_playlist_becomes_the_preparation_list_from_its_menu() {
    let mut stage = Stage::new(sample::playing());
    let row = stage.friday_row();
    stage.click(row, PointerButton::Secondary);
    let pick = stage.control("browser.preparation.select").unwrap();
    let actions = stage.click(pick, PointerButton::Primary);
    assert_eq!(
        app(&actions),
        [AppCommand::BrowserPreparationSelect { playlist: friday() }]
    );
    // *New this week*, the preparation list of the sample, does not offer it again.
    let new_this_week = stage.tree_rows("browser.playlist.select")[2];
    stage.click(new_this_week, PointerButton::Secondary);
    assert!(stage.menu_open());
    assert!(stage.control("browser.preparation.select").is_none());
}

#[test]
fn the_menu_of_a_row_adds_its_track_to_the_preparation_list_while_there_is_one() {
    let mut stage = Stage::new(sample::playlist());
    let list = stage.control("browser.select").unwrap();
    stage.click(list, PointerButton::Secondary);
    let append = stage.control("browser.preparation.append").unwrap();
    assert_eq!(
        app(&stage.click(append, PointerButton::Primary)),
        [AppCommand::BrowserPreparationAppend]
    );
    stage.click(list, PointerButton::Secondary);
    let next = stage.control("browser.preparation.next").unwrap();
    assert_eq!(
        app(&stage.click(next, PointerButton::Primary)),
        [AppCommand::BrowserPreparationNext]
    );

    let mut scene = sample::playlist();
    if let Some(collection) = &mut scene.browser.collection {
        for row in &mut collection.playlists {
            row.preparation = false;
        }
    }
    let mut stage = Stage::new(scene);
    stage.click(list, PointerButton::Secondary);
    assert!(stage.menu_open());
    assert!(stage.control("browser.preparation.append").is_none());
    assert!(stage.control("browser.preparation.next").is_none());
}

#[test]
fn the_preparation_node_shows_the_preparation_list_and_takes_the_selected_track() {
    let mut stage = Stage::new(sample::playing());
    let selected = stage.scene.browser.rows[2].path.clone();
    // Collection, then Preparation, in the order of the tree; History may sit below the edge.
    let nodes = stage.tree_rows("browser.node.select");
    let actions = stage.click(nodes[1], PointerButton::Primary);
    assert_eq!(
        app(&actions),
        [AppCommand::BrowserNodeSelect {
            node: BrowserNode::Preparation
        }]
    );
    let actions = stage.key(Key::Insert, Modifiers::NONE);
    assert!(
        app(&actions).contains(&AppCommand::BrowserPlaylistAdd {
            // *New this week*, the preparation list of the sample.
            playlist: NodeId::new(2),
            path: selected,
        }),
        "{actions:?}"
    );

    // Without a preparation list the node is not operated.
    let mut scene = sample::playing();
    if let Some(collection) = &mut scene.browser.collection {
        for row in &mut collection.playlists {
            row.preparation = false;
        }
    }
    let stage = Stage::new(scene);
    assert_eq!(
        stage.tree_rows("browser.node.select").len(),
        nodes.len() - 1
    );
    assert!(stage.chips("browser.node.select").len() == 2);
}

#[test]
fn a_playlist_goes_to_the_first_free_favourite_from_its_menu() {
    let mut stage = Stage::new(sample::playing());
    let row = stage.friday_row();
    stage.click(row, PointerButton::Secondary);
    let add = stage.control("browser.favourite.set").unwrap();
    // Places 4 and 5 hold *Summer terrace* and *New this week* in the sample.
    assert_eq!(
        app(&stage.click(add, PointerButton::Primary)),
        [AppCommand::BrowserFavouriteSet {
            place: 6,
            playlist: friday(),
        }]
    );
    // A playlist that is already a favourite is not offered again.
    let summer = stage.tree_rows("browser.playlist.select")[0];
    stage.click(summer, PointerButton::Secondary);
    assert!(stage.menu_open());
    assert!(stage.control("browser.favourite.set").is_none());
}

#[test]
fn a_favourite_shows_its_source_and_its_menu_frees_it() {
    let mut stage = Stage::new(sample::playing());
    // Collection, Preparation and History, then the two playlists of the sample.
    let fixed = stage.chips("browser.node.select");
    assert_eq!(fixed.len(), 3);
    assert_eq!(
        app(&stage.click(fixed[2], PointerButton::Primary)),
        [AppCommand::BrowserNodeSelect {
            node: BrowserNode::History
        }]
    );
    let summer = stage.chips("browser.playlist.select")[0];
    assert_eq!(
        app(&stage.click(summer, PointerButton::Primary)),
        [AppCommand::BrowserPlaylistSelect {
            playlist: NodeId::new(4)
        }]
    );
    stage.click(summer, PointerButton::Secondary);
    let clear = stage.control("browser.favourite.clear").unwrap();
    assert_eq!(
        app(&stage.click(clear, PointerButton::Primary)),
        [AppCommand::BrowserFavouriteClear { place: 4 }]
    );
}

#[test]
fn a_playlist_dragged_onto_a_free_favourite_goes_there() {
    let mut stage = Stage::new(sample::playing());
    let from = stage.friday_row();
    // The first free place: the first chip of the second line, below the first icon.
    let collection = stage.chips("browser.node.select")[0];
    let to = egui::pos2(collection.x + 8.0, collection.y + 28.0);
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    stage.frame(vec![Event::PointerMoved(from), button(from, true)]);
    let mut actions = Vec::new();
    for step in 1..=8 {
        let at = from + (to - from) * (step as f32 / 8.0);
        actions.extend(stage.frame(vec![Event::PointerMoved(at)]));
    }
    actions.extend(stage.frame(vec![button(to, false)]));
    actions.extend(stage.frame(Vec::new()));
    assert!(
        app(&actions).contains(&AppCommand::BrowserFavouriteSet {
            place: 6,
            playlist: friday(),
        }),
        "{actions:?}"
    );
}

#[test]
fn deleting_a_track_asks_first_and_keeping_it_sends_nothing() {
    let mut stage = Stage::new(sample::playlist());
    let list = stage.control("browser.select").unwrap();
    stage.click(list, PointerButton::Secondary);
    // Delete from collection is the last of ten items: Remove, the two of the preparation
    // list, Reset played state, Edit details, Import cover, Delete cover, Colour, Rating, then
    // it.
    for _ in 0..9 {
        stage.key(Key::ArrowDown, Modifiers::NONE);
    }
    let opened = stage.key(Key::Enter, Modifiers::NONE);
    assert!(app(&opened).is_empty(), "{opened:?}");
    assert!(stage.menu_open());
    let delete = stage.control("browser.track.delete").unwrap();
    // Keep it, the second entry, closes the question and deletes nothing.
    stage.key(Key::ArrowDown, Modifiers::NONE);
    let kept = stage.key(Key::Enter, Modifiers::NONE);
    assert!(app(&kept).is_empty(), "{kept:?}");
    assert!(!stage.menu_open());

    stage.click(list, PointerButton::Secondary);
    for _ in 0..9 {
        stage.key(Key::ArrowDown, Modifiers::NONE);
    }
    stage.key(Key::Enter, Modifiers::NONE);
    assert_eq!(
        app(&stage.click(delete, PointerButton::Primary)),
        [AppCommand::BrowserTrackDelete]
    );
}

#[test]
fn the_menus_export_and_import_playlists_with_the_folder_of_the_explorer() {
    let mut stage = Stage::new(sample::playing());
    let sets = std::path::PathBuf::from("Home/Music/Sets");
    let row = stage.friday_row();
    stage.click(row, PointerButton::Secondary);
    let export = stage.control("browser.playlist.export").unwrap();
    assert_eq!(
        app(&stage.click(export, PointerButton::Primary)),
        [AppCommand::BrowserPlaylistExport {
            playlist: friday(),
            path: sets.clone(),
        }]
    );
    stage.heading_menu();
    let import = stage.control("browser.playlist.import").unwrap();
    assert_eq!(
        app(&stage.click(import, PointerButton::Primary)),
        [AppCommand::BrowserPlaylistImport {
            folder: None,
            path: sets.clone(),
        }]
    );
    // `docs/behavior/browser.md`, *Playlist files*.
    stage.heading_menu();
    let collections = stage.control("browser.collection.import").unwrap();
    assert_eq!(
        app(&stage.click(collections, PointerButton::Primary)),
        [AppCommand::BrowserCollectionImport { path: sets }]
    );

    // Without a folder in the explorer, neither is offered.
    let mut scene = sample::playing();
    scene.browser.explorer = None;
    let mut stage = Stage::new(scene);
    let row = stage.friday_row();
    stage.click(row, PointerButton::Secondary);
    assert!(stage.menu_open());
    assert!(stage.control("browser.playlist.export").is_none());
}

#[test]
fn the_menu_of_a_row_sets_the_colour_and_the_rating_of_its_track() {
    let mut stage = Stage::new(sample::playlist());
    let list = stage.control("browser.select").unwrap();
    // Colour is the eighth item and Rating the ninth, after Edit details and the covers.
    for (downs, command, nth, expected) in [
        (
            7,
            "browser.track.colour.set",
            3,
            AppCommand::BrowserTrackColourSet { colour: 3 },
        ),
        (
            7,
            "browser.track.colour.set",
            0,
            AppCommand::BrowserTrackColourSet { colour: 0 },
        ),
        (
            8,
            "browser.track.rating.set",
            4,
            AppCommand::BrowserTrackRatingSet { rating: 4 },
        ),
    ] {
        stage.click(list, PointerButton::Secondary);
        for _ in 0..downs {
            stage.key(Key::ArrowDown, Modifiers::NONE);
        }
        let opened = stage.key(Key::Enter, Modifiers::NONE);
        assert!(app(&opened).is_empty(), "{opened:?}");
        let choices = stage.controls(command);
        assert_eq!(choices.len(), if downs == 7 { 9 } else { 6 });
        assert_eq!(
            app(&stage.click(choices[nth], PointerButton::Primary)),
            [expected]
        );
    }
}

#[test]
fn the_details_of_a_track_are_typed_and_saved_or_left_with_escape() {
    let mut stage = Stage::new(sample::playlist());
    let list = stage.control("browser.select").unwrap();
    stage.click(list, PointerButton::Secondary);
    let edit = stage.control("browser.track.details.open").unwrap();
    assert_eq!(
        app(&stage.click(edit, PointerButton::Primary)),
        [AppCommand::BrowserTrackDetailsOpen]
    );
    stage.shell.apply(&AppCommand::BrowserTrackDetailsOpen);
    stage.frame(Vec::new());
    // The selected row of the sample is Paper Lanterns, rated 3.
    let form = stage.shell.state.details.clone().unwrap();
    assert_eq!(form.details.title, "Paper Lanterns");
    assert_eq!(form.details.rating, 3);

    // The first field is the title: erase its last word and type another.
    let title = stage
        .shell
        .controls()
        .iter()
        .find(|control| control.command == "browser.track.details.save")
        .map(|control| control.rect.center())
        .unwrap();
    stage.click(title, PointerButton::Primary);
    let mut events: Vec<Event> = (0.."Lanterns".len())
        .map(|_| Event::Key {
            key: Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .collect();
    events.push(Event::Text("Moon".to_owned()));
    // Z would play deck A were it not typed into the field.
    events.push(Event::Text("z".to_owned()));
    let typed = stage.frame(events);
    assert!(app(&typed).is_empty(), "{typed:?}");
    assert!(
        typed.iter().all(|action| matches!(action, Action::App(_))),
        "{typed:?}"
    );

    let five = stage.controls("browser.track.details.rating")[5];
    let chosen = stage.click(five, PointerButton::Primary);
    assert_eq!(
        app(&chosen),
        [AppCommand::BrowserTrackDetailsRating { rating: 5 }]
    );
    stage
        .shell
        .apply(&AppCommand::BrowserTrackDetailsRating { rating: 5 });
    stage.frame(Vec::new());
    let save = stage
        .shell
        .controls()
        .iter()
        .filter(|control| {
            control.command == "browser.track.details.save" && control.kind != ControlKind::Keys
        })
        .map(|control| control.rect.center())
        .next()
        .unwrap();
    let saved = app(&stage.click(save, PointerButton::Primary));
    let [
        AppCommand::BrowserTrackDetailsSave {
            details,
            fields,
            position,
            then,
        },
    ] = saved.as_slice()
    else {
        panic!("{saved:?}");
    };
    assert_eq!(details.title, "Paper Moonz");
    assert_eq!(details.artist, "Mirela Costin");
    assert_eq!(details.rating, 5);
    // For one track, the fields that changed.
    assert_eq!(*fields, DetailFields::parse("title,rating").unwrap());
    assert_eq!((*position, *then), (0, DetailsThen::Close));

    // Escape leaves without saving.
    let closed = stage.key(Key::Escape, Modifiers::NONE);
    assert_eq!(app(&closed), [AppCommand::BrowserTrackDetailsClose]);
}

impl Stage {
    fn click_with(&mut self, at: Pos2, modifiers: Modifiers) -> Vec<Action> {
        let press = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers,
        };
        let mut actions = self.frame_with(vec![Event::PointerMoved(at), press(true)], modifiers);
        actions.extend(self.frame_with(vec![press(false)], modifiers));
        actions.extend(self.frame(Vec::new()));
        actions
    }
}

/// Behaviour note: `docs/behavior/browser.md`, *Selecting several tracks*.
#[test]
fn control_and_shift_change_what_a_click_on_a_row_selects() {
    let mut stage = Stage::new(sample::playlist());
    let list = stage.control("browser.select").unwrap();
    let toggled = app(&stage.click_with(list, Modifiers::COMMAND));
    assert!(
        toggled
            .iter()
            .any(|command| matches!(command, AppCommand::BrowserSelectToggle { .. })),
        "{toggled:?}"
    );
    let extended = app(&stage.click_with(list, Modifiers::SHIFT));
    assert!(
        extended
            .iter()
            .any(|command| matches!(command, AppCommand::BrowserSelectExtend { .. })),
        "{extended:?}"
    );
    assert!(
        !toggled
            .iter()
            .chain(&extended)
            .any(|command| matches!(command, AppCommand::BrowserSelect { .. }))
    );

    // Shift and an arrow extend from the current row of the focused list.
    stage.click(list, PointerButton::Primary);
    let selected = stage.scene.browser.selected.unwrap();
    let actions = app(&stage.key(Key::ArrowDown, Modifiers::SHIFT));
    assert_eq!(
        actions,
        [AppCommand::BrowserSelectExtend {
            index: selected + 1
        }]
    );
}

#[test]
fn control_and_a_selects_every_row_unless_a_field_or_a_dialog_has_the_keyboard() {
    let mut stage = Stage::new(sample::playlist());
    assert!(app(&stage.key(Key::A, Modifiers::COMMAND)).contains(&AppCommand::BrowserSelectAll));

    stage.shell.apply(&AppCommand::BrowserSearchFocus);
    stage.frame(Vec::new());
    assert!(!app(&stage.key(Key::A, Modifiers::COMMAND)).contains(&AppCommand::BrowserSelectAll));

    let mut stage = Stage::new(sample::playlist());
    stage.shell.apply(&AppCommand::BrowserTrackDetailsOpen);
    stage.frame(Vec::new());
    assert!(stage.shell.state.details.is_some());
    assert!(!app(&stage.key(Key::A, Modifiers::COMMAND)).contains(&AppCommand::BrowserSelectAll));
}

#[test]
fn the_menu_of_several_rows_offers_only_what_acts_on_them_all() {
    let mut scene = sample::playlist();
    scene.browser.marked = (0..scene.browser.rows.len()).collect();
    let mut stage = Stage::new(scene);
    let list = stage.control("browser.select").unwrap();
    let opened = app(&stage.click(list, PointerButton::Secondary));
    // A selected row keeps the selection.
    assert!(
        !opened
            .iter()
            .any(|command| matches!(command, AppCommand::BrowserSelect { .. })),
        "{opened:?}"
    );
    assert!(stage.menu_open());
    assert!(stage.control("browser.track.details.open").is_some());
    assert!(stage.control("browser.track.cover.delete").is_some());
    for single in [
        "browser.playlist.remove",
        "browser.played.reset",
        "browser.preparation.append",
    ] {
        assert!(stage.control(single).is_none(), "{single}");
    }
}

/// The save keys of the dialog of the details that are not text fields, in the order drawn.
fn detail_keys(stage: &Stage) -> Vec<Pos2> {
    stage
        .shell
        .controls()
        .iter()
        .filter(|control| {
            control.command == "browser.track.details.save" && control.kind != ControlKind::Keys
        })
        .map(|control| control.rect.center())
        .collect()
}

/// Behaviour note: `docs/behavior/browser.md`, *Track details*.
#[test]
fn the_details_of_several_tracks_tick_what_they_share_and_move_one_by_one() {
    let mut scene = sample::playlist();
    scene.browser.marked = vec![1, 2];
    scene.browser.selected = Some(1);
    let mut stage = Stage::new(scene);
    stage.shell.apply(&AppCommand::BrowserTrackDetailsOpen);
    stage.frame(Vec::new());
    let form = stage.shell.state.details.clone().unwrap();
    assert!(form.several());
    assert_eq!((form.count, form.position), (2, 0));
    // The two tracks have different titles and ratings: those fields open unticked and empty.
    assert!(!form.ticked.contains(DetailField::Title));
    assert!(!form.ticked.contains(DetailField::Rating));
    assert_eq!(form.details.title, "");
    assert_eq!(form.ticked, form.shared);
    assert!(form.to_write().is_empty());

    // A tick key on each field; the first is the title.
    let ticks = stage.controls("browser.track.details.tick");
    assert_eq!(ticks.len(), 6);
    let ticked = app(&stage.click(ticks[0], PointerButton::Primary));
    assert_eq!(
        ticked,
        [AppCommand::BrowserTrackDetailsTick {
            field: DetailField::Title
        }]
    );
    stage.shell.apply(&ticked[0]);
    stage.frame(Vec::new());

    // Previous, Next and Save: Previous is off at the first position.
    let keys = detail_keys(&stage);
    assert_eq!(keys.len(), 3);
    let moved = app(&stage.click(keys[1], PointerButton::Primary));
    let [
        AppCommand::BrowserTrackDetailsSave {
            fields,
            position: 0,
            then: DetailsThen::Next,
            ..
        },
    ] = moved.as_slice()
    else {
        panic!("{moved:?}");
    };
    // A ticked field whose values differed is written, even empty.
    assert_eq!(*fields, DetailFields::parse("title").unwrap());
    stage.shell.apply(&moved[0]);
    stage.frame(Vec::new());
    let form = stage.shell.state.details.clone().unwrap();
    assert_eq!(form.position, 1);
    assert!(!form.several());
    // The tracks go by their files, not by their places in the list.
    let first = [1, 2]
        .map(|index| &stage.scene.browser.rows[index])
        .into_iter()
        .min_by(|a, b| a.path.cmp(&b.path))
        .unwrap();
    assert_eq!(form.file, first.name);
    // What was saved shows at once, before the library answers.
    assert_eq!(form.details.title, "");

    // Next again shows the second track; Previous from there goes back to the first.
    let keys = detail_keys(&stage);
    let next = app(&stage.click(keys[1], PointerButton::Primary));
    stage.shell.apply(&next[0]);
    stage.frame(Vec::new());
    assert_eq!(stage.shell.state.details.as_ref().unwrap().position, 2);
    let keys = detail_keys(&stage);
    let back = app(&stage.click(keys[0], PointerButton::Primary));
    stage.shell.apply(&back[0]);
    stage.frame(Vec::new());
    assert_eq!(stage.shell.state.details.as_ref().unwrap().position, 1);

    // The selection changes behind the dialog: it closes without saving.
    stage.scene.browser.marked = vec![1];
    let closed = app(&stage.frame(Vec::new()));
    assert!(closed.is_empty(), "{closed:?}");
    assert!(stage.shell.state.details.is_none());
}

/// Behaviour note: `docs/behavior/cover-art.md`, *Importing and deleting a cover*.
#[test]
fn the_menu_imports_a_picture_of_the_explorer_as_the_cover_or_deletes_it() {
    let mut stage = Stage::new(sample::playlist());
    let list = stage.control("browser.select").unwrap();
    // Import cover and Delete cover follow Edit details, the sixth and seventh items.
    stage.click(list, PointerButton::Secondary);
    for _ in 0..5 {
        stage.key(Key::ArrowDown, Modifiers::NONE);
    }
    // Import cover opens the pictures of the folder of the explorer in its place.
    let opened = stage.key(Key::Enter, Modifiers::NONE);
    assert!(app(&opened).is_empty(), "{opened:?}");
    assert!(stage.menu_open());
    let pictures = stage.controls("browser.track.cover.import");
    assert_eq!(pictures.len(), 2);
    assert_eq!(
        app(&stage.click(pictures[1], PointerButton::Primary)),
        [AppCommand::BrowserTrackCoverImport {
            path: stage.scene.browser.pictures[1].clone()
        }]
    );
    stage.click(list, PointerButton::Secondary);
    for _ in 0..6 {
        stage.key(Key::ArrowDown, Modifiers::NONE);
    }
    assert_eq!(
        app(&stage.key(Key::Enter, Modifiers::NONE)),
        [AppCommand::BrowserTrackCoverDelete]
    );
}

#[test]
fn the_entries_of_the_covers_are_absent_unless_every_tag_is_written() {
    // What the sixth item of the menu of a row is.
    #[derive(Debug, PartialEq)]
    enum Sixth {
        Pictures,
        Delete,
        Colours,
    }
    for (mode, pictures, sixth) in [
        (TagWriting::Everything, true, Sixth::Pictures),
        (TagWriting::Everything, false, Sixth::Delete),
        (TagWriting::PlatterOnly, true, Sixth::Colours),
        (TagWriting::Nothing, true, Sixth::Colours),
    ] {
        let mut scene = sample::playlist();
        scene.settings.tag_writing = mode;
        if !pictures {
            scene.browser.pictures.clear();
        }
        let mut stage = Stage::new(scene);
        let list = stage.control("browser.select").unwrap();
        stage.click(list, PointerButton::Secondary);
        assert_eq!(
            stage.control("browser.track.cover.delete").is_some(),
            sixth != Sixth::Colours,
            "{mode:?}"
        );
        for _ in 0..5 {
            stage.key(Key::ArrowDown, Modifiers::NONE);
        }
        let sent = app(&stage.key(Key::Enter, Modifiers::NONE));
        let found = if sent == [AppCommand::BrowserTrackCoverDelete] {
            Sixth::Delete
        } else if !stage.controls("browser.track.cover.import").is_empty() {
            Sixth::Pictures
        } else {
            assert!(!stage.controls("browser.track.colour.set").is_empty());
            Sixth::Colours
        };
        assert_eq!(found, sixth, "{mode:?}, pictures {pictures}");
    }
}

#[test]
fn every_picture_offered_for_a_cover_fits_in_the_window() {
    let mut scene = sample::playlist();
    scene.browser.pictures = (0..MAX_PICTURES)
        .map(|index| {
            std::path::PathBuf::from(format!(
                "Home/Music/Sets/A rather long name for the picture of a cover {index:02}.jpeg"
            ))
        })
        .collect();
    let mut stage = Stage::new(scene);
    let list = stage.control("browser.select").unwrap();
    stage.click(list, PointerButton::Secondary);
    for _ in 0..5 {
        stage.key(Key::ArrowDown, Modifiers::NONE);
    }
    stage.key(Key::Enter, Modifiers::NONE);
    let window = Rect::from_min_size(Pos2::ZERO, egui::vec2(DESIGN_WIDTH, WINDOW_HEIGHT));
    let pictures: Vec<Rect> = stage
        .shell
        .controls()
        .iter()
        .filter(|control| control.command == "browser.track.cover.import")
        .map(|control| control.rect)
        .collect();
    assert_eq!(pictures.len(), MAX_PICTURES);
    for rect in pictures {
        assert!(window.contains_rect(rect), "{rect:?}");
    }
}

/// Behaviour note: `docs/behavior/cover-art.md`, *In the list of tracks*.
#[test]
fn covers_are_drawn_in_the_rows_and_in_the_header_of_a_deck() {
    let mut scene = sample::playlist();
    let cover = |red: u8| platter_ui_egui::model::CoverImage {
        width: 2,
        height: 2,
        rgba: std::sync::Arc::from(vec![red; 16]),
    };
    let paths: Vec<_> = scene
        .browser
        .rows
        .iter()
        .map(|row| row.path.clone())
        .collect();
    for (index, path) in paths.into_iter().enumerate() {
        scene.browser.covers.insert(path, cover(index as u8));
    }
    scene.browser.deck_covers = [Some(cover(200)), None, None, None];
    let mut stage = Stage::new(scene);
    // Drawn again with the same pictures, then with one read again, as after an import.
    stage.frame(Vec::new());
    let first = stage.scene.browser.rows[0].path.clone();
    stage.scene.browser.covers.insert(first, cover(99));
    stage.frame(Vec::new());
}
