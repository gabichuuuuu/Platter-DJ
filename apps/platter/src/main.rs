//! Platter, the desktop application.
//!
//! Without arguments it opens the main window. `--capture <state> <file.png>` draws one of
//! the states of the reference screens from invented data and saves it as an image.

// A release build for Windows has no console window behind the application.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod capture;
mod config;
mod png;

use std::path::PathBuf;
use std::process::ExitCode;

use eframe::egui;
use platter_ui_egui::shell::{DESIGN_HEIGHT, DESIGN_WIDTH, Shell};

use crate::app::PlatterApp;
use crate::config::{SETTINGS_FILE, Settings};

const USAGE: &str = "Usage:
  platter                                  open the application
  platter --capture <state> <file.png>     save a picture of one state of the interface
  platter --help                           show this text

States: main, startup, preferences-audio, preferences-general";

fn run() -> Result<(), String> {
    let folder = config::config_dir();
    let settings = folder.as_ref().map_or_else(Settings::default, |folder| {
        Settings::load(&folder.join(SETTINGS_FILE))
    });
    let shell = Shell::new(settings.language).map_err(|error| error.to_string())?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Platter DJ")
            .with_app_id("platter")
            .with_inner_size([DESIGN_WIDTH, DESIGN_HEIGHT])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Platter DJ",
        options,
        Box::new(move |creation| {
            shell.install(&creation.egui_ctx);
            Ok(Box::new(PlatterApp::new(shell, settings, folder)))
        }),
    )
    .map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let result = match arguments.as_slice() {
        [] => run(),
        ["--capture", state, file] => capture::run(state, PathBuf::from(file)),
        ["--help" | "-h"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
