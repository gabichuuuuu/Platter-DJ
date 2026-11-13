//! `platter-cli`: file information and render without a sound card.
//!
//! The render drives the same engine and the same processor as the desktop application, so
//! what a script produces here is what the same commands produce live.

mod args;
mod script;

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::process::ExitCode;

use platter_core::log::{Level, Logger};
use platter_decode::decode_file;
use platter_engine::{WavWriter, render};

use args::Invocation;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let invocation = match args::parse(&arguments) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("platter-cli: {error}\n\n{}", args::USAGE);
            return ExitCode::from(2);
        }
    };
    let result = match invocation {
        Invocation::Help => {
            println!("{}", args::USAGE);
            Ok(())
        }
        Invocation::Info { file } => info(&file),
        Invocation::Render { script, out, log } => run_render(&script, &out, log.as_deref()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("platter-cli: {message}");
            ExitCode::FAILURE
        }
    }
}

fn info(file: &Path) -> Result<(), String> {
    let audio = decode_file(file).map_err(|error| format!("{}: {error}", file.display()))?;
    println!("format:      {}", audio.container.name());
    println!("duration:    {}", duration(audio.seconds()));
    println!("channels:    {}", audio.channels);
    println!("sample rate: {} Hz", audio.sample_rate.hz());
    let tags = [
        ("title", &audio.tags.title),
        ("artist", &audio.tags.artist),
        ("album", &audio.tags.album),
    ];
    for (name, value) in tags {
        if let Some(value) = value {
            println!("{:<12} {value}", format!("{name}:"));
        }
    }
    Ok(())
}

/// Minutes and seconds, with the exact seconds alongside.
fn duration(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor();
    format!(
        "{minutes:.0}:{:06.3} ({seconds:.3} s)",
        seconds - minutes * 60.0
    )
}

fn run_render(script: &Path, out: &Path, log: Option<&Path>) -> Result<(), String> {
    let logger = match log {
        Some(path) => Some(
            Logger::to_file(path)
                .map_err(|error| format!("cannot open the log {}: {error}", path.display()))?,
        ),
        None => None,
    };
    let result = render_to_file(script, out, logger.as_ref());
    if let Err(message) = &result {
        if let Some(logger) = &logger {
            logger.line(Level::Error, message);
        }
        // A partial file would pass for a finished mix.
        let _ = std::fs::remove_file(out);
    }
    result
}

fn render_to_file(script: &Path, out: &Path, logger: Option<&Logger>) -> Result<(), String> {
    let note = |message: &str| {
        if let Some(logger) = logger {
            logger.line(Level::Info, message);
        }
    };
    let text = std::fs::read_to_string(script)
        .map_err(|error| format!("cannot read {}: {error}", script.display()))?;
    let folder = script.parent().unwrap_or(Path::new(""));
    let plan = script::parse(&text, folder).map_err(|error| error.to_string())?;
    note(&format!(
        "script read: {} commands, {} s at {} Hz",
        plan.commands.len(),
        plan.seconds,
        plan.sample_rate.hz()
    ));

    let file =
        File::create(out).map_err(|error| format!("cannot create {}: {error}", out.display()))?;
    let output_error = |error: std::io::Error| format!("cannot write {}: {error}", out.display());
    let mut writer =
        WavWriter::new(BufWriter::new(file), plan.sample_rate, 2).map_err(output_error)?;
    let report = render(&plan, |block| writer.write(block)).map_err(|error| error.to_string())?;
    writer.finish().map_err(output_error)?;

    let summary = format!(
        "rendered {:.3} s ({} frames), peak {:.4}",
        report.frames as f64 / plan.sample_rate.as_f64(),
        report.frames,
        report.peak
    );
    println!("{summary}");
    note(&summary);
    if report.lowest_limiter_gain < 1.0 {
        let warning = format!(
            "the safety limiter reduced the level by up to {:.1} dB",
            -20.0 * report.lowest_limiter_gain.max(1e-9).log10()
        );
        println!("{warning}");
        if let Some(logger) = logger {
            logger.line(Level::Warning, &warning);
        }
    }
    Ok(())
}
