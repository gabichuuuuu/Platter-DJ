//! A log written to a local file.
//!
//! Lines are handed to a writer thread through a channel, so the caller never waits for the
//! disk. The audio thread does not log at all: it reports through atomics and queues, and a
//! normal thread writes what it reads there. Nothing in this module opens a network
//! connection, and the log never leaves the machine.

use std::fmt;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}

/// Writes lines to one file until it is dropped. Dropping it waits for the pending lines.
pub struct Logger {
    lines: Option<Sender<String>>,
    writer: Option<JoinHandle<()>>,
    started: Instant,
}

impl Logger {
    /// Creates or truncates the file and starts the writer thread.
    pub fn to_file(path: &Path) -> std::io::Result<Self> {
        let mut file = BufWriter::new(File::create(path)?);
        let (lines, pending) = channel::<String>();
        let writer = std::thread::Builder::new()
            .name("platter-log".to_owned())
            .spawn(move || {
                for line in pending {
                    // A full disk must not take the program down; the line is lost. Each
                    // line reaches the disk at once: the last lines before a crash are the
                    // ones that matter.
                    let _ = writeln!(file, "{line}");
                    let _ = file.flush();
                }
            })?;
        Ok(Self {
            lines: Some(lines),
            writer: Some(writer),
            started: Instant::now(),
        })
    }

    /// Queues one line, stamped with the seconds elapsed since the log was opened.
    pub fn line(&self, level: Level, message: &str) {
        let elapsed = self.started.elapsed().as_secs_f64();
        if let Some(lines) = &self.lines {
            // The writer only stops when this value is dropped, so a failed send means the
            // thread died; there is nowhere left to report it.
            let _ = lines.send(format!("{elapsed:10.3} {level}: {message}"));
        }
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        // Closing the channel ends the loop of the writer.
        self.lines = None;
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Level, Logger};

    #[test]
    fn writes_every_line_before_drop_returns() {
        let path = std::env::temp_dir().join(format!("platter-log-test-{}", std::process::id()));
        let logger = Logger::to_file(&path).unwrap();
        logger.line(Level::Info, "first");
        logger.line(Level::Error, "second");
        drop(logger);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("info: first"));
        assert!(lines[1].ends_with("error: second"));
    }
}
