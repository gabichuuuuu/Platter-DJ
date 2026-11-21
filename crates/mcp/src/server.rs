//! The MCP server: JSON-RPC 2.0 messages, one per line, as the stdio transport of the Model
//! Context Protocol carries them. It offers three tools over the commands of Platter.
//!
//! Behaviour note: `docs/behavior/mcp.md`; the choices: `docs/decisions/0037-mcp-server.md`.
//! The protocol is read from its specification, revisions 2026-07-28 and 2025-11-25.

use platter_engine::command::{Args, CommandSpec, ParamKind, Value as ArgValue};

use crate::json::{self, Value};

/// The revision of the protocol this server follows, with its requests that carry their
/// version in `_meta`.
pub const PROTOCOL_VERSION: &str = "2026-07-28";
/// The earlier revision, for clients that start with `initialize`.
pub const LEGACY_VERSION: &str = "2025-11-25";
const SUPPORTED: [&str; 2] = [PROTOCOL_VERSION, LEGACY_VERSION];
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const SERVER_INFO_KEY: &str = "io.modelcontextprotocol/serverInfo";

const PARSE_ERROR: f64 = -32700.0;
const INVALID_REQUEST: f64 = -32600.0;
const METHOD_NOT_FOUND: f64 = -32601.0;
const INVALID_PARAMS: f64 = -32602.0;
const UNSUPPORTED_VERSION: f64 = -32022.0;

const INSTRUCTIONS: &str = "Platter DJ, a DJ application with four decks, a mixer and a \
     collection. Its actions are commands with identifiers such as deck.play_pause or \
     mixer.crossfader.set: platter_list_commands finds them by words and gives their \
     parameters, platter_run_command runs one, and platter_get_state reads the decks, the \
     mixer and the clock.";

/// Tool names carry the name of the service, so they stay apart from those of other servers.
const LIST: &str = "platter_list_commands";
const RUN: &str = "platter_run_command";
const STATE: &str = "platter_get_state";
/// Commands `platter_list_commands` gives when no limit is asked, and the most it gives.
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 100;

/// What the application gives the server: its commands, a way to run one, and its state.
/// The server calls it from the thread that owns the application.
pub trait Host {
    /// Every command, in a stable order.
    fn commands(&self) -> Vec<&'static CommandSpec>;
    /// Runs a command; the text says what happened, or why it was refused.
    fn run(&mut self, id: &str, args: &Args) -> Result<String, String>;
    /// The state of the decks, the mixer and the clock.
    fn state(&mut self) -> Value;
}

/// The name and the version the server gives of itself.
pub struct Server {
    pub name: String,
    pub version: String,
}

impl Server {
    /// Answers one message. A notification, and a message that is not a request, get no
    /// answer: `None`.
    pub fn handle(&self, line: &str, host: &mut dyn Host) -> Option<String> {
        let message = match json::parse(line) {
            Ok(message) => message,
            Err(error) => return Some(error_line(None, PARSE_ERROR, &error.to_string(), None)),
        };
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response, which a client does not send, or nothing that makes sense.
            return message
                .get("id")
                .map(|id| error_line(Some(id), INVALID_REQUEST, "not a request", None));
        };
        // A notification asks for nothing: initialized, cancelled and the rest.
        let id = message.get("id")?;
        if !matches!(id, Value::Number(_) | Value::Text(_))
            || message.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        {
            return Some(error_line(
                None,
                INVALID_REQUEST,
                "not a JSON-RPC 2.0 request",
                None,
            ));
        }
        let empty = Value::Object(Vec::new());
        let params = message.get("params").unwrap_or(&empty);
        if let Some(requested) = params
            .get("_meta")
            .and_then(|meta| meta.get(VERSION_KEY))
            .and_then(Value::as_str)
            && !SUPPORTED.contains(&requested)
        {
            let data = Value::object([
                ("supported", supported()),
                ("requested", Value::text(requested)),
            ]);
            return Some(error_line(
                Some(id),
                UNSUPPORTED_VERSION,
                "Unsupported protocol version",
                Some(data),
            ));
        }
        let result = match method {
            "initialize" => Ok(self.initialize(params)),
            "ping" => Ok(Value::Object(Vec::new())),
            "server/discover" => Ok(self.discover()),
            "tools/list" => Ok(complete(vec![("tools", tools())])),
            "tools/call" => call(params, host),
            _ => Err((METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        };
        Some(match result {
            Ok(result) => Value::object([
                ("jsonrpc", Value::text("2.0")),
                ("id", id.clone()),
                ("result", result),
            ])
            .to_line(),
            Err((code, message)) => error_line(Some(id), code, &message, None),
        })
    }

    fn info(&self) -> Value {
        Value::object([
            ("name", Value::text(&self.name)),
            ("version", Value::text(&self.version)),
        ])
    }

    /// The handshake of the earlier revision: the version asked for when this server speaks
    /// it, otherwise the latest it speaks in that revision.
    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let version = asked
            .filter(|asked| SUPPORTED.contains(asked))
            .unwrap_or(LEGACY_VERSION);
        Value::object([
            ("protocolVersion", Value::text(version)),
            ("capabilities", capabilities()),
            ("serverInfo", self.info()),
            ("instructions", Value::text(INSTRUCTIONS)),
        ])
    }

    fn discover(&self) -> Value {
        complete(vec![
            ("supportedVersions", supported()),
            ("capabilities", capabilities()),
            ("instructions", Value::text(INSTRUCTIONS)),
            ("_meta", Value::object([(SERVER_INFO_KEY, self.info())])),
        ])
    }
}

fn supported() -> Value {
    Value::Array(
        SUPPORTED
            .iter()
            .map(|version| Value::text(*version))
            .collect(),
    )
}

fn capabilities() -> Value {
    Value::object([(
        "tools",
        Value::object([("listChanged", Value::Bool(false))]),
    )])
}

/// A result of the revision 2026-07-28, which names its type; earlier clients ignore it.
fn complete(members: Vec<(&str, Value)>) -> Value {
    let mut all = vec![("resultType".to_owned(), Value::text("complete"))];
    all.extend(
        members
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value)),
    );
    Value::Object(all)
}

fn error_line(id: Option<&Value>, code: f64, message: &str, data: Option<Value>) -> String {
    let mut error = vec![
        ("code".to_owned(), Value::Number(code)),
        ("message".to_owned(), Value::text(message)),
    ];
    if let Some(data) = data {
        error.push(("data".to_owned(), data));
    }
    let mut members = vec![("jsonrpc".to_owned(), Value::text("2.0"))];
    if let Some(id) = id {
        members.push(("id".to_owned(), id.clone()));
    }
    members.push(("error".to_owned(), Value::Object(error)));
    Value::Object(members).to_line()
}

fn schema(properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
    Value::object([
        ("type", Value::text("object")),
        (
            "properties",
            Value::Object(
                properties
                    .into_iter()
                    .map(|(name, value)| (name.to_owned(), value))
                    .collect(),
            ),
        ),
        (
            "required",
            Value::Array(required.iter().map(|name| Value::text(*name)).collect()),
        ),
        ("additionalProperties", Value::Bool(false)),
    ])
}

/// What a host may assume of a tool, for its consent and its retries.
#[derive(Clone, Copy)]
struct Hints {
    read_only: bool,
    destructive: bool,
    idempotent: bool,
}

fn tool(name: &str, title: &str, description: &str, input: Value, hints: Hints) -> Value {
    Value::object([
        ("name", Value::text(name)),
        ("title", Value::text(title)),
        ("description", Value::text(description)),
        ("inputSchema", input),
        (
            "annotations",
            Value::object([
                ("title", Value::text(title)),
                ("readOnlyHint", Value::Bool(hints.read_only)),
                ("destructiveHint", Value::Bool(hints.destructive)),
                ("idempotentHint", Value::Bool(hints.idempotent)),
                // Everything stays in the application on this computer.
                ("openWorldHint", Value::Bool(false)),
            ]),
        ),
    ])
}

fn tools() -> Value {
    let text = |description: &str| {
        Value::object([
            ("type", Value::text("string")),
            ("description", Value::text(description)),
        ])
    };
    let count = |description: &str, minimum: f64, maximum: Option<f64>, default: f64| {
        let mut members = vec![
            ("type".to_owned(), Value::text("integer")),
            ("minimum".to_owned(), Value::Number(minimum)),
            ("default".to_owned(), Value::Number(default)),
            ("description".to_owned(), Value::text(description)),
        ];
        if let Some(maximum) = maximum {
            members.push(("maximum".to_owned(), Value::Number(maximum)));
        }
        Value::Object(members)
    };
    let reads = Hints {
        read_only: true,
        destructive: false,
        idempotent: true,
    };
    Value::Array(vec![
        tool(
            LIST,
            "Find Platter commands",
            "Lists the commands of Platter with their parameters, a page at a time: identifier, \
             parameters with their kinds and ranges, and label. A filter keeps the commands whose \
             identifier or label holds every word of it, such as `crossfader` or `deck loop`. \
             Returns the total and whether more pages follow. Reads only; to run a command use \
             platter_run_command.",
            schema(
                vec![
                    (
                        "filter",
                        text(
                            "Words the identifier or the label holds, such as `deck.play` or `filter`",
                        ),
                    ),
                    (
                        "limit",
                        count(
                            "Commands per page",
                            1.0,
                            Some(MAX_LIMIT as f64),
                            DEFAULT_LIMIT as f64,
                        ),
                    ),
                    (
                        "offset",
                        count(
                            "Commands to skip, from the next_offset of the page before",
                            0.0,
                            None,
                            0.0,
                        ),
                    ),
                ],
                &[],
            ),
            reads,
        ),
        tool(
            RUN,
            "Run a Platter command",
            "Runs one command of Platter by its identifier, with its parameters by name: numbers \
             as numbers, decks as a, b, c or d, choices and paths as text. A command does what the \
             same key of the interface does, so some change or delete things, such as \
             browser.track.delete. Returns what happened, or why the command was refused. The \
             identifiers and parameters come from platter_list_commands.",
            schema(
                vec![
                    (
                        "command",
                        text("The identifier of the command, such as deck.play_pause"),
                    ),
                    (
                        "arguments",
                        Value::object([
                            ("type", Value::text("object")),
                            (
                                "description",
                                Value::text(
                                    "The parameters of the command by name, such as {\"deck\": \"a\"}",
                                ),
                            ),
                            (
                                "additionalProperties",
                                Value::object([(
                                    "type",
                                    Value::Array(vec![
                                        Value::text("number"),
                                        Value::text("string"),
                                    ]),
                                )]),
                            ),
                        ]),
                    ),
                ],
                &["command"],
            ),
            Hints {
                read_only: false,
                destructive: true,
                idempotent: false,
            },
        ),
        tool(
            STATE,
            "Read the state of Platter",
            "Returns, as JSON, the four decks (track title, artist and file name, length, \
             position, playing, speed, tempo in beats per minute, sync, key lock, fader, and the \
             controls of each stem of a stem deck), the crossfader and the mixer mode, and the \
             tempo of the clock with its tempo master. Reads only.",
            schema(Vec::new(), &[]),
            reads,
        ),
    ])
}

/// A tool call. A tool that does not exist is an error of the protocol; a command refused
/// is a result marked as an error, for the model to read.
fn call(params: &Value, host: &mut dyn Host) -> Result<Value, (f64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "the call names no tool".to_owned()))?;
    let empty = Value::Object(Vec::new());
    let arguments = params.get("arguments").unwrap_or(&empty);
    let outcome = match name {
        LIST => {
            let page = list_commands(host, arguments);
            return Ok(complete(vec![
                ("content", text_content(&page.text)),
                ("structuredContent", page.structured),
                ("isError", Value::Bool(false)),
            ]));
        }
        RUN => run_command(host, arguments),
        STATE => {
            let state = host.state();
            return Ok(complete(vec![
                ("content", text_content(&state.to_line())),
                ("structuredContent", state),
                ("isError", Value::Bool(false)),
            ]));
        }
        other => return Err((INVALID_PARAMS, format!("Unknown tool: {other}"))),
    };
    let (text, failed) = match outcome {
        Ok(text) => (text, false),
        Err(text) => (text, true),
    };
    Ok(complete(vec![
        ("content", text_content(&text)),
        ("isError", Value::Bool(failed)),
    ]))
}

fn text_content(text: &str) -> Value {
    Value::Array(vec![Value::object([
        ("type", Value::text("text")),
        ("text", Value::text(text)),
    ])])
}

/// A page of the commands, as text for the model and as data.
struct CommandPage {
    text: String,
    structured: Value,
}

fn param_kind(kind: ParamKind) -> (String, Value) {
    match kind {
        ParamKind::Deck => (
            "a|b|c|d".to_owned(),
            Value::object([("kind", Value::text("deck"))]),
        ),
        ParamKind::Number { min, max } => (
            format!("number {min}..{max}"),
            Value::object([
                ("kind", Value::text("number")),
                ("min", Value::Number(min)),
                ("max", Value::Number(max)),
            ]),
        ),
        ParamKind::Choice(choices) => (
            choices.join("|"),
            Value::object([
                ("kind", Value::text("choice")),
                (
                    "choices",
                    Value::Array(choices.iter().map(|choice| Value::text(*choice)).collect()),
                ),
            ]),
        ),
        ParamKind::Path => (
            "path".to_owned(),
            Value::object([("kind", Value::text("path"))]),
        ),
        ParamKind::Text => (
            "text".to_owned(),
            Value::object([("kind", Value::text("text"))]),
        ),
    }
}

/// A whole number at least `minimum` from an argument, or `default` without one.
fn whole(arguments: &Value, name: &str, minimum: usize, default: usize) -> usize {
    match arguments.get(name) {
        Some(Value::Number(number)) if number.is_finite() && *number >= minimum as f64 => {
            number.min(usize::MAX as f64) as usize
        }
        _ => default,
    }
}

fn list_commands(host: &dyn Host, arguments: &Value) -> CommandPage {
    let words: Vec<String> = arguments
        .get("filter")
        .and_then(Value::as_str)
        .map(|filter| filter.split_whitespace().map(str::to_lowercase).collect())
        .unwrap_or_default();
    let limit = whole(arguments, "limit", 1, DEFAULT_LIMIT).min(MAX_LIMIT);
    let offset = whole(arguments, "offset", 0, 0);
    let matching: Vec<&CommandSpec> = host
        .commands()
        .into_iter()
        .filter(|spec| {
            let label = spec.label.to_lowercase();
            words
                .iter()
                .all(|word| spec.id.contains(word.as_str()) || label.contains(word.as_str()))
        })
        .collect();
    let total = matching.len();
    let page: Vec<&CommandSpec> = matching.into_iter().skip(offset).take(limit).collect();
    let next = offset.saturating_add(page.len());
    let has_more = next < total;
    let mut lines = Vec::new();
    let mut commands = Vec::new();
    for spec in &page {
        let mut shown = Vec::new();
        let mut params = Vec::new();
        for param in spec.params {
            let (kind, data) = param_kind(param.kind);
            shown.push(format!("{}: {kind}", param.name));
            let mut data = data;
            if let Value::Object(members) = &mut data {
                members.insert(0, ("name".to_owned(), Value::text(param.name)));
            }
            params.push(data);
        }
        lines.push(format!(
            "{}({}) - {}",
            spec.id,
            shown.join(", "),
            spec.label
        ));
        commands.push(Value::object([
            ("id", Value::text(spec.id)),
            ("label", Value::text(spec.label)),
            ("params", Value::Array(params)),
        ]));
    }
    let text = if total == 0 {
        "No command matches. Try fewer or other words, such as `deck`, `mixer` or `browser`."
            .to_owned()
    } else if page.is_empty() {
        format!("{total} commands match; the offset {offset} is past the last one.")
    } else {
        let mut text = format!(
            "Commands {} to {next} of {total}:\n{}",
            offset + 1,
            lines.join("\n")
        );
        if has_more {
            text.push_str(&format!("\nMore follow: ask again with offset {next}."));
        }
        text
    };
    let structured = Value::object([
        ("total", Value::Number(total as f64)),
        ("count", Value::Number(page.len() as f64)),
        ("offset", Value::Number(offset as f64)),
        ("has_more", Value::Bool(has_more)),
        (
            "next_offset",
            if has_more {
                Value::Number(next as f64)
            } else {
                Value::Null
            },
        ),
        ("commands", Value::Array(commands)),
    ]);
    CommandPage { text, structured }
}

fn run_command(host: &mut dyn Host, arguments: &Value) -> Result<String, String> {
    let id = arguments
        .get("command")
        .and_then(Value::as_str)
        .ok_or("`command` names no command: find one with platter_list_commands")?;
    let mut args = Args::new();
    if let Some(given) = arguments.get("arguments") {
        let Value::Object(members) = given else {
            return Err(
                "`arguments` is an object of parameters by name, such as {\"deck\": \"a\"}"
                    .to_owned(),
            );
        };
        for (name, value) in members {
            let value = match value {
                Value::Number(number) => ArgValue::Number(*number),
                Value::Text(text) => ArgValue::Text(text.clone()),
                _ => {
                    return Err(format!(
                        "the parameter {name} is a number or a text, not {}",
                        kind_of(value)
                    ));
                }
            };
            args.insert(name.clone(), value);
        }
    }
    host.run(id, &args).map_err(|why| {
        format!("{id} was not run: {why}. platter_list_commands with the filter `{id}` gives its parameters.")
    })
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::Text(_) => "a text",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use platter_engine::command::SPECS;

    use super::*;

    struct Fake {
        ran: Vec<(String, Args)>,
    }

    impl Host for Fake {
        fn commands(&self) -> Vec<&'static CommandSpec> {
            SPECS.iter().collect()
        }

        fn run(&mut self, id: &str, args: &Args) -> Result<String, String> {
            if id == "deck.refuse" {
                return Err("refused".to_owned());
            }
            self.ran.push((id.to_owned(), args.clone()));
            Ok(format!("ran {id}"))
        }

        fn state(&mut self) -> Value {
            Value::object([("decks", Value::Array(Vec::new()))])
        }
    }

    fn answer(line: &str) -> (Value, Fake) {
        let server = Server {
            name: "platter".to_owned(),
            version: "0.5.0".to_owned(),
        };
        let mut host = Fake { ran: Vec::new() };
        let reply = server.handle(line, &mut host).unwrap_or_default();
        assert!(!reply.contains('\n'));
        (json::parse(&reply).unwrap_or(Value::Null), host)
    }

    fn result(reply: &Value) -> &Value {
        reply.get("result").unwrap_or(&Value::Null)
    }

    #[test]
    fn an_earlier_client_gets_its_version_or_the_earlier_revision() {
        let (reply, _) = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
        );
        assert_eq!(
            result(&reply)
                .get("protocolVersion")
                .and_then(Value::as_str),
            Some("2025-11-25")
        );
        assert!(
            result(&reply)
                .get("capabilities")
                .and_then(|c| c.get("tools"))
                .is_some()
        );
        let (reply, _) = answer(
            r#"{"jsonrpc":"2.0","id":"x","method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        );
        assert_eq!(
            result(&reply)
                .get("protocolVersion")
                .and_then(Value::as_str),
            Some(LEGACY_VERSION)
        );
        assert_eq!(reply.get("id"), Some(&Value::text("x")));
    }

    #[test]
    fn discover_names_the_versions_and_an_unknown_one_is_refused() {
        let (reply, _) = answer(
            r#"{"jsonrpc":"2.0","id":2,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        );
        assert_eq!(
            result(&reply).get("resultType"),
            Some(&Value::text("complete"))
        );
        assert_eq!(result(&reply).get("supportedVersions"), Some(&supported()));
        let (reply, _) = answer(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}}}"#,
        );
        let error = reply.get("error").unwrap_or(&Value::Null);
        assert_eq!(error.get("code"), Some(&Value::Number(-32022.0)));
        assert_eq!(
            error.get("data").and_then(|data| data.get("requested")),
            Some(&Value::text("1900-01-01"))
        );
    }

    #[test]
    fn notifications_get_no_answer_and_bad_messages_get_errors() {
        let server = Server {
            name: "platter".to_owned(),
            version: "0".to_owned(),
        };
        let mut host = Fake { ran: Vec::new() };
        assert_eq!(
            server.handle(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &mut host
            ),
            None
        );
        for (line, code) in [
            ("{nope", -32700.0),
            (
                r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#,
                -32601.0,
            ),
            (r#"{"jsonrpc":"1.0","id":5,"method":"ping"}"#, -32600.0),
            (
                r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"dance"}}"#,
                -32602.0,
            ),
        ] {
            let (reply, _) = answer(line);
            let error = reply.get("error").unwrap_or(&Value::Null);
            assert_eq!(error.get("code"), Some(&Value::Number(code)), "{line}");
        }
        let (reply, _) = answer(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#);
        assert_eq!(result(&reply), &Value::Object(Vec::new()));
    }

    #[test]
    fn the_tools_list_run_and_read_the_commands() {
        let (reply, _) = answer(r#"{"jsonrpc":"2.0","id":8,"method":"tools/list"}"#);
        let Some(Value::Array(tools)) = result(&reply).get("tools") else {
            panic!("{reply:?}");
        };
        assert_eq!(tools.len(), 3);
        let text = |reply: &Value| {
            result(reply)
                .get("content")
                .and_then(|content| match content {
                    Value::Array(items) => items.first().and_then(|item| item.get("text")),
                    _ => None,
                })
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_default()
        };
        let (reply, _) = answer(
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"platter_list_commands","arguments":{"filter":"deck.play_pause"}}}"#,
        );
        assert!(
            text(&reply).contains("\ndeck.play_pause(deck: a|b|c|d) - "),
            "{reply:?}"
        );
        let (reply, host) = answer(
            r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"platter_run_command","arguments":{"command":"deck.tempo.set","arguments":{"deck":"a","ratio":1.02}}}}"#,
        );
        assert_eq!(result(&reply).get("isError"), Some(&Value::Bool(false)));
        assert_eq!(host.ran[0].0, "deck.tempo.set");
        assert_eq!(host.ran[0].1.get("ratio"), Some(&ArgValue::Number(1.02)));
        let (reply, _) = answer(
            r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"platter_run_command","arguments":{"command":"deck.refuse"}}}"#,
        );
        assert_eq!(result(&reply).get("isError"), Some(&Value::Bool(true)));
        assert!(
            text(&reply).starts_with("deck.refuse was not run: refused."),
            "{reply:?}"
        );
        let (reply, _) = answer(
            r#"{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"platter_get_state"}}"#,
        );
        assert!(result(&reply).get("structuredContent").is_some());
    }

    #[test]
    fn every_tool_carries_its_hints_and_only_the_runner_changes_anything() {
        let (reply, _) = answer(r#"{"jsonrpc":"2.0","id":13,"method":"tools/list"}"#);
        let Some(Value::Array(tools)) = result(&reply).get("tools") else {
            panic!("{reply:?}");
        };
        for tool in tools {
            let name = tool.get("name").and_then(Value::as_str).unwrap_or_default();
            assert!(name.starts_with("platter_") && name.len() <= 64, "{name}");
            let hints = tool.get("annotations").unwrap_or(&Value::Null);
            assert!(hints.get("title").is_some(), "{name}");
            let read_only = hints.get("readOnlyHint") == Some(&Value::Bool(true));
            assert_eq!(read_only, name != "platter_run_command", "{name}");
            assert_eq!(
                hints.get("openWorldHint"),
                Some(&Value::Bool(false)),
                "{name}"
            );
        }
    }

    #[test]
    fn the_commands_come_a_page_at_a_time() {
        let call = |arguments: &str| {
            let (reply, _) = answer(&format!(
                r#"{{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{{"name":"platter_list_commands","arguments":{arguments}}}}}"#
            ));
            result(&reply)
                .get("structuredContent")
                .cloned()
                .unwrap_or(Value::Null)
        };
        let first = call(r#"{"filter":"deck","limit":5}"#);
        let total = match first.get("total") {
            Some(Value::Number(total)) => *total,
            _ => 0.0,
        };
        assert!(total > 5.0, "{first:?}");
        assert_eq!(first.get("has_more"), Some(&Value::Bool(true)));
        assert_eq!(first.get("next_offset"), Some(&Value::Number(5.0)));
        let last = call(&format!(
            r#"{{"filter":"deck","limit":5,"offset":{}}}"#,
            total - 1.0
        ));
        assert_eq!(last.get("count"), Some(&Value::Number(1.0)));
        assert_eq!(last.get("has_more"), Some(&Value::Bool(false)));
        // Every word has to match, and a limit past the most is brought down to it.
        let both = call(r#"{"filter":"crossfader set","limit":1000}"#);
        let Some(Value::Array(commands)) = both.get("commands") else {
            panic!("{both:?}");
        };
        assert!(commands.len() <= MAX_LIMIT && !commands.is_empty());
        for command in commands {
            let id = command
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let label = command
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase();
            assert!(
                (id.contains("crossfader") || label.contains("crossfader"))
                    && (id.contains("set") || label.contains("set")),
                "{id}"
            );
        }
        let none = call(r#"{"filter":"teleport"}"#);
        assert_eq!(none.get("total"), Some(&Value::Number(0.0)));
    }
}
