# 0037. The MCP server: the application on standard input and output

Status: accepted (M4).

## Context

M4 adds an MCP server, so that an assistant that speaks the Model Context Protocol can drive
Platter through its commands, as the keyboard, the interface and MIDI do (`AGENTS.md`,
*Everything is a command*). The closed decisions forbid a network connection without an explicit
action of the user and allow no dependency the maintainer has not approved; JSON has none
approved for the distributed crates. The engine and the collection live in the window: the
engine is built again when the sample rate changes, and the collection runs on a worker the
window owns. The protocol has two revisions in use: 2026-07-28, where every request carries its
version and the server answers `server/discover`, and 2025-11-25, where a session starts with
`initialize`.

## Options

- **How a client reaches Platter.** A local socket or HTTP port the running application
  listens on, which is a network connection open while it runs and needs authorisation; a
  separate process that talks to the window through a pipe, with a second protocol between the
  two; or the application itself as the server on its standard input and output, started by the
  client. The third opens nothing: the user starts it by naming it in the configuration of the
  client.
- **JSON.** `serde_json`, not approved, or a reader and a writer of the project for the few
  structures of JSON-RPC, as the reader of the XML collection files is.
- **Tools.** One tool per command, some 260, which crowds the context of a model and
  changes with every command added; or a few tools over the registry: list the commands, run
  one, read the state.

## Choice

- `platter --mcp` opens the window as always and serves MCP on the standard input and output,
  one message per line (the stdio transport). Without the flag nothing reads the input. A
  thread reads the lines, bounded to 1 MiB each, and hands each message to the window, which
  answers it between two frames and wakes for it; the thread writes the answer. The end of the
  input closes the window, as the transport asks of a server. Nothing else is written to the
  standard output.
- The crate `platter-mcp` (L3) holds the JSON reader and writer, bounded in size, depth and
  members, and the server: it knows the commands only through `CommandSpec` and a trait the
  application implements, so it does not depend on the interface crate.
- Both revisions are served. A request whose `_meta` names a version the server does not speak
  gets the error -32022 with the versions it speaks; `initialize` gets the version asked for
  when the server speaks it, otherwise 2025-11-25. Results carry `resultType: "complete"`, which
  clients of the earlier revision ignore.
- Three tools, a search and a run over the registry plus the state, as the guidance for large
  sets of actions advises: `platter_list_commands`, paged and filtered, `platter_run_command` and
  `platter_get_state`. The names carry the service. Every tool declares its title and the hints
  of the protocol (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`): the
  reads apart from the one tool that changes things, which a client asks the user about.
  `platter_run_command` goes through `Action::from_args`, the path of the command line and of
  the mappings, so every command and every check of its parameters is the same as anywhere
  else. A refused command is a result marked as an error, with the next step to take; an unknown
  tool is an error of the protocol. The reads give their data as `structuredContent` as well as
  text.
- The state names tracks by their tags and the name of their file, without their folder.

## How it was measured

The tests of `platter-mcp` send the messages of both revisions and check the answers, the pages
and the hints; the tests of the application check the reading of lines and the state. A debug
build was run once as `platter --mcp` with a script of messages on its input, a profile of its
own and no MCP client: it answered `initialize`, `tools/list`, `server/discover` and the three
tools, moved the crossfader and closed when its input closed. `crates/mcp/evaluation.xml` holds
ten questions for an assistant; none was run with one yet.
