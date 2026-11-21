//! The MCP server of the application: with `--mcp` it reads the messages of a client on the
//! standard input and writes the answers on the standard output, one per line. A thread of
//! its own reads and writes; the window answers each message between two frames, since only
//! it holds the engine and the collection.
//!
//! Behaviour note: `docs/behavior/mcp.md`; the choices: `docs/decisions/0037-mcp-server.md`.

use std::io::{BufRead, Write};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

use eframe::egui;
use platter_core::{DeckId, Stem};
use platter_engine::{DeckSnapshot, Snapshot};
use platter_mcp::json::{self, MAX_MESSAGE_BYTES, Value};

/// How long the reader waits for the window to answer a message. A window that does not
/// draw, minimised on some systems, answers when it draws again; the client gets an error
/// meanwhile.
const ANSWER_WAIT: Duration = Duration::from_secs(10);

/// A message of the client, for the window to answer.
pub struct Request {
    pub line: String,
    pub reply: SyncSender<Option<String>>,
}

/// Starts the thread that reads the standard input. When the client closes it the window is
/// closed too, as the transport asks of a server.
pub fn start(ctx: egui::Context) -> std::io::Result<Receiver<Request>> {
    let (sender, requests) = sync_channel(1);
    std::thread::Builder::new()
        .name("platter-mcp".to_owned())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            let stdout = std::io::stdout();
            // The end of the input, or an input that cannot be read, ends the session.
            while let Ok(Some(line)) = read_line(&mut input) {
                let answer = match line {
                    Line::Message(line) if line.trim().is_empty() => continue,
                    Line::Message(line) => {
                        // A notification is never answered, not even when the window is late.
                        let id = json::parse(&line)
                            .ok()
                            .and_then(|message| message.get("id").cloned());
                        let (reply, answer) = sync_channel(1);
                        if sender.send(Request { line, reply }).is_err() {
                            break;
                        }
                        ctx.request_repaint();
                        match answer.recv_timeout(ANSWER_WAIT) {
                            Ok(answer) => answer,
                            Err(_) => id.map(|id| busy(&id)),
                        }
                    }
                    Line::TooLong => Some(too_long()),
                };
                if let Some(answer) = answer {
                    let mut out = stdout.lock();
                    if writeln!(out, "{answer}")
                        .and_then(|()| out.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        })?;
    Ok(requests)
}

enum Line {
    Message(String),
    /// Longer than [`MAX_MESSAGE_BYTES`]: skipped to its end, never kept whole.
    TooLong,
}

/// The next line, without its end; `None` at the end of the input.
fn read_line(input: &mut impl BufRead) -> std::io::Result<Option<Line>> {
    let mut bytes = Vec::new();
    let mut too_long = false;
    loop {
        let buffer = input.fill_buf()?;
        if buffer.is_empty() {
            return Ok((!bytes.is_empty() || too_long).then(|| finish(bytes, too_long)));
        }
        let (taken, ended) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(end) => (end + 1, true),
            None => (buffer.len(), false),
        };
        if !too_long {
            let content = &buffer[..if ended { taken - 1 } else { taken }];
            if bytes.len() + content.len() > MAX_MESSAGE_BYTES {
                too_long = true;
                bytes = Vec::new();
            } else {
                bytes.extend_from_slice(content);
            }
        }
        input.consume(taken);
        if ended {
            return Ok(Some(finish(bytes, too_long)));
        }
    }
}

fn finish(mut bytes: Vec<u8>, too_long: bool) -> Line {
    if too_long {
        return Line::TooLong;
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    // A line that is not UTF-8 reaches the server as text it cannot read.
    Line::Message(String::from_utf8_lossy(&bytes).into_owned())
}

fn error(id: Option<&Value>, code: f64, message: &str) -> String {
    let mut members = vec![("jsonrpc".to_owned(), Value::text("2.0"))];
    if let Some(id) = id {
        members.push(("id".to_owned(), id.clone()));
    }
    members.push((
        "error".to_owned(),
        Value::object([
            ("code", Value::Number(code)),
            ("message", Value::text(message)),
        ]),
    ));
    Value::Object(members).to_line()
}

fn too_long() -> String {
    error(None, -32700.0, "the message is too long")
}

/// The window did not answer in time. The command may still run once it draws again.
fn busy(id: &Value) -> String {
    error(Some(id), -32603.0, "the window did not answer in time")
}

/// What `get_state` gives: the decks, the mixer and the clock. Tracks are named by their
/// tags and the name of their file, not by their folder.
pub fn state_of(snapshot: &Snapshot) -> Value {
    Value::object([
        (
            "decks",
            Value::Array(
                DeckId::ALL
                    .into_iter()
                    .zip(&snapshot.decks)
                    .map(|(deck, state)| deck_state(deck, state))
                    .collect(),
            ),
        ),
        (
            "mixer",
            Value::object([
                ("crossfader", Value::Number(snapshot.mixer.crossfader)),
                (
                    "mode",
                    Value::text(format!("{:?}", snapshot.mixer.mode).to_lowercase()),
                ),
            ]),
        ),
        (
            "clock",
            Value::object([
                ("bpm", Value::Number(snapshot.clock.bpm)),
                (
                    "master",
                    snapshot.clock.master.map_or(Value::Null, |deck| {
                        Value::text(deck.to_string().to_lowercase())
                    }),
                ),
            ]),
        ),
    ])
}

fn deck_state(deck: DeckId, state: &DeckSnapshot) -> Value {
    let track = state.track.as_ref();
    let text = |value: Option<&String>| value.map_or(Value::Null, |text| Value::text(text.clone()));
    let bpm = state
        .grid
        .bpm()
        .map_or(Value::Null, |bpm| Value::Number(bpm.value() * state.speed));
    let stems = state.stem_peaks.as_ref().map_or(Value::Null, |_| {
        Value::Array(
            Stem::ALL
                .into_iter()
                .zip(state.controls.stems)
                .map(|(stem, settings)| {
                    Value::object([
                        ("stem", Value::text(stem.name())),
                        ("volume", Value::Number(f64::from(settings.volume))),
                        ("muted", Value::Bool(settings.muted)),
                        ("filter_on", Value::Bool(settings.filter_on)),
                        ("filter", Value::Number(f64::from(settings.filter))),
                        ("send_on", Value::Bool(settings.send_on)),
                        ("send", Value::Number(f64::from(settings.send))),
                    ])
                })
                .collect(),
        )
    });
    Value::object([
        ("deck", Value::text(deck.to_string().to_lowercase())),
        ("loaded", Value::Bool(track.is_some())),
        ("loading", Value::Bool(state.loading)),
        (
            "title",
            text(track.and_then(|track| track.tags.title.as_ref())),
        ),
        (
            "artist",
            text(track.and_then(|track| track.tags.artist.as_ref())),
        ),
        (
            "file",
            track
                .and_then(|track| track.path.file_name())
                .map_or(Value::Null, |name| Value::text(name.to_string_lossy())),
        ),
        (
            "seconds",
            Value::Number(track.map_or(0.0, |track| track.seconds)),
        ),
        ("position_seconds", Value::Number(state.position_seconds)),
        ("playing", Value::Bool(state.playing)),
        ("speed", Value::Number(state.speed)),
        ("bpm", bpm),
        ("sync", Value::Bool(state.sync)),
        ("key_lock", Value::Bool(state.key_lock)),
        ("fader", Value::Number(state.controls.fader)),
        ("stems", stems),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(input: &[u8]) -> Vec<String> {
        let mut input = std::io::BufReader::with_capacity(16, input);
        let mut out = Vec::new();
        while let Ok(Some(line)) = read_line(&mut input) {
            out.push(match line {
                Line::Message(text) => text,
                Line::TooLong => "<too long>".to_owned(),
            });
        }
        out
    }

    #[test]
    fn lines_end_at_a_newline_with_or_without_a_return_and_long_ones_are_skipped() {
        let long = "x".repeat(MAX_MESSAGE_BYTES + 1);
        let input = format!("{{\"a\":1}}\r\n\n{long}\nlast");
        assert_eq!(
            lines(input.as_bytes()),
            ["{\"a\":1}", "", "<too long>", "last"]
        );
    }

    #[test]
    fn the_state_names_each_deck_and_the_stems_only_of_a_stem_deck() {
        let snapshot = Snapshot::default();
        let state = state_of(&snapshot);
        let Some(Value::Array(decks)) = state.get("decks") else {
            panic!("{state:?}");
        };
        assert_eq!(decks.len(), 4);
        assert_eq!(decks[3].get("deck"), Some(&Value::text("d")));
        assert_eq!(decks[0].get("stems"), Some(&Value::Null));
    }
}
